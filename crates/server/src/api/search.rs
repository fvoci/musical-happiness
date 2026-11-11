//! Searching boards.

use axum::extract::{Path, RawQuery, State};
use fvoci_model::{Board, BoardSearchField};
use fvoci_store::PublicBoards;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::contract::params::{SearchField, SearchTerm, WorkspaceId};
use super::contract::tag;

use super::user_is_guest;
use super::workspace::Ws;
use super::{ApiResult, RequiredSession, json_response, query_param};
use crate::app::App;
use crate::error::AppError;

pub(super) fn routes() -> OpenApiRouter<App> {
    OpenApiRouter::new()
        .routes(routes!(handle_search_boards))
        .routes(routes!(handle_search_all_boards))
}

/// Search the boards, asked through a workspace
///
/// The boards the user may find, as `getBoards` lists them, whose title matches the
/// term as `searchBoards` matches it, or with `field=property_name` those whose own
/// `properties` have a member of exactly that name, whatever its value (`null` too). The workspace of the path is not part of
/// the search: the boards of every workspace are searched. A `field` that is neither is 400.
/// Oldest first.
#[utoipa::path(
    get,
    path = "/w/{workspaceId}/boards/search",
    operation_id = "searchWorkspaceBoards",
    tag = tag::BOARDS,
    params(WorkspaceId, SearchTerm, SearchField),
    responses((status = 200, description = "The boards that match.", body = Vec<Board>)),
)]
async fn handle_search_boards(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    Path(workspace_id): Path<String>,
    RawQuery(query): RawQuery,
) -> ApiResult {
    let term = query_param(query.as_deref(), "q");
    let field_text = query_param(query.as_deref(), "field");
    let search_field = if field_text.is_empty() {
        BoardSearchField::Title
    } else {
        field_text
            .parse::<BoardSearchField>()
            .map_err(AppError::bad_request)?
    };
    // The query is read before the workspace is asked for, as it always was.
    let ws = Ws::enter(&app, session, workspace_id)
        .await?
        .works_there()?;
    let workspace_id = &ws.workspace_id;

    // Preserved quirk: the workspace of the path is not part of the search. The boards found
    // are those of every workspace (server/api/search.go:171).
    let boards = search_boards(&ws.app, &ws.session.user_id, &term, search_field).await?;
    tracing::debug!(workspace_id, boards_count = boards.len(), "SearchBoards");
    Ok(json_response(&boards))
}

/// Search the boards
///
/// The boards the user may find, as `getBoards` lists them, whose title matches the
/// term: a title matches if every word of the term, as spaces divide it, is somewhere
/// in it, whatever the case of the letters (of every letter, not only ASCII). Templates
/// are never found. `%` and `_` are characters like any other. Oldest first.
#[utoipa::path(
    get,
    path = "/boards/search",
    operation_id = "searchBoards",
    tag = tag::BOARDS,
    params(SearchTerm),
    responses((status = 200, description = "The boards that match.", body = Vec<Board>)),
)]
async fn handle_search_all_boards(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    RawQuery(query): RawQuery,
) -> ApiResult {
    let term = query_param(query.as_deref(), "q");
    let boards = search_boards(&app, &session.user_id, &term, BoardSearchField::Title).await?;
    tracing::debug!(boards_count = boards.len(), "SearchAllBoards");
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
