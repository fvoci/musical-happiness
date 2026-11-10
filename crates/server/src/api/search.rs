//! Searching boards.

use axum::Router;
use axum::extract::{Path, RawQuery, State};
use axum::routing::get;
use fvoci_model::{Board, BoardSearchField};
use fvoci_store::PublicBoards;

use super::{ApiResult, RequiredSession, json_response, query_param};
use super::{require_team_access, user_is_guest};
use crate::app::App;
use crate::error::AppError;

pub(super) fn routes() -> Router<App> {
    Router::new()
        .route("/teams/{team_id}/boards/search", get(handle_search_boards))
        .route("/boards/search", get(handle_search_all_boards))
}

async fn handle_search_boards(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    Path(team_id): Path<String>,
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
    require_team_access(&app, &session.user_id, &team_id, "access denied to team").await?;

    // Preserved quirk: the team of the path is not part of the search. The boards found
    // are those of every team (server/api/search.go:171).
    let boards = search_boards(&app, &session.user_id, &term, search_field).await?;
    tracing::debug!(team_id, boards_count = boards.len(), "SearchBoards");
    Ok(json_response(&boards))
}

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
