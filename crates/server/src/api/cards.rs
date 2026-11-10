//! Cards: the card view of blocks of type `card`.

use std::num::IntErrorKind;

use axum::Router;
use axum::body::Bytes;
use axum::extract::{Path, RawQuery, State};
use axum::routing::{get, post};
use fvoci_model::{BlockType, Card, CardError, CardPatch};

use super::change_notification;
use super::{ApiResult, RequiredSession, created, json_response, query_param};
use super::{decode_object, require_board_permission, require_permission_through};
use crate::app::App;
use crate::auth::now_millis;
use crate::error::AppError;
use crate::permissions::BoardPermission;

/// The page of cards a request without `page` gets.
const DEFAULT_PAGE: i64 = 0;
/// The number of cards a page has in a request without `perPage`.
const DEFAULT_PER_PAGE: i64 = 100;

pub(super) fn routes() -> Router<App> {
    Router::new()
        .route(
            "/boards/{board_id}/cards",
            post(handle_create_card).get(handle_get_cards),
        )
        .route(
            "/cards/{card_id}",
            get(handle_get_card).patch(handle_patch_card),
        )
}

/// Reads a whole number from a query parameter; `default` if the parameter is absent
/// or empty.
fn int_query_param(query: Option<&str>, name: &str, default: i64) -> Result<i64, AppError> {
    let text = query_param(query, name);
    if text.is_empty() {
        return Ok(default);
    }
    text.parse().map_err(|error: std::num::ParseIntError| {
        let reason = match error.kind() {
            IntErrorKind::PosOverflow | IntErrorKind::NegOverflow => "out of range",
            _ => "not a whole number",
        };
        AppError::bad_request(format_args!("invalid `{name}` parameter: {reason}")).with_field(name)
    })
}

/// Returns the card with the given identifier for a handler of `/cards/{card_id}`, if
/// the user has the permission on the card's board.
///
/// The path names a card and no board. So for a user who may not read the card's board
/// there is no such card, and neither is there for anyone if the block is no card.
async fn card_with_permission(
    app: &App,
    user_id: &str,
    card_id: &str,
    permission: BoardPermission,
    denial: &str,
) -> Result<Card, AppError> {
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
    app.card(card_id).await
}

/// `POST /boards/{board_id}/cards`: creates a card on the board and answers with it.
///
/// The server chooses the identifier, the creator and the times.
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
    let mut card: Card = decode_object(&body)?;
    if !card.board_id.is_empty() && card.board_id != board_id {
        return Err(CardError::BoardIdMismatch.into());
    }
    card.board_id.clone_from(&board_id);
    card.populate(now_millis());
    card.validate().map_err(AppError::bad_request)?;

    let card = app
        .create_card(card, &board_id, user_id, notification)
        .await?;
    tracing::debug!(board_id, card_id = %card.id, user_id, "CreateCard");
    Ok(created(&card))
}

/// `GET /boards/{board_id}/cards`: one page of the board's cards, for everyone who may
/// view the board. `page` counts from zero and defaults to it; `perPage` defaults to
/// 100, and zero or less asks for all cards.
async fn handle_get_cards(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    Path(board_id): Path<String>,
    RawQuery(query): RawQuery,
) -> ApiResult {
    let query = query.as_deref();
    let user_id = session.user_id.as_str();
    require_board_permission(
        &app,
        user_id,
        &board_id,
        BoardPermission::ViewBoard,
        "access denied to fetch cards",
    )
    .await?;

    let page = int_query_param(query, "page", DEFAULT_PAGE)?;
    let per_page = int_query_param(query, "perPage", DEFAULT_PER_PAGE)?;

    let cards = app.cards_for_board(&board_id, page, per_page).await?;
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

/// `PATCH /cards/{card_id}`: partially updates a card and answers with the result.
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
    let patch: CardPatch = decode_object(&body)?;

    let patched = app
        .patch_card(&patch, &card.id, user_id, notification)
        .await?;
    tracing::debug!(board_id = %patched.board_id, card_id = %patched.id, user_id, "PatchCard");
    Ok(json_response(&patched))
}

/// `GET /cards/{card_id}`: one card, for everyone who may view its board.
async fn handle_get_card(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    Path(card_id): Path<String>,
) -> ApiResult {
    let user_id = session.user_id.as_str();
    let card = card_with_permission(
        &app,
        user_id,
        &card_id,
        BoardPermission::ViewBoard,
        "access denied to fetch card",
    )
    .await?;
    tracing::debug!(board_id = %card.board_id, card_id = %card.id, user_id, "GetCard");
    Ok(json_response(&card))
}
