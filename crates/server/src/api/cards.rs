//! Cards: the card view of blocks of type `card`.

use std::num::IntErrorKind;

use axum::Router;
use axum::body::Bytes;
use axum::extract::{Path, RawQuery, State};
use axum::routing::{get, post};
use fvoci_model::{Card, CardError, CardPatch};

use super::change_notification;
use super::{ApiResult, RequiredSession, json_response, query_param};
use super::{decode_object_or_bad_request, require_board_permission};
use crate::app::App;
use crate::auth::now_millis;
use crate::error::AppError;
use crate::permissions::BoardPermission;

/// The page of cards a request without `page` gets.
const DEFAULT_PAGE: i64 = 0;
/// The number of cards a page has in a request without `per_page`.
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
/// or empty. The refusal is worded as the original's number parser words it.
fn int_query_param(query: Option<&str>, name: &str, default: i64) -> Result<i64, AppError> {
    let text = query_param(query, name);
    if text.is_empty() {
        return Ok(default);
    }
    text.parse().map_err(|error: std::num::ParseIntError| {
        let reason = match error.kind() {
            IntErrorKind::PosOverflow | IntErrorKind::NegOverflow => "value out of range",
            _ => "invalid syntax",
        };
        AppError::bad_request(format_args!(
            "invalid `{name}` parameter: strconv.Atoi: parsing {text:?}: {reason}"
        ))
    })
}

/// Returns the card with the given identifier for a handler of `/cards/{card_id}`.
///
/// Preserved quirk: whatever goes wrong, a block that does not exist or is no card
/// included, is a bad request that says why (server/api/cards.go:279-284, :356-361).
/// The handlers ask for the card before they check any permission, so this answers
/// every signed-in user.
async fn fetch_card(app: &App, card_id: &str) -> Result<Card, AppError> {
    app.card(card_id).await.map_err(|error| {
        AppError::bad_request(format_args!("could not fetch card {card_id}: {error}"))
    })
}

/// `POST /boards/{board_id}/cards`: creates a card on the board and answers with it.
///
/// The server chooses the identifier, the creator and the times. The body is decoded
/// before the permission is checked, so a body that is no card is a bad request for
/// everyone; a `null` body is a server error, as in the original.
async fn handle_create_card(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    Path(board_id): Path<String>,
    RawQuery(query): RawQuery,
    body: Bytes,
) -> ApiResult {
    let user_id = session.user_id.as_str();
    let notification = change_notification(query.as_deref());
    let mut card: Card = decode_object_or_bad_request(&body)?;

    require_board_permission(
        &app,
        user_id,
        &board_id,
        BoardPermission::ManageBoardCards,
        "access denied to create card",
    )
    .await?;
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
    Ok(json_response(&card))
}

/// `GET /boards/{board_id}/cards`: one page of the board's cards, for everyone who may
/// view the board. `page` counts from zero and defaults to it; `per_page` defaults to
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

    // The original writes its refusal of a malformed number and then, for want of a
    // `return`, carries on and writes the cards after it (server/api/cards.go:186-196).
    // The refusal is the whole answer here.
    let page = int_query_param(query, "page", DEFAULT_PAGE)?;
    let per_page = int_query_param(query, "per_page", DEFAULT_PER_PAGE)?;

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
///
/// The card is fetched first, then the permission on its board is checked, and only
/// then the body is decoded: a body that is no patch is a bad request, `null` a server
/// error, as in the original.
async fn handle_patch_card(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    Path(card_id): Path<String>,
    RawQuery(query): RawQuery,
    body: Bytes,
) -> ApiResult {
    let user_id = session.user_id.as_str();
    let notification = change_notification(query.as_deref());

    let card = fetch_card(&app, &card_id).await?;
    require_board_permission(
        &app,
        user_id,
        &card.board_id,
        BoardPermission::ManageBoardCards,
        "access denied to patch card",
    )
    .await?;
    let patch: CardPatch = decode_object_or_bad_request(&body)?;

    let patched = app
        .patch_card(&patch, &card.id, user_id, notification)
        .await?;
    tracing::debug!(board_id = %patched.board_id, card_id = %patched.id, user_id, "PatchCard");
    Ok(json_response(&patched))
}

/// `GET /cards/{card_id}`: one card.
///
/// Preserved quirk: reading a single card takes the permission to manage the board's
/// cards, so viewers and commenters, who get the card in the board's list of cards,
/// are refused here (server/api/cards.go:363).
async fn handle_get_card(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    Path(card_id): Path<String>,
) -> ApiResult {
    let user_id = session.user_id.as_str();
    let card = fetch_card(&app, &card_id).await?;
    require_board_permission(
        &app,
        user_id,
        &card.board_id,
        BoardPermission::ViewBoard,
        "access denied to fetch card",
    )
    .await?;
    tracing::debug!(board_id = %card.board_id, card_id = %card.id, user_id, "GetCard");
    Ok(json_response(&card))
}
