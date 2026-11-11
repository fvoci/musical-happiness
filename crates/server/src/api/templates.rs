//! Listing the template boards of a team.
//!
//! Ports `server/api/templates.go`.

use axum::extract::{Path, State};
use fvoci_model::{Board, BoardType};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::contract::params::TeamId;
use super::contract::tag;

use super::{ApiResult, RequiredSession, json_response};
use super::{require_team_access, user_is_guest};
use crate::app::App;
use crate::error::AppError;
use crate::permissions::BoardPermission;

pub(super) fn routes() -> OpenApiRouter<App> {
    OpenApiRouter::new().routes(routes!(handle_get_templates))
}

/// The templates of a team
///
/// The team's template boards the user may see, then the built-in templates, which
/// every team is offered. Each of the two oldest first.
#[utoipa::path(
    get,
    path = "/teams/{teamId}/templates",
    operation_id = "getTemplates",
    tag = tag::BOARDS,
    params(TeamId),
    responses((status = 200, description = "The templates.", body = Vec<Board>)),
)]
async fn handle_get_templates(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    Path(team_id): Path<String>,
) -> ApiResult {
    let user_id = session.user_id.as_str();
    require_team_access(&app, user_id, &team_id).await?;
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
