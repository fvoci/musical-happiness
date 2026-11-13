//! The onboarding tour.
//!
//! Ports `server/api/onboarding.go`.

use serde::Serialize;
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::contract::params::WorkspaceId;
use super::contract::tag;

use super::workspace::TextWs;
use super::{ApiResult, json_response};
use crate::app::App;

/// What starting the tour answers: where the tour's board is.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct OnboardingStart {
    /// The identifier of the board the tour goes through.
    board_id: String,
    /// The identifier of the workspace.
    workspace_id: String,
}

pub(super) fn routes() -> OpenApiRouter<App> {
    OpenApiRouter::new().routes(routes!(handle_onboard))
}

/// Start the tour
///
/// Copies the built-in welcome board for the user, as a private board of their own,
/// and notes in the client's state (`getClientState`) that the tour has begun:
/// `onboardingTourStarted` is `1`, `onboardingTourStep` is `0` and `tourCategory` is
/// `onboarding`. Every request makes another copy.
#[utoipa::path(
    post,
    path = "/w/{workspaceId}/onboard",
    operation_id = "onboard",
    tag = tag::WORKSPACES,
    params(WorkspaceId),
    responses((status = 200, description = "Where the tour's board is.", body = OnboardingStart)),
)]
async fn handle_onboard(TextWs(ws): TextWs) -> ApiResult {
    let ws = ws.require_no_guest()?;
    let (app, user_id, workspace_id) = (&ws.app, ws.session.user_id.as_str(), &ws.workspace_id);
    let (workspace_id, board_id) = app.prepare_onboarding_tour(user_id, workspace_id).await?;
    Ok(json_response(&OnboardingStart {
        board_id,
        workspace_id,
    }))
}
