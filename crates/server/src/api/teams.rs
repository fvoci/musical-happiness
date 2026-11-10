//! Teams and their users.

use axum::Router;
use axum::body::Bytes;
use axum::extract::{Path, RawQuery, State};
use axum::routing::get;
use fvoci_model::SINGLE_USER_ID;

use super::users::single_user;
use super::{ApiResult, RequiredSession, decode_json_or_internal, json_response, query_param};
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
    RequiredSession(_session): RequiredSession,
) -> ApiResult {
    Ok(json_response(&app.teams().await?))
}

async fn handle_get_team(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    Path(team_id): Path<String>,
) -> ApiResult {
    require_team_access(&app, &session.user_id, &team_id, TEAM_DENIED)?;
    // Preserved quirk: whatever team is asked for, the answer is the root team, signup
    // token included.
    Ok(json_response(&app.root_team().await?))
}

async fn handle_get_team_users(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    Path(team_id): Path<String>,
    RawQuery(query): RawQuery,
) -> ApiResult {
    let search_query = query_param(query.as_deref(), "search");
    require_team_access(&app, &session.user_id, &team_id, TEAM_DENIED)?;
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
    let user_ids: Vec<String> = decode_json_or_internal(&body)?;
    require_team_access(&app, &session.user_id, &team_id, TEAM_DENIED)?;
    let Some(first) = user_ids.first() else {
        return Err(AppError::bad_request("User IDs are empty"));
    };
    let users = if first == SINGLE_USER_ID {
        vec![single_user(&app).await]
    } else {
        app.users_list(&user_ids).await?
    };
    Ok(json_response(&users))
}
