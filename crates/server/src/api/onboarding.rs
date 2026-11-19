//! A new account's first board.

use axum::extract::RawQuery;
use serde::Serialize;
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::contract::params::{ScreenLanguage, WorkspaceId};
use super::contract::tag;

use super::workspace::TextInWorkspace;
use super::{ApiResult, json_response, query_option};
use crate::app::App;

/// What asking for a first board answers: where it is.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct WelcomeBoard {
    /// The identifier of the new board.
    board_id: String,
}

pub(super) fn routes() -> OpenApiRouter<App> {
    OpenApiRouter::new().routes(routes!(handle_onboard))
}

/// Make the welcome board
///
/// Copies the built-in welcome board for the user, as a private board of their own in
/// the workspace: what a new account starts on. It is written in the language of the
/// user's settings; where that is left to the device, or is one the board is not
/// written in, in `language`; failing both, in English. Every request makes another
/// copy.
#[utoipa::path(
    post,
    path = "/w/{workspaceId}/onboard",
    operation_id = "onboard",
    tag = tag::WORKSPACES,
    params(WorkspaceId, ScreenLanguage),
    responses((status = 200, description = "Where the new board is.", body = WelcomeBoard)),
)]
async fn handle_onboard(
    TextInWorkspace(ws): TextInWorkspace,
    RawQuery(query): RawQuery,
) -> ApiResult {
    let ws = ws.require_no_guest()?;
    let (app, user_id, workspace_id) = (&ws.app, ws.session.user_id.as_str(), &ws.workspace_id);
    let screen_language = query_option(query.as_deref(), "language");
    let board_id = app
        .create_welcome_board(user_id, workspace_id, screen_language.as_deref())
        .await?;
    Ok(json_response(&WelcomeBoard { board_id }))
}
