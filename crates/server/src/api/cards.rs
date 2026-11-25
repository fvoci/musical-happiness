//! Cards: the card view of blocks of type `card`.

use axum::body::Bytes;
use axum::extract::{Path, RawQuery};
use fvoci_model::{
    Block, BlockType, BodyVersion, BodyVersionContent, Card, CardError, CardPatch, KeptBodyVersion,
    RefusalReason, Refusals,
};

use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::contract::bodies::NewCard;
use super::contract::params::MemberWorkspaceId;
use super::contract::params::{
    BoardId, BodyVersionNumber, CardId, CardNumber, DisableNotify, Page, PerPage, Preview,
};
use super::contract::tag;
use super::workspace::InWorkspace;

use super::blocks::BLOCK_FIELDS;
use super::body_versions::{
    get_body_version, keep_body_version, list_body_versions, restore_body_version,
};
use super::change_notification;
use super::paging::paging_params;
use super::{ApiResult, TRUE, created, json_response, query_param};
use super::{decode_with_document, require_container_permission, require_permission_through};
use crate::app::{App, CardPreviews};
use crate::auth::now_millis;
use crate::error::AppError;
use crate::permissions::ContainerPermission;

pub(super) fn routes() -> OpenApiRouter<App> {
    OpenApiRouter::new()
        .routes(routes!(handle_create_card, handle_get_cards))
        .routes(routes!(handle_get_card, handle_patch_card))
        .routes(routes!(handle_get_card_by_number))
        .routes(routes!(
            handle_get_card_body_versions,
            handle_keep_card_body_version
        ))
        .routes(routes!(handle_get_card_body_version))
        .routes(routes!(handle_restore_card_body_version))
}

/// Returns the card with the given identifier for a handler of `/cards/{cardId}`, if
/// the user has the permission on the card's board: its block, read once.
///
/// The path names a card and no board. So for a user who may not read the card's board
/// there is no such card, and neither is there for anyone if the block is no card.
async fn card_with_permission(
    app: &App,
    user_id: &str,
    card_id: &str,
    permission: ContainerPermission,
    denial: &str,
) -> Result<Block, AppError> {
    const WHAT: &str = "card";
    let block = match app.block(card_id).await {
        Ok(block) => block,
        Err(error) if error.is_not_found() => return Err(AppError::not_found(WHAT)),
        Err(error) => return Err(error),
    };
    require_permission_through(app, user_id, &block.container_id, permission, denial, WHAT).await?;
    if block.block_type != BlockType::Card {
        return Err(AppError::not_found(WHAT));
    }
    Ok(block)
}

/// Make a card
///
/// The server chooses the identifier, the creator and the times. For editors.
///
/// `parentId` places an ordinary card below another live, non-template card of the same
/// board, at most three levels deep (top-level = 1, child = 2, grandchild = 3); empty,
/// `null`, or omitted makes a top-level card. Template cards (`isTemplate: true`) cannot
/// have a parent or sub-cards. Invalid parents or cycles answer 400 at `/parentId`
/// (depth > 3 with reason `card_too_deep` and `limit: 3`).
///
/// The value of a card relation property (of the `type` `relation`) is `null` or a list
/// of relation link objects (`{"card": "<cardId>", "kind": "fs" | "ss" | "ff" | "sf",
/// "lagDays": -3650..=3650}`), naming other cards of the board, in the trash or not,
/// each once (even if `kind` differs), at most 50 (a value over it from before may be
/// written with fewer of its cards, not with another). `kind` defaults to `"fs"` and
/// `lagDays` defaults to `0` when omitted, unknown fields and plain string arrays are
/// refused with 400, and stored links always include all three fields normalized. Any
/// other refusal is 400 with the reason `relation_not_a_list`, `relation_to_itself`,
/// `relation_repeated`, `relation_too_many` (with the `limit`), `relation_not_a_card`
/// (a block of the board that is no card) or `relation_card_not_found`: no card of that
/// identifier on the board, which is also the answer for a card of another board. Every
/// write of a card checks so (`patchCard`, the block routes, `createContainersAndBlocks`)
/// the values it changes, and does not ask again for a card that a value named before.
#[utoipa::path(
    post,
    path = "/w/{workspaceId}/boards/{boardId}/cards",
    operation_id = "createCard",
    tag = tag::CARDS,
    params(MemberWorkspaceId, BoardId, DisableNotify),
    request_body = NewCard,
    responses((status = 201, description = "The card that was made.", body = Card)),
)]
async fn handle_create_card(
    ws: InWorkspace,
    Path((_, board_id)): Path<(String, String)>,
    RawQuery(query): RawQuery,
    body: Bytes,
) -> ApiResult {
    let user_id = ws.session.user_id.as_str();
    let notification = change_notification(query.as_deref());
    require_container_permission(
        &ws.app,
        user_id,
        &board_id,
        ContainerPermission::ManageCards,
        "access denied to create card",
    )
    .await?;
    let (mut card, document): (Card, _) = decode_with_document(&body)?;
    // Everything of the request's shape that is refused, before the store is asked.
    let mut refused: Refusals = card.check_request().err().unwrap_or_default();
    if !card.board_id.is_empty() && card.board_id != board_id {
        refused.refuse(
            "boardId",
            RefusalReason::Invalid,
            CardError::BoardIdMismatch.to_string(),
        );
    }
    refused.into_result()?;
    card.body = document.into_body();
    ws.app
        .check_body_files(&board_id, card.body.as_ref())
        .await?;
    card.board_id.clone_from(&board_id);
    card.populate(now_millis());
    card.validate().map_err(AppError::bad_request)?;

    // What the store refuses of the card's values it names as the block's `fields`.
    let card = ws
        .app
        .create_card(card, &board_id, user_id, notification)
        .await
        .map_err(|error| error.moved(BLOCK_FIELDS, "/properties"))?;
    tracing::debug!(board_id, card_id = %card.id, user_id, "CreateCard");
    Ok(created(&card))
}

/// The cards of a board
///
/// One page of the board's cards, for the board's members. Oldest first; the pages cut
/// that order. With `preview=true` each card has its `preview` (the first image and the beginning of the
/// text of its body), which a list without it leaves out.
#[utoipa::path(
    get,
    path = "/w/{workspaceId}/boards/{boardId}/cards",
    operation_id = "getCards",
    tag = tag::CARDS,
    params(MemberWorkspaceId, BoardId, Page, PerPage, Preview),
    responses((status = 200, description = "The cards of the page.", body = Vec<Card>)),
)]
async fn handle_get_cards(
    ws: InWorkspace,
    Path((_, board_id)): Path<(String, String)>,
    RawQuery(query): RawQuery,
) -> ApiResult {
    let user_id = &ws.session.user_id;
    require_container_permission(
        &ws.app,
        user_id,
        &board_id,
        ContainerPermission::View,
        "access denied to fetch cards",
    )
    .await?;
    let cards = cards_page(&ws.app, &board_id, query.as_deref()).await?;
    tracing::debug!(board_id, user_id, count = cards.len(), "GetCards");
    Ok(json_response(&cards))
}

/// The page of the board's cards that the query of a request asks for, as `getCards`
/// describes it; for whoever may read the board.
pub(super) async fn cards_page(
    app: &App,
    board_id: &str,
    query: Option<&str>,
) -> Result<Vec<Card>, AppError> {
    let page = paging_params(query)?;
    let previews = if query_param(query, "preview") == TRUE {
        CardPreviews::With
    } else {
        CardPreviews::Without
    };
    app.cards_for_board(board_id, page, previews).await
}

/// Change a card
///
/// For editors of the card's board. To a user who is no member of that board there is
/// no such card. The relation values it changes are checked as `createCard` says.
/// `parentId` reparents the card within the same board (empty or `null` moves it to the
/// top level; left out keeps its current parent), checked for cycles and maximum depth 3
/// including any sub-cards below it (400 at `/parentId`, `card_too_deep` when depth > 3).
#[utoipa::path(
    patch,
    path = "/w/{workspaceId}/cards/{cardId}",
    operation_id = "patchCard",
    tag = tag::CARDS,
    params(MemberWorkspaceId, CardId, DisableNotify),
    request_body = CardPatch,
    responses((status = 200, description = "The card as it is now.", body = Card)),
)]
async fn handle_patch_card(
    ws: InWorkspace,
    Path((_, card_id)): Path<(String, String)>,
    RawQuery(query): RawQuery,
    body: Bytes,
) -> ApiResult {
    let user_id = ws.session.user_id.as_str();
    let notification = change_notification(query.as_deref());

    let card = card_with_permission(
        &ws.app,
        user_id,
        &card_id,
        ContainerPermission::ManageCards,
        "access denied to patch card",
    )
    .await?;
    // The block, read to check the permission, is the card as it was.
    let (mut patch, document): (CardPatch, _) = decode_with_document(&body)?;
    patch.body = document;
    // Everything of the request's shape that is refused, before the store is asked.
    patch.check_request()?;
    ws.app
        .check_body_files(&card.container_id, patch.body.new_body())
        .await?;

    let patched = ws
        .app
        .patch_card(&patch, &card.id, user_id, notification)
        .await
        .map_err(|error| error.moved(BLOCK_FIELDS, "/updatedProperties"))?;
    tracing::debug!(board_id = %patched.board_id, card_id = %patched.id, user_id, "PatchCard");
    Ok(json_response(&patched))
}

/// A card
///
/// For the members of the card's board. To a user who is no member of it there is no
/// such card.
#[utoipa::path(
    get,
    path = "/w/{workspaceId}/cards/{cardId}",
    operation_id = "getCard",
    tag = tag::CARDS,
    params(MemberWorkspaceId, CardId),
    responses((status = 200, description = "The card.", body = Card)),
)]
async fn handle_get_card(ws: InWorkspace, Path((_, card_id)): Path<(String, String)>) -> ApiResult {
    let user_id = ws.session.user_id.as_str();
    let block = card_with_permission(
        &ws.app,
        user_id,
        &card_id,
        ContainerPermission::View,
        "access denied to fetch card",
    )
    .await?;
    let card = ws.app.card_of_block(&block).await?;
    tracing::debug!(board_id = %card.board_id, card_id = %card.id, user_id, "GetCard");
    Ok(json_response(&card))
}

/// A card by its number
///
/// The card of the board that has the number (`Card.number`, as `#12` names the card),
/// for the members of the board, as `getCard` gives it. One answer, 404, says that there
/// is no such card, whatever the reason: no card of the board has the number (the card
/// is in the trash, is gone for good, or is a template of the board's cards), the
/// number is none, the board is one the user does not read (a private board they are
/// no member of, one they only manage, one they are no guest of), or is no board of
/// this workspace (of another workspace or organization, a wiki, or none at all).
#[utoipa::path(
    get,
    path = "/w/{workspaceId}/boards/{boardId}/cards/by-number/{number}",
    operation_id = "getCardByNumber",
    tag = tag::CARDS,
    params(MemberWorkspaceId, BoardId, CardNumber),
    responses((status = 200, description = "The card.", body = Card)),
)]
async fn handle_get_card_by_number(
    ws: InWorkspace,
    Path((_, board_id, number)): Path<(String, String, String)>,
) -> ApiResult {
    const WHAT: &str = "card";
    let user_id = ws.session.user_id.as_str();
    let denial = "access denied to fetch card";
    let view = ContainerPermission::View;
    require_permission_through(&ws.app, user_id, &board_id, view, denial, WHAT).await?;
    // A number as `Card.number` writes it, so that a card has one path: not `+7` or `07`.
    let number = number
        .parse::<u32>()
        .ok()
        .filter(|parsed| parsed.to_string() == number)
        .ok_or_else(|| AppError::not_found(WHAT))?;
    let block = ws
        .app
        .card_by_number(&board_id, number)
        .await?
        .ok_or_else(|| AppError::not_found(WHAT))?;
    let card = ws.app.card_of_block(&block).await?;
    tracing::debug!(board_id, card_id = %card.id, user_id, "GetCardByNumber");
    Ok(json_response(&card))
}

/// The versions kept of a card's body
///
/// The server keeps the body of a card as a version each time it changed: when the
/// last of those who wrote it since the newest version leaves it, at an interval
/// while it is edited on (its setting `collab_version_minutes`, ten minutes unless
/// set otherwise), and before and after a write of the whole body (`patchCard` and the
/// block routes). The newest first, each with when it was kept and who wrote the body
/// up to it, without the bodies (`getCardBodyVersion`). The server keeps the newest
/// of them (its setting `collab_body_versions`, fifty unless set otherwise), so the
/// numbers may leave the lowest out. Empty for a card whose body never changed since
/// versions were kept. For the members of the card's board, whatever
/// their role; to a user who is no member of it there is no such card, and a reader by
/// the board's read token has no session to ask with. A card in the trash is not
/// there (404); its versions come back with it, and go when it is gone for good.
#[utoipa::path(
    get,
    path = "/w/{workspaceId}/cards/{cardId}/body-versions",
    operation_id = "getCardBodyVersions",
    tag = tag::CARDS,
    params(MemberWorkspaceId, CardId),
    responses((status = 200, description = "The versions, the newest first.", body = Vec<BodyVersion>)),
)]
async fn handle_get_card_body_versions(
    ws: InWorkspace,
    Path((_, card_id)): Path<(String, String)>,
) -> ApiResult {
    let user_id = ws.session.user_id.as_str();
    let card = card_with_permission(
        &ws.app,
        user_id,
        &card_id,
        ContainerPermission::View,
        "access denied to fetch card",
    )
    .await?;
    list_body_versions(&ws.app, &card.id).await
}

/// Keep a card's body as a version now
///
/// Keeps the body the card has as a version, by hand (`cause` `manual`), as an editor
/// asks for one: what those who edit the card together typed is written to it first, so
/// that the version is their body of that moment, and what they see and type goes on as
/// it was. The version names who wrote the body since the one before, as every version
/// does, and is as of now; the caller is not named for asking. It counts with the
/// others against the versions kept, the oldest going. After it, the next version the
/// server keeps on its own is of what is typed after it.
///
/// Nothing is kept (`outcome` `unchanged`, `version` the newest) of a body that is the
/// card's newest version already, nor (`outcome` `recent`, `version` that one) within
/// a minute of a version kept by hand, the server then keeping what was typed since on
/// its own. For editors of the card's board, as `restoreCardBodyVersion` is (403 for
/// its other members); to a user who is no member of it there is no such card, and a
/// card in the trash is not there (404). 503 (`document_unavailable`) if what was typed
/// cannot be written to the card now.
#[utoipa::path(
    post,
    path = "/w/{workspaceId}/cards/{cardId}/body-versions",
    operation_id = "keepCardBodyVersion",
    tag = tag::CARDS,
    params(MemberWorkspaceId, CardId),
    responses((status = 200, description = "What became of the body.", body = KeptBodyVersion)),
)]
async fn handle_keep_card_body_version(
    ws: InWorkspace,
    Path((_, card_id)): Path<(String, String)>,
) -> ApiResult {
    let user_id = ws.session.user_id.as_str();
    let card = card_with_permission(
        &ws.app,
        user_id,
        &card_id,
        ContainerPermission::ManageCards,
        "access denied to keep a version of the card's body",
    )
    .await?;
    let kept = keep_body_version(&ws.app, &card.id).await?;
    tracing::debug!(card_id = %card.id, user_id, outcome = ?kept.outcome, "KeepCardBodyVersion");
    Ok(json_response(&kept))
}

/// A version kept of a card's body
///
/// The version with its body, as `getCard` gives a body. For whom
/// `getCardBodyVersions` is for. A number the card has no version of, or has none of
/// any more, is 404.
#[utoipa::path(
    get,
    path = "/w/{workspaceId}/cards/{cardId}/body-versions/{version}",
    operation_id = "getCardBodyVersion",
    tag = tag::CARDS,
    params(MemberWorkspaceId, CardId, BodyVersionNumber),
    responses((status = 200, description = "The version.", body = BodyVersionContent)),
)]
async fn handle_get_card_body_version(
    ws: InWorkspace,
    Path((_, card_id, version)): Path<(String, String, String)>,
) -> ApiResult {
    let user_id = ws.session.user_id.as_str();
    let card = card_with_permission(
        &ws.app,
        user_id,
        &card_id,
        ContainerPermission::View,
        "access denied to fetch card",
    )
    .await?;
    get_body_version(&ws.app, &card.id, &version).await
}

/// Go back to a version of a card's body
///
/// Writes the body of the version to the card again, as a write of the whole body by
/// the caller: those who edit the card together are sent the body as a new document
/// (`contracts/websocket.md`), the body it replaces is kept as a version first, with
/// what they typed, and the body written is the card's newest version, by the caller.
/// The versions before stay as they are. A card that has that body already is left as
/// it is. For editors of the card's board (403 for its other members); to a user who is
/// no member of it there is no such card. A number the card has no version of, or has
/// none of any more, is 404. A version whose body may not be stored any more, or names
/// a file (an image, a file block) the board no longer has, is 409.
#[utoipa::path(
    post,
    path = "/w/{workspaceId}/cards/{cardId}/body-versions/{version}/restore",
    operation_id = "restoreCardBodyVersion",
    tag = tag::CARDS,
    params(MemberWorkspaceId, CardId, BodyVersionNumber),
    responses((status = 200, description = "The card as it is now.", body = Card)),
)]
async fn handle_restore_card_body_version(
    ws: InWorkspace,
    Path((_, card_id, version)): Path<(String, String, String)>,
) -> ApiResult {
    let user_id = ws.session.user_id.as_str();
    let card = card_with_permission(
        &ws.app,
        user_id,
        &card_id,
        ContainerPermission::ManageCards,
        "access denied to restore a version of the card's body",
    )
    .await?;
    let block = restore_body_version(&ws.app, &card, &version, user_id).await?;
    let restored = ws.app.card_of_block(&block).await?;
    tracing::debug!(
        board_id = %restored.board_id,
        card_id = %restored.id,
        user_id,
        "RestoreCardBodyVersion"
    );
    Ok(json_response(&restored))
}
