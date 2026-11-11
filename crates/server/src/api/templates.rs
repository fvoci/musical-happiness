//! Listing the template boards of a workspace.
//!
//! Ports `server/api/templates.go`.

use fvoci_model::{Board, BoardType};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::contract::params::WorkspaceId;
use super::contract::tag;

use super::workspace::WorksIn;
use super::{ApiResult, json_response, user_is_guest};
use crate::app::App;
use crate::error::AppError;
use crate::permissions::BoardPermission;

pub(super) fn routes() -> OpenApiRouter<App> {
    OpenApiRouter::new().routes(routes!(handle_get_templates))
}

/// The templates of a workspace
///
/// The workspace's template boards the user may see, then the built-in templates, which
/// every workspace is offered. Each of the two oldest first.
#[utoipa::path(
    get,
    path = "/w/{workspaceId}/templates",
    operation_id = "getTemplates",
    tag = tag::BOARDS,
    params(WorkspaceId),
    responses((status = 200, description = "The templates.", body = Vec<Board>)),
)]
async fn handle_get_templates(WorksIn(ws): WorksIn) -> ApiResult {
    let (app, user_id, workspace_id) = (&ws.app, ws.session.user_id.as_str(), &ws.workspace_id);
    if user_is_guest(app, user_id).await? {
        return Err(AppError::forbidden("access denied to templates"));
    }

    let boards = app.template_boards(workspace_id, user_id).await?;
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
    tracing::debug!(workspace_id, boards_count = results.len(), "GetTemplates");
    Ok(json_response(&results))
}
