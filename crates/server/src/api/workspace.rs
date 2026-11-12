//! The workspaces of a user, and the wall of the routes that name a workspace.
//!
//! A route under `/w/{workspaceId}` takes [`Ws`]: the session's user as a member of the
//! workspace, with the application reaching the rows of the workspace's organization
//! and, of those, what is of this workspace (`App::in_workspace`): a board of another
//! workspace, and what is on it, is not there (404) as for a board that does not exist.
//! For anybody else the workspace is not there (404), whether it exists or not.

use axum::body::Bytes;
use axum::extract::rejection::RawPathParamsRejection;
use axum::extract::{FromRequestParts, Path, RawPathParams, RawQuery, State};
use axum::http::request::Parts;
use fvoci_model::{AuditRecord, Board, Session, User, UserWorkspace, Workspace, WorkspaceRole};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::contract::bodies::UserIds;
use super::contract::params::{
    CappedPerPage, HomeMemberWorkspaceId, MemberWorkspaceId, Page, UserId, UserSearch,
    WorkerWorkspaceId, WorkspaceId,
};
use super::contract::tag;
use super::org_admin::requested_page;
use super::session::session_of;
use super::{ApiResult, MAX_USER_IDS, RequiredSession, decode_json, json_response, query_param};
use crate::app::App;
use crate::error::AppError;
use crate::permissions;

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
        .routes(routes!(handle_get_workspace_user))
        .routes(routes!(handle_get_workspace_audit))
}

/// The workspace a request names in its path, for a member of it.
///
/// Extraction fails with 401 without a session, and with 404 unless the session's user
/// is a member of the workspace: one that is another's and one that does not exist
/// answer alike. A member whose session does not open the workspace's organization,
/// which takes only sessions made through its provider, is told so (401, reason
/// `sso_required`): that is asked where the workspace is entered
/// (`App::enter_workspace`), of every request, before anything else is asked of the
/// member. The application it hands the handler is behind the wall between
/// organizations, reaching the rows of the workspace's organization and no other, and
/// finds a board of the organization's other workspaces as one that is not there
/// (`App::board_access`); a handler that lists asks for what is in this one by
/// `workspace_id`.
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
            // The workspace that is no text is `no_text`; another parameter that is none
            // (a board's, a card's) is the malformed request it was before the route
            // was under a workspace.
            Err(RawPathParamsRejection::InvalidUtf8InPathParam(error)) => {
                return Err(
                    if error.to_string().ends_with(&format!("`{WORKSPACE_ID}`")) {
                        no_text()
                    } else {
                        malformed_path()
                    },
                );
            }
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
            .enter_workspace(&session, &workspace_id)
            .await?
            .ok_or_else(workspace_not_found)?;
        Ok(Self {
            app: entered.app.in_workspace(&workspace_id),
            session,
            workspace_id,
            role: entered.role,
            home: entered.home,
        })
    }

    /// The workspace for one who makes boards in it: whoever works in it, and is no
    /// guest of the instance either. A guest is refused (403) in `denial`: the workspace
    /// is there for them.
    ///
    /// # Errors
    ///
    /// Returns 403 for a guest of the workspace or of the instance, and the store's
    /// error if the user cannot be read.
    pub(crate) async fn require_board_maker(&self, denial: &str) -> Result<(), AppError> {
        if self.role == WorkspaceRole::Guest
            || self.app.user_is_guest(&self.session.user_id).await?
        {
            return Err(AppError::forbidden(denial));
        }
        Ok(())
    }

    /// Puts the path's workspace into a board a request body makes: a body that names
    /// another is refused (400, naming `field`), one that names none or this one takes
    /// it.
    ///
    /// # Errors
    ///
    /// Returns 400 for a board that names another workspace.
    pub(crate) fn place_board(&self, board: &mut Board, field: &str) -> Result<(), AppError> {
        if !board.workspace_id.is_empty() && board.workspace_id != self.workspace_id {
            return Err(
                AppError::bad_request("the board is made in the workspace of the path")
                    .with_field(field),
            );
        }
        board.workspace_id.clone_from(&self.workspace_id);
        Ok(())
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
/// workspace first. A workspace of an organization that takes only sessions made
/// through its provider is listed for any session, with `ssoRequired` true for one that
/// was not made so: what is under it answers 401 with the reason `sso_required` until
/// the user signs in through the provider of `orgId`.
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
    let workspaces = app.workspaces_of_user(&session).await?;
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
/// The users with the identifiers of the body that the user sees in the workspace, each
/// once; the body names at least one and at most 200: an empty list, or a longer one,
/// is 400. An identifier of
/// somebody the user does not see finds nothing, as one of nobody: the answer does not
/// tell them apart. The user sees whom `getWorkspaceUsers` finds, and who created or
/// last changed a card, block or comment of a board of the workspace they read, also
/// after leaving the workspace. Oldest account first, not in the order of the request.
#[utoipa::path(
    post,
    path = "/w/{workspaceId}/users",
    operation_id = "getWorkspaceUsersById",
    tag = tag::WORKSPACES,
    params(HomeMemberWorkspaceId),
    request_body = UserIds,
    responses((status = 200, description = "The users.", body = Vec<User>)),
)]
async fn handle_get_workspace_users_by_id(ws: Ws, body: Bytes) -> ApiResult {
    let ws = ws.require_home()?;
    let user_ids: Vec<String> = decode_json(&body)?;
    if user_ids.is_empty() {
        return Err(AppError::bad_request("User IDs are empty"));
    }
    if user_ids.len() > MAX_USER_IDS {
        return Err(AppError::bad_request(format_args!(
            "at most {MAX_USER_IDS} user IDs are asked for at once"
        )));
    }
    let users = ws
        .app
        .users_in_workspace(&ws.session.user_id, &ws.workspace_id, ws.role, &user_ids)
        .await?;
    Ok(json_response(&users))
}

/// A user of a workspace
///
/// A user the user sees in the workspace, as `getWorkspaceUsersById` finds them; anybody
/// else is 404, as one who does not exist.
#[utoipa::path(
    get,
    path = "/w/{workspaceId}/users/{userId}",
    operation_id = "getWorkspaceUser",
    tag = tag::WORKSPACES,
    params(HomeMemberWorkspaceId, UserId),
    responses((status = 200, description = "The user.", body = User)),
)]
async fn handle_get_workspace_user(
    ws: Ws,
    Path((_, user_id)): Path<(String, String)>,
) -> ApiResult {
    let ws = ws.require_home()?;
    let mut user = ws
        .app
        .users_in_workspace(
            &ws.session.user_id,
            &ws.workspace_id,
            ws.role,
            std::slice::from_ref(&user_id),
        )
        .await?
        .pop()
        .ok_or_else(|| AppError::not_found("user"))?;
    if user_id == ws.session.user_id {
        crate::app::sanitize(&mut user);
    } else {
        ws.app.sanitize_profile(&mut user);
    }
    Ok(json_response(&user))
}

/// The audit log of a workspace
///
/// One page of what was done in the workspace that its members are to know of, the
/// newest first: an administrator of the organization joining it as its administrator,
/// leaving it, or being taken out of it when they stopped administering the
/// organization; and an administrator of the workspace who could not read a board
/// giving somebody (themselves or another) a role on it. A board's title is there where
/// the reader sees the board. For whoever works in the workspace (a member who is no
/// guest), of any organization. 50 to a page unless `perPage` says otherwise, at most
/// 200. The records are never changed or removed but with the workspace, when it is
/// removed for good.
#[utoipa::path(
    get,
    path = "/w/{workspaceId}/audit",
    operation_id = "getWorkspaceAudit",
    tag = tag::WORKSPACES,
    params(WorkerWorkspaceId, Page, CappedPerPage),
    responses((status = 200, description = "The records of the page.", body = Vec<AuditRecord>)),
)]
async fn handle_get_workspace_audit(ws: Ws, RawQuery(query): RawQuery) -> ApiResult {
    if !permissions::reads_the_audit_log(Some(ws.role)) {
        return Err(workspace_not_found());
    }
    let paging = requested_page(query.as_deref())?;
    Ok(json_response(
        &ws.app
            .workspace_audit(&ws.session.user_id, &ws.workspace_id, paging)
            .await?,
    ))
}
