//! Team board roles. A team belongs to this workspace and never receives admin.

use axum::{body::Bytes, extract::Path};
use fvoci_model::{BoardTeam, BoardTeamRole};
use serde::Deserialize;
use utoipa_axum::{router::OpenApiRouter, routes};

use super::contract::{
    params::{BoardId, TeamId, WorkerWorkspaceId},
    tag,
};
use super::workspace::InContainer;
use super::{ApiResult, decode_object, json_response, no_content, require_board_permission};
use crate::{app::App, permissions::BoardPermission};

pub(super) fn routes() -> OpenApiRouter<App> {
    OpenApiRouter::new()
        .routes(routes!(list))
        .routes(routes!(assign, remove))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BoardPlace {
    board_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TeamPlace {
    board_id: String,
    team_id: String,
}

/// List the teams assigned to a board
///
/// For nonguest members allowed to list board members. Includes the team's current
/// people and its role. Ordered by team name in any letter case, by code point, then by
/// identifier.
/// Direct memberships remain separate.
#[utoipa::path(get, path = "/w/{workspaceId}/boards/{boardId}/teams", operation_id = "getBoardTeams", tag = tag::MEMBERS,
    params(WorkerWorkspaceId, BoardId), responses((status = 200, description = "Assigned teams.", body = Vec<BoardTeam>)))]
async fn list(InContainer(ws): InContainer, Path(place): Path<BoardPlace>) -> ApiResult {
    let ws = ws.require_no_guest()?;
    require_board_permission(
        &ws.app,
        &ws.session.user_id,
        &place.board_id,
        BoardPermission::ListMembers,
        "access denied to board members",
    )
    .await?;
    Ok(json_response(
        &ws.app
            .board_teams(&ws.workspace_id, &place.board_id)
            .await?,
    ))
}

/// Give or change a team's board role
///
/// For board administrators and workspace administrators. The role is viewer,
/// commenter, or editor; admin is rejected. The board and team must belong to this
/// workspace and are checked before decoding the body. Managed access is audited.
#[utoipa::path(put, path = "/w/{workspaceId}/boards/{boardId}/teams/{teamId}", operation_id = "setBoardTeam", tag = tag::MEMBERS,
    params(WorkerWorkspaceId, BoardId, TeamId), request_body = BoardTeamRole,
    responses((status = 204, description = "The team has this role.")))]
async fn assign(
    InContainer(ws): InContainer,
    Path(place): Path<TeamPlace>,
    body: Bytes,
) -> ApiResult {
    let ws = ws.require_no_guest()?;
    require_board_permission(
        &ws.app,
        &ws.session.user_id,
        &place.board_id,
        BoardPermission::ManageBoardRoles,
        "access denied to modify board members",
    )
    .await?;
    ws.app.team(&ws.workspace_id, &place.team_id).await?;
    let request: BoardTeamRole = decode_object(&body)?;
    ws.app
        .set_board_team(
            &ws.workspace_id,
            &place.board_id,
            &place.team_id,
            Some(request.role),
            &ws.session.user_id,
        )
        .await?;
    Ok(no_content())
}

/// Remove a team's board role
///
/// For board administrators and workspace administrators. Direct roles and the
/// roles inherited from other teams remain. Idempotent for an existing team.
#[utoipa::path(delete, path = "/w/{workspaceId}/boards/{boardId}/teams/{teamId}", operation_id = "removeBoardTeam", tag = tag::MEMBERS,
    params(WorkerWorkspaceId, BoardId, TeamId), responses((status = 204, description = "The team has no role on this board.")))]
async fn remove(InContainer(ws): InContainer, Path(place): Path<TeamPlace>) -> ApiResult {
    let ws = ws.require_no_guest()?;
    require_board_permission(
        &ws.app,
        &ws.session.user_id,
        &place.board_id,
        BoardPermission::ManageBoardRoles,
        "access denied to modify board members",
    )
    .await?;
    ws.app.team(&ws.workspace_id, &place.team_id).await?;
    ws.app
        .set_board_team(
            &ws.workspace_id,
            &place.board_id,
            &place.team_id,
            None,
            &ws.session.user_id,
        )
        .await?;
    Ok(no_content())
}
