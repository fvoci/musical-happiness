//! The workspaces of a user, and the wall of the routes that name a workspace.
//!
//! A route under `/w/{workspaceId}` takes [`Ws`]: the session's user as a member of the
//! workspace, with the application reaching the rows of the workspace's organization.
//! For anybody else the workspace is not there (404), whether it exists or not.

use axum::body::Bytes;
use axum::extract::rejection::RawPathParamsRejection;
use axum::extract::{FromRequestParts, Path, RawPathParams, RawQuery, State};
use axum::http::request::Parts;
use fvoci_model::{Board, Session, User, UserWorkspace, Workspace, WorkspaceRole};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::contract::params::{MemberWorkspaceId, UserSearch, WorkspaceId};
use super::contract::tag;
use super::session::session_of;
use super::{ApiResult, RequiredSession, decode_json, json_response, query_param, user_is_guest};
use crate::app::App;
use crate::error::AppError;

/// The path parameter that names the workspace of a route.
const WORKSPACE_ID: &str = "workspaceId";

pub(super) fn routes() -> OpenApiRouter<App> {
    OpenApiRouter::new()
        .routes(routes!(handle_get_my_workspaces))
        .routes(routes!(handle_get_workspace_boards))
        .routes(routes!(handle_get_workspace))
        .routes(routes!(
            handle_get_workspace_users,
            handle_get_workspace_users_by_id
        ))
}

/// The workspace a request names in its path, for a member of it.
///
/// Extraction fails with 401 without a session, and with 404 unless the session's user
/// is a member of the workspace: one that is another's and one that does not exist
/// answer alike. The application it hands the handler is behind the wall between
/// organizations, reaching the rows of the workspace's organization and no other. It
/// is not narrowed to the workspace: the organization's other workspaces are in its
/// reach, and a handler asks for what is in this one by `workspace_id`.
#[derive(Debug)]
pub(crate) struct Ws {
    /// The application, reaching the rows of the workspace's organization.
    pub(crate) app: App,
    /// The session of the request.
    pub(crate) session: Session,
    /// The workspace the path names.
    pub(crate) workspace_id: String,
    /// What the user is in the workspace.
    pub(crate) role: WorkspaceRole,
}

impl FromRequestParts<App> for Ws {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, app: &App) -> Result<Self, Self::Rejection> {
        // A path that is no text names no workspace the user could be in.
        Self::extract(parts, app, workspace_not_found).await
    }
}

impl Ws {
    /// The workspace of the path for the session's user; `no_text` is the refusal of a
    /// parameter of the path that is no text.
    async fn extract(
        parts: &mut Parts,
        app: &App,
        no_text: fn() -> AppError,
    ) -> Result<Self, AppError> {
        let session = session_of(app, &parts.headers, &parts.extensions).await?;
        let params = match RawPathParams::from_request_parts(parts, app).await {
            Ok(params) => params,
            Err(RawPathParamsRejection::InvalidUtf8InPathParam(_)) => return Err(no_text()),
            Err(_) => return Err(AppError::internal("the route has no path parameters")),
        };
        let workspace_id = params
            .iter()
            .find(|(name, _)| *name == WORKSPACE_ID)
            .map(|(_, value)| value.to_owned())
            .ok_or_else(|| AppError::internal("the route names no workspace"))?;
        Self::enter(app, session, workspace_id).await
    }
}

impl Ws {
    /// The workspace for the session's user, as the extractor finds it: for a route
    /// that looks at something of the request (its body, its query) before it asks
    /// whether the workspace is there.
    ///
    /// # Errors
    ///
    /// Returns 404 unless the user is a member of the workspace, and the store's error
    /// if the membership cannot be read.
    pub(crate) async fn enter(
        app: &App,
        session: Session,
        workspace_id: String,
    ) -> Result<Self, AppError> {
        let entered = app
            .enter_workspace(&session.user_id, &workspace_id)
            .await?
            .ok_or_else(workspace_not_found)?;
        Ok(Self {
            app: entered.app,
            session,
            workspace_id,
            role: entered.role,
        })
    }

    /// The workspace for one who works in it: a member who is no guest. A guest of the
    /// workspace is on the boards they were invited to and reaches those by their
    /// board; for the rest of the workspace (its sidebar, its templates, its users,
    /// its archives) a guest is told what anybody else is, that it is not there.
    ///
    /// # Errors
    ///
    /// Returns 404 for a guest.
    pub(crate) fn require_works_there(self) -> Result<Self, AppError> {
        if self.role == WorkspaceRole::Guest {
            return Err(workspace_not_found());
        }
        Ok(self)
    }
}

/// The workspace a request names in its path, for one who works in it: [`Ws`] for a
/// member who is no guest ([`Ws::require_works_there`]). A guest is told what anybody
/// else is: 404. A parameter of the path that is no text is a malformed request (400),
/// as it is on the routes that take their parameters as text.
#[derive(Debug)]
pub(crate) struct WorksIn(pub(crate) Ws);

impl FromRequestParts<App> for WorksIn {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, app: &App) -> Result<Self, Self::Rejection> {
        Ws::extract(parts, app, malformed_path)
            .await?
            .require_works_there()
            .map(Self)
    }
}

fn workspace_not_found() -> AppError {
    AppError::not_found("workspace")
}

/// What the framework answers for a parameter of the path that is no text.
fn malformed_path() -> AppError {
    AppError::bad_request("the request is malformed")
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
    let workspaces = app.workspaces_of_user(&session.user_id).await?;
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
    params(MemberWorkspaceId),
    responses((status = 200, description = "The boards.", body = Vec<Board>)),
)]
async fn handle_get_workspace_boards(ws: Ws) -> ApiResult {
    let boards = ws
        .app
        .workspace_boards(&ws.session.user_id, &ws.workspace_id, ws.role)
        .await?;
    Ok(json_response(&boards))
}

/// A workspace
///
/// The signup token, which lets in whoever has it, is there for administrators only.
#[utoipa::path(
    get,
    path = "/w/{workspaceId}",
    operation_id = "getWorkspace",
    tag = tag::WORKSPACES,
    params(WorkspaceId),
    responses((status = 200, description = "The workspace.", body = Workspace)),
)]
async fn handle_get_workspace(WorksIn(ws): WorksIn) -> ApiResult {
    let mut workspace = ws.app.workspace(&ws.workspace_id).await?;
    // The signup token lets anyone in who has it: it is for the administrators, who
    // hand it on, and is left out for everybody else.
    if !ws.app.user(&ws.session.user_id).await?.is_admin {
        workspace.signup_token.clear();
    }
    Ok(json_response(&workspace))
}

/// The users of a workspace
///
/// Up to ten users: with `search` those whose login name or display name has that text
/// in it, whatever the case of the letters, without it the ten oldest accounts. Oldest
/// account first.
#[utoipa::path(
    get,
    path = "/w/{workspaceId}/users",
    operation_id = "getWorkspaceUsers",
    tag = tag::WORKSPACES,
    params(WorkspaceId, UserSearch),
    responses((status = 200, description = "The users.", body = Vec<User>)),
)]
async fn handle_get_workspace_users(WorksIn(ws): WorksIn, RawQuery(query): RawQuery) -> ApiResult {
    let search_query = query_param(query.as_deref(), "search");
    // There are no guests to treat differently, but the user must exist.
    user_is_guest(&ws.app, &ws.session.user_id).await?;
    Ok(json_response(
        &ws.app.search_workspace_users(&search_query).await?,
    ))
}

/// Users of a workspace by their identifiers
///
/// The users with the identifiers of the body, which must name at least one: an empty
/// list is 400. Every identifier must be a user's and none may be there twice;
/// otherwise the request is 404. Oldest account first, not in the order of the request.
#[utoipa::path(
    post,
    path = "/w/{workspaceId}/users",
    operation_id = "getWorkspaceUsersById",
    tag = tag::WORKSPACES,
    params(WorkspaceId),
    request_body = Vec<String>,
    responses((status = 200, description = "The users.", body = Vec<User>)),
)]
async fn handle_get_workspace_users_by_id(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    Path(workspace_id): Path<String>,
    body: Bytes,
) -> ApiResult {
    // The body is read before the workspace is asked for, as it always was.
    let user_ids: Vec<String> = decode_json(&body)?;
    let ws = Ws::enter(&app, session, workspace_id)
        .await?
        .require_works_there()?;
    if user_ids.is_empty() {
        return Err(AppError::bad_request("User IDs are empty"));
    }
    Ok(json_response(&ws.app.users_list(&user_ids).await?))
}
