//! Listing the template boards of a team.
//!
//! Ports `server/api/templates.go`.

use axum::Router;
use axum::extract::{Path, State};
use axum::routing::get;
use fvoci_model::{Board, BoardType, GLOBAL_TEAM_ID};

use super::{ApiResult, RequiredSession, json_response};
use super::{require_team_access, user_is_guest};
use crate::app::App;
use crate::error::AppError;
use crate::permissions::BoardPermission;

pub(super) fn routes() -> Router<App> {
    Router::new().route("/teams/{team_id}/templates", get(handle_get_templates))
}

/// `GET /teams/{team_id}/templates`: the team's template boards the user may see.
async fn handle_get_templates(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    Path(team_id): Path<String>,
) -> ApiResult {
    let user_id = session.user_id.as_str();
    if team_id != GLOBAL_TEAM_ID {
        require_team_access(&app, user_id, &team_id, "access denied to team")?;
    }
    if user_is_guest(&app, user_id).await? {
        return Err(AppError::forbidden("access denied to templates"));
    }

    let boards = app.template_boards(&team_id, user_id).await?;
    let mut results: Vec<Board> = Vec::with_capacity(boards.len());
    for board in boards {
        let visible = board.board_type == BoardType::Open
            || app
                .has_permission_to_board(user_id, &board.id, BoardPermission::ViewBoard)
                .await?;
        if visible {
            results.push(board);
        }
    }
    tracing::debug!(team_id, boards_count = results.len(), "GetTemplates");
    Ok(json_response(&results))
}
