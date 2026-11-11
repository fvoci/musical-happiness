//! Teams and their users.

use axum::body::Bytes;
use axum::extract::{Path, RawQuery, State};
use fvoci_model::{Team, User};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::contract::params::{TeamId, UserSearch};
use super::contract::tag;

use super::{ApiResult, RequiredSession, decode_json, json_response, query_param};
use super::{require_team_access, user_is_guest};
use crate::app::App;
use crate::error::AppError;

pub(super) fn routes() -> OpenApiRouter<App> {
    OpenApiRouter::new()
        .routes(routes!(handle_get_teams))
        .routes(routes!(handle_get_team))
        .routes(routes!(handle_get_team_users, handle_get_team_users_by_id))
}

/// The refusal of the team routes.
const TEAM_DENIED: &str = "access denied to team";

/// The teams
///
/// The teams of the server, oldest first. A server has one, and nothing in the API
/// makes another. The signup token is there for administrators only.
#[utoipa::path(
    get,
    path = "/teams",
    operation_id = "getTeams",
    tag = tag::TEAMS,
    responses((status = 200, description = "The teams.", body = Vec<Team>)),
)]
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

/// A team
///
/// The signup token, which lets in whoever has it, is there for administrators only.
#[utoipa::path(
    get,
    path = "/teams/{teamId}",
    operation_id = "getTeam",
    tag = tag::TEAMS,
    params(TeamId),
    responses((status = 200, description = "The team.", body = Team)),
)]
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

/// The users of a team
///
/// Up to ten users: with `search` those whose name has that text in it, whatever the
/// case of the letters, without it the ten oldest accounts. Oldest account first.
#[utoipa::path(
    get,
    path = "/teams/{teamId}/users",
    operation_id = "getTeamUsers",
    tag = tag::TEAMS,
    params(TeamId, UserSearch),
    responses((status = 200, description = "The users.", body = Vec<User>)),
)]
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

/// Users of a team by their identifiers
///
/// The users with the identifiers of the body, which must name at least one: an empty
/// list is 400. Every identifier must be a user's and none may be there twice;
/// otherwise the request is 404. Oldest account first, not in the order of the request.
#[utoipa::path(
    post,
    path = "/teams/{teamId}/users",
    operation_id = "getTeamUsersById",
    tag = tag::TEAMS,
    params(TeamId),
    request_body = Vec<String>,
    responses((status = 200, description = "The users.", body = Vec<User>)),
)]
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
