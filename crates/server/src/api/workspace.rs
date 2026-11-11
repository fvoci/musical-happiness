//! The workspaces of a user, and the wall of the routes that name a workspace.
//!
//! A route under `/w/{workspaceId}` takes [`Ws`]: the session's user as a member of the
//! workspace, with the application reaching the rows of the workspace's organization.
//! For anybody else the workspace is not there (404), whether it exists or not.

use axum::extract::{FromRequestParts, RawPathParams, State};
use axum::http::request::Parts;
use fvoci_model::{Board, Session, UserWorkspace, WorkspaceRole};
use fvoci_store::{PublicBoards, Tenant, WorkspaceAccess};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::contract::params::WorkspaceId;
use super::contract::tag;
use super::session::session_of;
use super::{ApiResult, RequiredSession, json_response};
use crate::app::App;
use crate::error::AppError;

/// The path parameter that names the workspace of a route.
const WORKSPACE_ID: &str = "workspaceId";

pub(super) fn routes() -> OpenApiRouter<App> {
    OpenApiRouter::new()
        .routes(routes!(handle_get_my_workspaces))
        .routes(routes!(handle_get_workspace_boards))
}

/// The workspace a request names in its path, for a member of it.
///
/// Extraction fails with 401 without a session, and with 404 unless the session's user
/// is a member of the workspace: one that is another's and one that does not exist
/// answer alike. What it hands the handler is all the handler needs to stay behind the
/// wall: the application as the workspace's organization reaches it.
#[derive(Debug)]
pub(crate) struct Ws {
    /// The application, reaching the rows of the workspace's organization.
    pub(crate) app: App,
    /// The session of the request.
    pub(crate) session: Session,
    /// The workspace the path names.
    pub(crate) workspace_id: String,
    /// What the user is in the workspace and in its organization.
    pub(crate) access: WorkspaceAccess,
}

impl FromRequestParts<App> for Ws {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, app: &App) -> Result<Self, Self::Rejection> {
        let session = session_of(app, &parts.headers, &parts.extensions).await?;
        // A path that cannot be read names no workspace the user could be in.
        let params = RawPathParams::from_request_parts(parts, app)
            .await
            .map_err(|_| workspace_not_found())?;
        let workspace_id = params
            .iter()
            .find(|(name, _)| *name == WORKSPACE_ID)
            .map(|(_, value)| value.to_owned())
            .ok_or_else(|| AppError::internal("the route names no workspace"))?;
        let access = app
            .store()
            .resolve_workspace(&session.user_id, &workspace_id)
            .await?
            .ok_or_else(workspace_not_found)?;
        let app = app.scoped(Tenant::org(&access.org_id)?);
        Ok(Self {
            app,
            session,
            workspace_id,
            access,
        })
    }
}

fn workspace_not_found() -> AppError {
    AppError::not_found("workspace")
}

/// The workspaces of the user
///
/// The workspaces the user of the session is a member of, in every organization, with
/// what the user is in each. By organization, the oldest first, and in each the oldest
/// workspace first.
#[utoipa::path(
    get,
    path = "/users/me/workspaces",
    operation_id = "getMyWorkspaces",
    tag = tag::WORKSPACES,
    responses((status = 200, description = "The workspaces.", body = Vec<UserWorkspace>)),
)]
async fn handle_get_my_workspaces(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
) -> ApiResult {
    let workspaces = app.store().workspaces_of_user(&session.user_id).await?;
    Ok(json_response(&workspaces))
}

/// The boards of a workspace
///
/// The boards of the workspace that the user is a member of, and the workspace's open
/// boards; a guest of the workspace gets only the former. Templates are not among
/// them. Oldest first. 404 unless the user is a member of the workspace.
#[utoipa::path(
    get,
    path = "/w/{workspaceId}/boards",
    operation_id = "getWorkspaceBoards",
    tag = tag::WORKSPACES,
    params(WorkspaceId),
    responses((status = 200, description = "The boards.", body = Vec<Board>)),
)]
async fn handle_get_workspace_boards(ws: Ws) -> ApiResult {
    let public_boards = match ws.access.workspace_role {
        WorkspaceRole::Guest => PublicBoards::Exclude,
        WorkspaceRole::Admin | WorkspaceRole::Member => PublicBoards::Include,
    };
    let boards = ws
        .app
        .boards_for_user_and_team(&ws.session.user_id, &ws.workspace_id, public_boards)
        .await?;
    Ok(json_response(&boards))
}
