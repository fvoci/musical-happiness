//! Listing the template boards of a workspace.
//!
//! Ports `server/api/templates.go`.

use fvoci_model::Board;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::contract::params::WorkspaceId;
use super::contract::tag;

use super::workspace::TextWs;
use super::{ApiResult, json_response};
use crate::app::{App, as_managed};

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
async fn handle_get_templates(TextWs(ws): TextWs) -> ApiResult {
    let ws = ws.require_no_guest()?;
    let (app, user_id, workspace_id) = (&ws.app, ws.session.user_id.as_str(), &ws.workspace_id);
    let boards = app.template_boards(workspace_id, user_id, ws.role).await?;
    let mut results: Vec<Board> = Vec::with_capacity(boards.len());
    for board in boards {
        // Whole to who reads it; to the workspace's administrators who only manage it,
        // its name, as getBoard gives it (`as_managed`).
        let grant = app.board_grant(user_id, &board.id).await?;
        if grant.reads() {
            results.push(board);
        } else if grant.manages_only() {
            results.push(as_managed(board));
        }
    }
    tracing::debug!(workspace_id, boards_count = results.len(), "GetTemplates");
    Ok(json_response(&results))
}
