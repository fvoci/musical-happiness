//! The workspaces of a user, and the wall of the routes that name a workspace.
//!
//! A route under `/w/{workspaceId}` takes [`InWorkspace`]: the session's user as a member of the
//! workspace, with the application reaching the rows of the workspace's organization
//! and, of those, what is of this workspace (`App::in_workspace`): a board of another
//! workspace, and what is on it, is not there (404) as for a board that does not exist.
//! For anybody else the workspace is not there (404), whether it exists or not.

use axum::body::Bytes;
use axum::extract::rejection::RawPathParamsRejection;
use axum::extract::{FromRequestParts, Path, RawPathParams, RawQuery, State};
use axum::http::request::Parts;
use fvoci_model::{
    AuditRecord, Container, ContainerKinds, OrgRole, Session, User, UserWorkspace, Workspace,
    WorkspaceRole,
};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::contract::bodies::UserIds;
use super::contract::params::{
    CappedPerPage, MemberWorkspaceId, Page, UserId, UserSearch, WorkerWorkspaceId, WorkspaceId,
};
use super::contract::tag;
use super::org_admin::requested_page;
use super::session::session_of;
use super::{ApiResult, MAX_USER_IDS, RequiredSession, decode_json, json_response, query_param};
use crate::app::App;
use crate::error::AppError;
use crate::permissions::{self, Actor};

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

/// The workspace a request names in its path, for a member of it, reaching its boards
/// and no wiki: an operation on wikis takes [`InWiki`], one on both kinds [`InContainer`].
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
/// (`App::container_access`); a handler that lists asks for what is in this one by
/// `workspace_id`.
#[derive(Debug)]
pub(crate) struct InWorkspace {
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
}

impl FromRequestParts<App> for InWorkspace {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, app: &App) -> Result<Self, Self::Rejection> {
        // A path that is no text names no workspace the user could be in.
        Self::extract(parts, app, workspace_not_found).await
    }
}

impl InWorkspace {
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

impl InWorkspace {
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
            // What a route under a workspace is about is a board's, unless its extractor
            // says otherwise (`InWiki`, `InContainer`).
            app: entered
                .app
                .in_workspace(&workspace_id)
                .reaching(ContainerKinds::Boards),
            session,
            workspace_id,
            role: entered.role,
            org_role: entered.org_role,
        })
    }

    /// The same workspace, reaching boards of `kinds`.
    fn reaching(mut self, kinds: ContainerKinds) -> Self {
        self.app = self.app.reaching(kinds);
        self
    }

    /// What the user is in the workspace and its organization: the facts of what they
    /// may do there (`permissions`).
    pub(crate) fn actor(&self) -> Actor {
        Actor {
            org: self.org_role,
            workspace: Some(self.role),
        }
    }

    /// Refuses a guest of this workspace who asks to make a board.
    ///
    /// # Errors
    ///
    /// Returns 403 in `denial` for a guest; the workspace is there for them.
    pub(crate) fn require_container_maker(&self, denial: &str) -> Result<(), AppError> {
        if self.role == WorkspaceRole::Guest {
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
    pub(crate) fn place_container(
        &self,
        container: &mut Container,
        field: &str,
    ) -> Result<(), AppError> {
        if !container.workspace_id.is_empty() && container.workspace_id != self.workspace_id {
            return Err(
                AppError::bad_request("the board is made in the workspace of the path")
                    .with_field(field),
            );
        }
        container.workspace_id.clone_from(&self.workspace_id);
        Ok(())
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
}

/// A workspace whose path must be UTF-8 text, preserving the malformed-path response
/// of routes that previously extracted their path as text. Membership and guest
/// checks are the same as for [`InWorkspace`].
#[derive(Debug)]
pub(crate) struct TextInWorkspace(pub(crate) InWorkspace);

impl FromRequestParts<App> for TextInWorkspace {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, app: &App) -> Result<Self, Self::Rejection> {
        InWorkspace::extract(parts, app, malformed_path)
            .await
            .map(Self)
    }
}

/// A workspace as an operation on a wiki and its pages enters it: [`InWorkspace`] that reaches
/// wikis and no board. A board named where a wiki is meant is not there.
#[derive(Debug)]
pub(crate) struct InWiki(pub(crate) InWorkspace);

impl FromRequestParts<App> for InWiki {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, app: &App) -> Result<Self, Self::Rejection> {
        let ws = InWorkspace::from_request_parts(parts, app).await?;
        Ok(Self(ws.reaching(ContainerKinds::Wikis)))
    }
}

/// A workspace as an operation enters it that is the same for a board and a wiki (the
/// container itself, who is on it, its sharing, its files, its place in the sidebar and
/// in the trash): [`InWorkspace`] that reaches both kinds.
#[derive(Debug)]
pub(crate) struct InContainer(pub(crate) InWorkspace);

impl FromRequestParts<App> for InContainer {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, app: &App) -> Result<Self, Self::Rejection> {
        let ws = InWorkspace::from_request_parts(parts, app).await?;
        Ok(Self(ws.reaching(ContainerKinds::Every)))
    }
}

/// [`InContainer`] with the malformed-path answer of [`TextInWorkspace`].
#[derive(Debug)]
pub(crate) struct TextInContainer(pub(crate) InWorkspace);

impl FromRequestParts<App> for TextInContainer {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, app: &App) -> Result<Self, Self::Rejection> {
        let TextInWorkspace(ws) = TextInWorkspace::from_request_parts(parts, app).await?;
        Ok(Self(ws.reaching(ContainerKinds::Every)))
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
    responses((status = 200, description = "The boards.", body = Vec<Container>)),
)]
async fn handle_get_workspace_boards(ws: InWorkspace) -> ApiResult {
    let boards = ws
        .app
        .workspace_containers(&ws.session.user_id, &ws.workspace_id, ws.role)
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
async fn handle_get_workspace(TextInWorkspace(ws): TextInWorkspace) -> ApiResult {
    let ws = ws.require_no_guest()?;
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
    params(MemberWorkspaceId, UserSearch),
    responses((status = 200, description = "The users.", body = Vec<User>)),
)]
async fn handle_get_workspace_users(ws: InWorkspace, RawQuery(query): RawQuery) -> ApiResult {
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
/// after leaving the workspace. Oldest account first by `createAt`, then by identifier,
/// not in the order of the request.
///
/// An account that was deleted is answered to whoever asks by its identifier, seen
/// before or not, as what is left of it: the identifier, an empty `username`, `email`
/// and `displayName`, and `createAt`, `updateAt` and `deleteAt` all zero: no time of
/// the account is answered, neither when it went nor when it was asked to (the
/// identifier is kept as it was made). The empty `username` is what tells it, not a
/// time: a page shows a deleted user where it wrote or is mentioned.
/// `deleteAt` is not zero for a writer whose account waits to be deleted, who is
/// answered as they are, with their name.
#[utoipa::path(
    post,
    path = "/w/{workspaceId}/users",
    operation_id = "getWorkspaceUsersById",
    tag = tag::WORKSPACES,
    params(MemberWorkspaceId),
    request_body = UserIds,
    responses((status = 200, description = "The users.", body = Vec<User>)),
)]
async fn handle_get_workspace_users_by_id(ws: InWorkspace, body: Bytes) -> ApiResult {
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
    params(MemberWorkspaceId, UserId),
    responses((status = 200, description = "The user.", body = User)),
)]
async fn handle_get_workspace_user(
    ws: InWorkspace,
    Path((_, user_id)): Path<(String, String)>,
) -> ApiResult {
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
async fn handle_get_workspace_audit(ws: InWorkspace, RawQuery(query): RawQuery) -> ApiResult {
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
