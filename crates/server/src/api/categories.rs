//! Sidebar categories and the boards filed under them.

use axum::Router;
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::header::CONTENT_TYPE;
use axum::response::{IntoResponse, Response};
use axum::routing::{post, put};
use fvoci_model::{Category, CategoryType};
use fvoci_store::BoardVisibility;
use serde_json::Value;

use super::decode_object_members;
use super::require_team_access;
use super::{ApiResult, RequiredSession, decode_json_or_internal, empty_response, json_response};
use crate::app::App;
use crate::auth::now_millis;
use crate::error::AppError;

/// The refusal of the routes that take the team for the thing asked about.
const ACCESS_DENIED_TO_TEAM: &str = "access denied to team";
/// The refusal of the routes that reorder or hide, whatever was denied.
const ACCESS_DENIED_TO_CATEGORY: &str = "access denied to category";

pub(super) fn routes() -> Router<App> {
    Router::new()
        .route(
            "/teams/{team_id}/categories",
            post(handle_create_category).get(handle_get_user_category_boards),
        )
        .route(
            "/teams/{team_id}/categories/reorder",
            put(handle_reorder_categories).delete(handle_delete_category_named_reorder),
        )
        .route(
            "/teams/{team_id}/categories/{category_id}",
            put(handle_update_category).delete(handle_delete_category),
        )
        .route(
            "/teams/{team_id}/categories/{category_id}/boards/reorder",
            put(handle_reorder_category_boards).post(handle_update_category_board_named_reorder),
        )
        .route(
            "/teams/{team_id}/categories/{category_id}/boards/{board_id}",
            post(handle_update_category_board),
        )
        .route(
            "/teams/{team_id}/categories/{category_id}/boards/{board_id}/hide",
            put(handle_hide_board),
        )
        .route(
            "/teams/{team_id}/categories/{category_id}/boards/{board_id}/unhide",
            put(handle_unhide_board),
        )
}

/// The path parameters of the routes that address a board in a category: the team, the
/// category and the board.
type BoardPlace = (String, String, String);

/// The word that names the reorder routes where other routes have an identifier.
///
/// Preserved quirk: the original does not reserve it. With another method than the
/// reorder routes', a path that ends in it addresses the category or the board that has
/// the word as its identifier (`server/api/categories.go:17-23`).
const REORDER: &str = "reorder";

async fn handle_delete_category_named_reorder(
    app: State<App>,
    session: RequiredSession,
    Path(team_id): Path<String>,
) -> ApiResult {
    handle_delete_category(app, session, Path((team_id, REORDER.to_owned()))).await
}

async fn handle_update_category_board_named_reorder(
    app: State<App>,
    session: RequiredSession,
    Path((team_id, category_id)): Path<(String, String)>,
) -> ApiResult {
    let place = (team_id, category_id, REORDER.to_owned());
    handle_update_category_board(app, session, Path(place)).await
}

/// A category as a request body carries it.
struct CategoryRequest {
    category: Category,
    /// Whether the body's `type` is one a category can have. Reported late, see
    /// [`CategoryRequest::into_category`].
    type_is_valid: bool,
}

impl CategoryRequest {
    /// Decodes a category the way the original does: `null`, as the body or as a
    /// member, leaves the default, and so does a blank `type`. Any other malformed body
    /// is a server error, because the original does not classify the decoding error.
    fn decode(body: &[u8]) -> Result<Self, AppError> {
        let mut members = decode_object_members(body)?;
        let unknown_type_is_blank = match members.get("type") {
            Some(Value::String(text)) if !is_category_type(text) => Some(text.trim().is_empty()),
            _ => None,
        };
        if unknown_type_is_blank.is_some() {
            members.remove("type");
        }
        let category =
            serde_json::from_value(Value::Object(members)).map_err(AppError::internal)?;
        Ok(Self {
            category,
            type_is_valid: unknown_type_is_blank.unwrap_or(true),
        })
    }

    /// The category, if its type is valid. If not, the answer is the bad request the
    /// original gives: it checks the type after the identifier, the name, the user and
    /// the team, so a category with one of those blank is refused for that.
    fn into_category(self) -> Result<Category, AppError> {
        let mut category = self.category;
        if self.type_is_valid {
            return Ok(category);
        }
        category.hydrate(now_millis());
        category.validate()?;
        Err(AppError::bad_request(format_args!(
            "category type is invalid. Allowed types: {} and {}",
            CategoryType::System.as_str(),
            CategoryType::Custom.as_str(),
        )))
    }
}

fn is_category_type(text: &str) -> bool {
    [CategoryType::System, CategoryType::Custom]
        .iter()
        .any(|category_type| category_type.as_str() == text)
}

/// Decodes the list of identifiers of a reorder request. `null` is the empty list.
fn decode_order(body: &[u8]) -> Result<Vec<String>, AppError> {
    let order: Option<Vec<String>> = decode_json_or_internal(body)?;
    Ok(order.unwrap_or_default())
}

/// The answer of a reorder request: the new order.
///
/// Preserved quirk: an empty order, which is accepted when there is nothing to order, is
/// answered with `null` (`server/services/store/sqlstore/category.go:215`,
/// `category_boards.go:138`).
fn order_response(order: &[String]) -> Response {
    if order.is_empty() {
        json_response(&Value::Null)
    } else {
        json_response(&order)
    }
}

async fn handle_create_category(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    Path(team_id): Path<String>,
    body: Bytes,
) -> ApiResult {
    let request = CategoryRequest::decode(&body)?;
    let category = &request.category;
    if category.user_id != session.user_id {
        return Err(AppError::bad_request(format_args!(
            "userID {} and category userID {} mismatch",
            session.user_id, category.user_id
        )));
    }
    if category.team_id != team_id {
        return Err(AppError::bad_request("teamID mismatch"));
    }
    require_team_access(&app, &session.user_id, &team_id, ACCESS_DENIED_TO_TEAM)?;

    let created = app.create_category(request.into_category()?).await?;
    Ok(json_response(&created))
}

async fn handle_update_category(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    Path((team_id, category_id)): Path<(String, String)>,
    body: Bytes,
) -> ApiResult {
    let request = CategoryRequest::decode(&body)?;
    let category = &request.category;
    if category.id != category_id {
        return Err(AppError::bad_request(
            "categoryID mismatch in patch and body",
        ));
    }
    if category.user_id != session.user_id {
        return Err(AppError::bad_request(
            "user ID mismatch in session and category",
        ));
    }
    if category.team_id != team_id {
        return Err(AppError::bad_request("teamID mismatch"));
    }
    require_team_access(&app, &session.user_id, &team_id, ACCESS_DENIED_TO_TEAM)?;

    let updated = app.update_category(request.into_category()?).await?;
    Ok(json_response(&updated))
}

async fn handle_delete_category(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    Path((team_id, category_id)): Path<(String, String)>,
) -> ApiResult {
    require_team_access(&app, &session.user_id, &team_id, ACCESS_DENIED_TO_TEAM)?;
    let deleted = app
        .delete_category(&category_id, &session.user_id, &team_id)
        .await?;
    Ok(json_response(&deleted))
}

async fn handle_get_user_category_boards(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    Path(team_id): Path<String>,
) -> ApiResult {
    require_team_access(&app, &session.user_id, &team_id, ACCESS_DENIED_TO_TEAM)?;
    let category_boards = app.user_category_boards(&session.user_id, &team_id).await?;
    Ok(json_response(&category_boards))
}

async fn handle_update_category_board(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    Path((team_id, category_id, board_id)): Path<BoardPlace>,
) -> ApiResult {
    require_team_access(&app, &session.user_id, &team_id, ACCESS_DENIED_TO_TEAM)?;
    app.add_update_user_category_board(&team_id, &session.user_id, &category_id, &[board_id])
        .await?;
    // Preserved quirk: the body is the bare word, which is not JSON, labelled as JSON
    // (server/api/categories.go:391).
    Ok(([(CONTENT_TYPE, "application/json")], "ok").into_response())
}

async fn handle_reorder_categories(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    Path(team_id): Path<String>,
    body: Bytes,
) -> ApiResult {
    require_team_access(&app, &session.user_id, &team_id, ACCESS_DENIED_TO_CATEGORY)?;
    let new_order = decode_order(&body)?;
    let order = app
        .reorder_categories(&session.user_id, &team_id, new_order)
        .await?;
    Ok(order_response(&order))
}

async fn handle_reorder_category_boards(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    Path((team_id, category_id)): Path<(String, String)>,
    body: Bytes,
) -> ApiResult {
    require_team_access(&app, &session.user_id, &team_id, ACCESS_DENIED_TO_CATEGORY)?;
    let category = app.category(&category_id).await?;
    if category.user_id != session.user_id {
        return Err(AppError::forbidden(ACCESS_DENIED_TO_CATEGORY));
    }
    let new_order = decode_order(&body)?;
    let order = app
        .reorder_category_boards(&session.user_id, &team_id, &category_id, new_order)
        .await?;
    Ok(order_response(&order))
}

async fn handle_hide_board(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    Path(place): Path<BoardPlace>,
) -> ApiResult {
    set_board_visibility(&app, &session.user_id, &place, BoardVisibility::Hidden).await
}

async fn handle_unhide_board(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    Path(place): Path<BoardPlace>,
) -> ApiResult {
    set_board_visibility(&app, &session.user_id, &place, BoardVisibility::Visible).await
}

async fn set_board_visibility(
    app: &App,
    user_id: &str,
    (team_id, category_id, board_id): &BoardPlace,
    visibility: BoardVisibility,
) -> ApiResult {
    require_team_access(app, user_id, team_id, ACCESS_DENIED_TO_CATEGORY)?;
    app.set_board_visibility(team_id, user_id, category_id, board_id, visibility)
        .await?;
    Ok(empty_response())
}
