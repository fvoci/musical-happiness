//! The onboarding tour.
//!
//! Ports `server/api/onboarding.go`.

use axum::Router;
use axum::extract::{Path, State};
use axum::routing::post;
use serde_json::json;

use super::{ApiResult, RequiredSession, json_response};
use super::{require_team_access, user_is_guest};
use crate::app::App;
use crate::error::AppError;

pub(super) fn routes() -> Router<App> {
    Router::new().route("/teams/{team_id}/onboard", post(handle_onboard))
}

/// `POST /teams/{team_id}/onboard`: starts the onboarding tour and returns the team and
/// the new board.
async fn handle_onboard(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    Path(team_id): Path<String>,
) -> ApiResult {
    let user_id = session.user_id.as_str();
    require_team_access(&app, user_id, &team_id, "access denied to create board")?;
    if user_is_guest(&app, user_id).await? {
        return Err(AppError::forbidden("access denied to create board"));
    }

    let (team_id, board_id) = app.prepare_onboarding_tour(user_id, &team_id).await?;
    Ok(json_response(
        &json!({ "teamID": team_id, "boardID": board_id }),
    ))
}
