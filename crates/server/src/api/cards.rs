//! Cards: the card view of blocks of type `card`.

use std::num::IntErrorKind;

use axum::body::Bytes;
use axum::extract::{Path, RawQuery, State};
use fvoci_model::{Block, BlockType, Card, CardError, CardPatch, RefusalReason, Refusals};

use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::contract::bodies::NewCard;
use super::contract::params::{BoardId, CardId, DisableNotify, Page, PerPage, Preview, ReadToken};
use super::contract::tag;

use super::blocks::BLOCK_FIELDS;
use super::change_notification;
use super::{
    ApiResult, OptionalSession, RequiredSession, TRUE, by_read_token, created, json_response,
    query_param,
};
use super::{decode_with_document, require_board_permission, require_permission_through};
use crate::app::{App, CardPreviews};
use crate::auth::now_millis;
use crate::error::AppError;
use crate::permissions::BoardPermission;

/// The page of cards a request without `page` gets.
pub(super) const DEFAULT_PAGE: i64 = 0;
/// The number of cards a page has in a request without `perPage`.
pub(super) const DEFAULT_PER_PAGE: i64 = 100;

pub(super) fn routes() -> OpenApiRouter<App> {
    OpenApiRouter::new()
        .routes(routes!(handle_create_card, handle_get_cards))
        .routes(routes!(handle_get_card, handle_patch_card))
}

/// Reads the `page` and `perPage` of a list that is answered in pages. Both are read,
/// so that a request with both wrong is told of both.
pub(super) fn page_params(query: Option<&str>) -> Result<(i64, i64), AppError> {
    let page = int_query_param(query, "page", DEFAULT_PAGE);
    let per_page = int_query_param(query, "perPage", DEFAULT_PER_PAGE);
    match (page, per_page) {
        (Ok(page), Ok(per_page)) => Ok((page, per_page)),
        (Err(refusal), Ok(_)) | (Ok(_), Err(refusal)) => Err(refusal),
        (Err(page), Err(per_page)) => Err(page.and(per_page)),
    }
}

/// Reads a whole number from a query parameter; `default` if the parameter is absent
/// or empty.
pub(super) fn int_query_param(
    query: Option<&str>,
    name: &str,
    default: i64,
) -> Result<i64, AppError> {
    let text = query_param(query, name);
    if text.is_empty() {
        return Ok(default);
    }
    text.parse().map_err(|error: std::num::ParseIntError| {
        let reason = match error.kind() {
            IntErrorKind::PosOverflow | IntErrorKind::NegOverflow => "out of range",
            _ => "not a whole number",
        };
        AppError::bad_request(format_args!("invalid `{name}` parameter: {reason}"))
            .with_parameter(name)
    })
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
    permission: BoardPermission,
    denial: &str,
) -> Result<Block, AppError> {
    const WHAT: &str = "card";
    let block = match app.block(card_id).await {
        Ok(block) => block,
        Err(error) if error.is_not_found() => return Err(AppError::not_found(WHAT)),
        Err(error) => return Err(error),
    };
    require_permission_through(app, user_id, &block.board_id, permission, denial, WHAT).await?;
    if block.block_type != BlockType::Card {
        return Err(AppError::not_found(WHAT));
    }
    Ok(block)
}

/// Make a card
///
/// The server chooses the identifier, the creator and the times. For editors.
///
/// The value of a card relation property (of the `type` `relation`) is `null` or a list
/// of other cards of the board, in the trash or not, each once, at most 50. Any other is
/// 400 with the reason `relation_not_a_list`, `relation_to_itself`, `relation_repeated`,
/// `relation_too_many` (with the `limit`), `relation_not_a_card` (a block of the board
/// that is no card) or `relation_card_not_found`: no card of that identifier on the
/// board, which is also the answer for a card of another board. Every write of a card
/// checks so (`patchCard`, the block routes, `createBoardsAndBlocks`) the values it
/// changes, and does not ask again for a card that a value named before.
#[utoipa::path(
    post,
    path = "/boards/{boardId}/cards",
    operation_id = "createCard",
    tag = tag::CARDS,
    params(BoardId, DisableNotify),
    request_body = NewCard,
    responses((status = 201, description = "The card that was made.", body = Card)),
)]
async fn handle_create_card(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    Path(board_id): Path<String>,
    RawQuery(query): RawQuery,
    body: Bytes,
) -> ApiResult {
    let user_id = session.user_id.as_str();
    let notification = change_notification(query.as_deref());
    require_board_permission(
        &app,
        user_id,
        &board_id,
        BoardPermission::ManageBoardCards,
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
    app.check_body_files(&board_id, card.body.as_ref()).await?;
    card.board_id.clone_from(&board_id);
    card.populate(now_millis());
    card.validate().map_err(AppError::bad_request)?;

    // What the store refuses of the card's values it names as the block's `fields`.
    let card = app
        .create_card(card, &board_id, user_id, notification)
        .await
        .map_err(|error| error.moved(BLOCK_FIELDS, "/properties"))?;
    tracing::debug!(board_id, card_id = %card.id, user_id, "CreateCard");
    Ok(created(&card))
}

/// The cards of a board
///
/// One page of the board's cards, for the board's members, and with the board's read
/// token for whoever has it. Oldest first; the pages cut that order. A `page` or
/// `perPage` that is no whole number is 400. With `preview=true` each card has its
/// `preview` (the first image and the beginning of the text of its body), which a
/// list without it leaves out.
///
/// To a reader with the read token alone (no session, or a user who is no member of
/// the board) the cards name no users, as `getBlocks` gives them: `createdBy` and
/// `modifiedBy` are empty, the values of person properties are emptied, and values of
/// properties the board does not define are left out. The preview's text has no
/// mentions in it, and its image `getFile` gives with the same token.
#[utoipa::path(
    get,
    path = "/boards/{boardId}/cards",
    operation_id = "getCards",
    tag = tag::CARDS,
    params(BoardId, Page, PerPage, Preview, ReadToken),
    // With a read token no session is needed.
    security((), ("sessionCookie" = []), ("bearerToken" = [])),
    responses((status = 200, description = "The cards of the page.", body = Vec<Card>)),
)]
async fn handle_get_cards(
    State(app): State<App>,
    session: OptionalSession,
    Path(board_id): Path<String>,
    RawQuery(query): RawQuery,
) -> ApiResult {
    let query = query.as_deref();
    let user_id = session.user_id();
    let shared = by_read_token(&app, query, &board_id).await?;
    let has_read_token = shared.is_some();
    if user_id.is_empty() && !has_read_token {
        return Err(AppError::unauthorized("access denied to fetch cards"));
    }
    let app = shared.unwrap_or(app);
    // The board's person properties, for a reader with the token alone.
    let hidden_people = if has_read_token {
        // A board that is shared exists; one that went since is not there.
        let board = app.board(&board_id).await?;
        (!app.is_board_reader(user_id, &board_id).await?).then(|| board.person_properties())
    } else {
        require_board_permission(
            &app,
            user_id,
            &board_id,
            BoardPermission::ViewBoard,
            "access denied to fetch cards",
        )
        .await?;
        None
    };

    let (page, per_page) = page_params(query)?;
    let previews = if query_param(query, "preview") == TRUE {
        CardPreviews::With
    } else {
        CardPreviews::Without
    };

    let mut cards = app
        .cards_for_board(&board_id, page, per_page, previews)
        .await?;
    if let Some(people) = &hidden_people {
        for card in &mut cards {
            card.hide_people(people);
        }
    }
    tracing::debug!(
        board_id,
        user_id,
        page,
        per_page,
        count = cards.len(),
        "GetCards"
    );
    Ok(json_response(&cards))
}

/// Change a card
///
/// For editors of the card's board. To a user who is no member of that board there is
/// no such card.
#[utoipa::path(
    patch,
    path = "/cards/{cardId}",
    operation_id = "patchCard",
    tag = tag::CARDS,
    params(CardId, DisableNotify),
    request_body = CardPatch,
    responses((status = 200, description = "The card as it is now.", body = Card)),
)]
async fn handle_patch_card(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    Path(card_id): Path<String>,
    RawQuery(query): RawQuery,
    body: Bytes,
) -> ApiResult {
    let user_id = session.user_id.as_str();
    let notification = change_notification(query.as_deref());

    let card = card_with_permission(
        &app,
        user_id,
        &card_id,
        BoardPermission::ManageBoardCards,
        "access denied to patch card",
    )
    .await?;
    // The block, read to check the permission, is the card as it was.
    let (mut patch, document): (CardPatch, _) = decode_with_document(&body)?;
    patch.body = document;
    // Everything of the request's shape that is refused, before the store is asked.
    patch.check_request()?;
    app.check_body_files(&card.board_id, patch.body.new_body())
        .await?;

    let patched = app
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
    path = "/cards/{cardId}",
    operation_id = "getCard",
    tag = tag::CARDS,
    params(CardId),
    responses((status = 200, description = "The card.", body = Card)),
)]
async fn handle_get_card(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    Path(card_id): Path<String>,
) -> ApiResult {
    let user_id = session.user_id.as_str();
    let block = card_with_permission(
        &app,
        user_id,
        &card_id,
        BoardPermission::ViewBoard,
        "access denied to fetch card",
    )
    .await?;
    let card = app.card_of_block(&block).await?;
    tracing::debug!(board_id = %card.board_id, card_id = %card.id, user_id, "GetCard");
    Ok(json_response(&card))
}
