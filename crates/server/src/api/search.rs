//! Searching boards.

use axum::extract::RawQuery;
use fvoci_model::{Board, BoardSearchField};
use fvoci_store::PublicBoards;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::contract::params::{SearchField, SearchTerm, WorkspaceId};
use super::contract::tag;

use super::user_is_guest;
use super::workspace::Ws;
use super::{ApiResult, json_response, query_param};
use crate::app::App;
use crate::error::AppError;

pub(super) fn routes() -> OpenApiRouter<App> {
    OpenApiRouter::new().routes(routes!(handle_search_boards))
}

/// Search the boards of a workspace
///
/// The boards of the workspace the user finds (a member of them, or open where the user
/// works), whose title matches the term: a title matches if every word of the term, as
/// spaces divide it, is somewhere in it, whatever the case of the letters (of every
/// letter, not only ASCII); `%` and `_` are characters like any other. With
/// `field=property_name` those whose own `properties` have a member of exactly that
/// name, whatever its value (`null` too). A `field` that is neither is 400. Templates are
/// never found, and an empty term finds nothing. Oldest first.
#[utoipa::path(
    get,
    path = "/w/{workspaceId}/boards/search",
    operation_id = "searchWorkspaceBoards",
    tag = tag::BOARDS,
    params(WorkspaceId, SearchTerm, SearchField),
    responses((status = 200, description = "The boards that match.", body = Vec<Board>)),
)]
async fn handle_search_boards(ws: Ws, RawQuery(query): RawQuery) -> ApiResult {
    let ws = ws.require_no_guest()?;
    let term = query_param(query.as_deref(), "q");
    let field_text = query_param(query.as_deref(), "field");
    let search_field = if field_text.is_empty() {
        BoardSearchField::Title
    } else {
        field_text
            .parse::<BoardSearchField>()
            .map_err(AppError::bad_request)?
    };
    let workspace_id = &ws.workspace_id;

    let mut boards = search_boards(&ws.app, &ws.session.user_id, &term, search_field).await?;
    boards.retain(|board| board.workspace_id == *workspace_id);
    tracing::debug!(workspace_id, boards_count = boards.len(), "SearchBoards");
    Ok(json_response(&boards))
}

/// Searches the boards the user may find. An empty term finds nothing.
async fn search_boards(
    app: &App,
    user_id: &str,
    term: &str,
    search_field: BoardSearchField,
) -> Result<Vec<Board>, AppError> {
    if term.is_empty() {
        return Ok(Vec::new());
    }
    // A guest finds only the boards they are a member of.
    let is_guest = user_is_guest(app, user_id).await?;
    let public_boards = if is_guest {
        PublicBoards::Exclude
    } else {
        PublicBoards::Include
    };
    app.search_boards_for_user(term, search_field, user_id, public_boards)
        .await
}
