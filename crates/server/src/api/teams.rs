//! Teams belong to a workspace. Nonguest members see them; its administrators manage them.

use super::contract::{
    params::{TeamId, UserId, WorkerWorkspaceId},
    tag,
};
use super::workspace::Ws;
use super::{ApiResult, decode_object, json_response, no_content};
use crate::{
    app::App,
    error::AppError,
    permissions::{self, WorkspacePermission},
};
use axum::{body::Bytes, extract::Path};
use fvoci_model::{Team, TeamName};
use serde::Deserialize;
use utoipa_axum::{router::OpenApiRouter, routes};

pub(super) fn routes() -> OpenApiRouter<App> {
    OpenApiRouter::new()
        .routes(routes!(list_teams, create_team))
        .routes(routes!(get_team, rename_team, delete_team))
        .routes(routes!(add_team_member, remove_team_member))
}

fn managing(ws: Ws) -> Result<Ws, AppError> {
    let ws = ws.require_no_guest()?;
    if !permissions::allows_in_workspace(ws.actor(), WorkspacePermission::ManageMembers) {
        return Err(AppError::forbidden(
            "only workspace administrators manage teams",
        ));
    }
    Ok(ws)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TeamPlace {
    team_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MemberPlace {
    team_id: String,
    user_id: String,
}

/// List the workspace's teams
///
/// Includes the login and display names of their nonguest members, ordered by user
/// identifier. Teams are ordered by name and identifier. Guests are answered 404.
#[utoipa::path(get, path = "/w/{workspaceId}/teams", operation_id = "getTeams", tag = tag::WORKSPACES,
    params(WorkerWorkspaceId), responses((status = 200, description = "Teams.", body = Vec<Team>)))]
async fn list_teams(ws: Ws) -> ApiResult {
    let ws = ws.require_no_guest()?;
    Ok(json_response(&ws.app.teams(&ws.workspace_id).await?))
}

/// Create an empty team
///
/// For workspace administrators. Surrounding whitespace is removed from the name;
/// an empty name is 400 at `/name`, a duplicate in this workspace is 409.
#[utoipa::path(post, path = "/w/{workspaceId}/teams", operation_id = "createTeam", tag = tag::WORKSPACES,
    params(WorkerWorkspaceId), request_body = TeamName,
    responses((status = 200, description = "The new team.", body = Team)))]
async fn create_team(ws: Ws, body: Bytes) -> ApiResult {
    let ws = managing(ws)?;
    let request: TeamName = decode_object(&body)?;
    Ok(json_response(
        &ws.app
            .create_team(&ws.workspace_id, &request.name, &ws.session.user_id)
            .await?,
    ))
}

/// Read a team in this workspace
///
/// A team outside the workspace or a guest is answered 404.
#[utoipa::path(get, path = "/w/{workspaceId}/teams/{teamId}", operation_id = "getTeam", tag = tag::WORKSPACES,
    params(WorkerWorkspaceId, TeamId), responses((status = 200, description = "The team.", body = Team)))]
async fn get_team(ws: Ws, Path(place): Path<TeamPlace>) -> ApiResult {
    let ws = ws.require_no_guest()?;
    Ok(json_response(
        &ws.app.team(&ws.workspace_id, &place.team_id).await?,
    ))
}

/// Rename a team
///
/// For workspace administrators. The same name rules as creation apply. The team is
/// checked before the request body, so a team in another workspace is always 404.
#[utoipa::path(put, path = "/w/{workspaceId}/teams/{teamId}", operation_id = "renameTeam", tag = tag::WORKSPACES,
    params(WorkerWorkspaceId, TeamId), request_body = TeamName,
    responses((status = 200, description = "The renamed team.", body = Team)))]
async fn rename_team(ws: Ws, Path(place): Path<TeamPlace>, body: Bytes) -> ApiResult {
    let ws = managing(ws)?;
    ws.app.team(&ws.workspace_id, &place.team_id).await?;
    let request: TeamName = decode_object(&body)?;
    Ok(json_response(
        &ws.app
            .rename_team(
                &ws.workspace_id,
                &place.team_id,
                &request.name,
                &ws.session.user_id,
            )
            .await?,
    ))
}

/// Delete a team and its board roles
///
/// For workspace administrators. This does not delete the people's accounts or
/// change their membership of the workspace or their direct roles on boards.
#[utoipa::path(delete, path = "/w/{workspaceId}/teams/{teamId}", operation_id = "deleteTeam", tag = tag::WORKSPACES,
    params(WorkerWorkspaceId, TeamId), responses((status = 204, description = "The team is deleted.")))]
async fn delete_team(ws: Ws, Path(place): Path<TeamPlace>) -> ApiResult {
    let ws = managing(ws)?;
    ws.app.team(&ws.workspace_id, &place.team_id).await?;
    ws.app
        .delete_team(&ws.workspace_id, &place.team_id, &ws.session.user_id)
        .await?;
    Ok(no_content())
}

/// Add a nonguest workspace member to a team
///
/// For workspace administrators. Idempotent. A guest or somebody outside the
/// workspace is 404. Removing a workspace member or making them a guest removes
/// them from every team of that workspace.
#[utoipa::path(put, path = "/w/{workspaceId}/teams/{teamId}/members/{userId}", operation_id = "addTeamMember", tag = tag::WORKSPACES,
    params(WorkerWorkspaceId, TeamId, UserId), responses((status = 204, description = "The person is in the team.")))]
async fn add_team_member(ws: Ws, Path(place): Path<MemberPlace>) -> ApiResult {
    let ws = managing(ws)?;
    ws.app.team(&ws.workspace_id, &place.team_id).await?;
    ws.app
        .add_team_member(
            &ws.workspace_id,
            &place.team_id,
            &place.user_id,
            &ws.session.user_id,
        )
        .await?;
    Ok(no_content())
}

/// Remove a person from a team
///
/// For workspace administrators. Idempotent within a team that exists.
#[utoipa::path(delete, path = "/w/{workspaceId}/teams/{teamId}/members/{userId}", operation_id = "removeTeamMember", tag = tag::WORKSPACES,
    params(WorkerWorkspaceId, TeamId, UserId), responses((status = 204, description = "The person is out of the team.")))]
async fn remove_team_member(ws: Ws, Path(place): Path<MemberPlace>) -> ApiResult {
    let ws = managing(ws)?;
    ws.app.team(&ws.workspace_id, &place.team_id).await?;
    ws.app
        .remove_team_member(
            &ws.workspace_id,
            &place.team_id,
            &place.user_id,
            &ws.session.user_id,
        )
        .await?;
    Ok(no_content())
}
