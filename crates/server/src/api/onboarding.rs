//! The onboarding tour.
//!
//! Ports `server/api/onboarding.go`.

use axum::extract::{Path, State};
use serde::Serialize;
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::contract::params::TeamId;
use super::contract::tag;

use super::{ApiResult, RequiredSession, json_response};
use super::{require_team_access, user_is_guest};
use crate::app::App;
use crate::error::AppError;

/// What starting the tour answers: where the tour's board is.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct OnboardingStart {
    /// The identifier of the board the tour goes through.
    board_id: String,
    /// The identifier of the team.
    team_id: String,
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
    path = "/teams/{teamId}/onboard",
    operation_id = "onboard",
    tag = tag::TEAMS,
    params(TeamId),
    responses((status = 200, description = "Where the tour's board is.", body = OnboardingStart)),
)]
async fn handle_onboard(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    Path(team_id): Path<String>,
) -> ApiResult {
    let user_id = session.user_id.as_str();
    require_team_access(&app, user_id, &team_id).await?;
    if user_is_guest(&app, user_id).await? {
        return Err(AppError::forbidden("access denied to create board"));
    }

    let (team_id, board_id) = app.prepare_onboarding_tour(user_id, &team_id).await?;
    Ok(json_response(&OnboardingStart { board_id, team_id }))
}
