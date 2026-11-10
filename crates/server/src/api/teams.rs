//! Teams and their users.

use axum::Router;
use axum::body::Bytes;
use axum::extract::{Path, RawQuery, State};
use axum::routing::get;

use super::{ApiResult, RequiredSession, decode_json, json_response, query_param};
use super::{require_team_access, user_is_guest};
use crate::app::App;
use crate::error::AppError;

pub(super) fn routes() -> Router<App> {
    Router::new()
        .route("/teams", get(handle_get_teams))
        .route("/teams/{team_id}", get(handle_get_team))
        .route(
            "/teams/{team_id}/users",
            get(handle_get_team_users).post(handle_get_team_users_by_id),
        )
}

/// The refusal of the team routes.
const TEAM_DENIED: &str = "access denied to team";

async fn handle_get_teams(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
) -> ApiResult {
    let mut teams = app.teams().await?;
    if !app.user(&session.user_id).await?.is_admin {
        for team in &mut teams {
            team.signup_token.clear();
        }
    }
    Ok(json_response(&teams))
}

async fn handle_get_team(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    Path(team_id): Path<String>,
) -> ApiResult {
    let mut team = app.team(&team_id).await?;
    require_team_access(&app, &session.user_id, &team_id, TEAM_DENIED).await?;
    // The signup token lets anyone in who has it: it is for the administrators, who
    // hand it on, and is left out for everybody else.
    if !app.user(&session.user_id).await?.is_admin {
        team.signup_token.clear();
    }
    Ok(json_response(&team))
}

async fn handle_get_team_users(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    Path(team_id): Path<String>,
    RawQuery(query): RawQuery,
) -> ApiResult {
    let search_query = query_param(query.as_deref(), "search");
    require_team_access(&app, &session.user_id, &team_id, TEAM_DENIED).await?;
    // There are no guests to treat differently, but the user must exist.
    user_is_guest(&app, &session.user_id).await?;
    Ok(json_response(&app.search_team_users(&search_query).await?))
}

async fn handle_get_team_users_by_id(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    Path(team_id): Path<String>,
    body: Bytes,
) -> ApiResult {
    let user_ids: Vec<String> = decode_json(&body)?;
    require_team_access(&app, &session.user_id, &team_id, TEAM_DENIED).await?;
    if user_ids.is_empty() {
        return Err(AppError::bad_request("User IDs are empty"));
    }
    Ok(json_response(&app.users_list(&user_ids).await?))
}
