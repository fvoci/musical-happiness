//! The workspaces of a user, and the wall of the routes that name a workspace.
//!
//! A route under `/w/{workspaceId}` takes [`Ws`]: the session's user as a member of the
//! workspace, with the application reaching the rows of the workspace's organization.
//! For anybody else the workspace is not there (404), whether it exists or not.

use axum::body::Bytes;
use axum::extract::rejection::RawPathParamsRejection;
use axum::extract::{FromRequestParts, Path, RawPathParams, RawQuery, State};
use axum::http::request::Parts;
use fvoci_model::{Board, OrgRole, Session, User, UserWorkspace, Workspace, WorkspaceRole};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::contract::params::{HomeMemberWorkspaceId, MemberWorkspaceId, UserSearch, WorkspaceId};
use super::contract::tag;
use super::session::session_of;
use super::{ApiResult, RequiredSession, decode_json, json_response, query_param};
use crate::app::App;
use crate::error::AppError;
use crate::permissions::Actor;

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
    /// What the user is in the workspace's organization, if a member of it.
    org_role: Option<OrgRole>,
    /// Whether the workspace is in the organization the server stands in, the one the
    /// routes that name no workspace work in.
    home: bool,
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
            org_role: entered.org_role,
            home: entered.home,
        })
    }

    /// What the user is in the workspace and its organization: the facts of what they
    /// may do there (`permissions`).
    pub(crate) fn actor(&self) -> Actor {
        Actor {
            org: self.org_role,
            workspace: Some(self.role),
        }
    }

    /// The workspace for one who works in it: a member who is no guest. A guest of the
    /// workspace is on the boards they were invited to and reaches those by their
    /// board; for the rest of the workspace (its sidebar, its templates, its users,
    /// its archives) a guest is told what anybody else is, that it is not there.
    ///
    /// The routes that ask this read the workspace, before they moved under `/w/`, in
    /// the organization the server stands in: a workspace of another organization was
    /// not there for them, also for its members. That answer is kept until the routes
    /// are decided anew.
    ///
    /// # Errors
    ///
    /// Returns 404 for a guest, and for a workspace of another organization.
    pub(crate) fn require_works_there(self) -> Result<Self, AppError> {
        if self.role == WorkspaceRole::Guest {
            return Err(workspace_not_found());
        }
        self.require_home()
    }

    /// The workspace for one who works in it, a member who is no guest, in whatever
    /// organization it is: what the routes take that were made under `/w/` and never
    /// read their workspace anywhere else. A guest is told what anybody else is.
    ///
    /// # Errors
    ///
    /// Returns 404 for a guest.
    pub(crate) fn require_no_guest(self) -> Result<Self, AppError> {
        if self.role == WorkspaceRole::Guest {
            return Err(workspace_not_found());
        }
        Ok(self)
    }

    /// The workspace for a member of it, a guest too, if it is of the organization the
    /// server stands in: the routes that read their workspace there before they moved
    /// under `/w/` keep that answer until the routes are decided anew.
    ///
    /// # Errors
    ///
    /// Returns 404 for a workspace of another organization.
    pub(crate) fn require_home(self) -> Result<Self, AppError> {
        if !self.home {
            return Err(workspace_not_found());
        }
        Ok(self)
    }
}

/// The workspace a request names in its path, for one who works in it: [`Ws`] for a
/// member who is no guest, of a workspace of the server's organization
/// ([`Ws::require_works_there`]). Anybody else is told 404. A parameter of the path that is no text is a malformed request (400),
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
/// boards; a guest of the workspace gets only the former, an administrator every board
/// of the workspace (what is on a private board they are no member of stays 404 to
/// them). Templates are not among them. Oldest first. 404 unless the user is a member of
/// the workspace.
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
/// Its name, and whether the admins of its boards may invite guests to them.
#[utoipa::path(
    get,
    path = "/w/{workspaceId}",
    operation_id = "getWorkspace",
    tag = tag::WORKSPACES,
    params(WorkspaceId),
    responses((status = 200, description = "The workspace.", body = Workspace)),
)]
async fn handle_get_workspace(WorksIn(ws): WorksIn) -> ApiResult {
    let workspace = ws.app.workspace(&ws.workspace_id).await?;
    Ok(json_response(&workspace))
}

/// The users of a workspace
///
/// Up to ten of the users the user sees in the workspace: whoever works there sees its
/// members and who is on a board of it with them, a guest only who is on a board of it
/// with them; and the user. With `search` those whose login name or display name has
/// that text in it, whatever the case of the letters, without it the ten oldest
/// accounts. Oldest account first.
#[utoipa::path(
    get,
    path = "/w/{workspaceId}/users",
    operation_id = "getWorkspaceUsers",
    tag = tag::WORKSPACES,
    params(HomeMemberWorkspaceId, UserSearch),
    responses((status = 200, description = "The users.", body = Vec<User>)),
)]
async fn handle_get_workspace_users(ws: Ws, RawQuery(query): RawQuery) -> ApiResult {
    let ws = ws.require_home()?;
    let search_query = query_param(query.as_deref(), "search");
    Ok(json_response(
        &ws.app
            .search_users_in_workspace(
                &ws.session.user_id,
                &ws.workspace_id,
                ws.role,
                &search_query,
            )
            .await?,
    ))
}

/// Users of a workspace by their identifiers
///
/// The users with the identifiers of the body, which must name at least one: an empty
/// list is 400. Every identifier must be of a user the user sees in the workspace and
/// none may be there twice; otherwise the request is 404. The user sees whom
/// `getWorkspaceUsers` finds, and who created or last changed a card, block or comment
/// of a board of the workspace they read, also after leaving the workspace. Oldest
/// account first, not in the order of the request.
#[utoipa::path(
    post,
    path = "/w/{workspaceId}/users",
    operation_id = "getWorkspaceUsersById",
    tag = tag::WORKSPACES,
    params(HomeMemberWorkspaceId),
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
        .require_home()?;
    if user_ids.is_empty() {
        return Err(AppError::bad_request("User IDs are empty"));
    }
    let users = ws
        .app
        .users_in_workspace(&ws.session.user_id, &ws.workspace_id, ws.role, &user_ids)
        .await?;
    Ok(json_response(&users))
}
