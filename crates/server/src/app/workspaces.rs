//! Workspaces: which a user is a member of, the way into one, and the workspaces of the
//! organization the routes that name no workspace in their path work in. And the way
//! into an organization: for its administrators, and for a sign-in through its
//! provider.

use fvoci_model::{Board, OrgRole, UserWorkspace, Workspace, WorkspaceRole};
use fvoci_store::{PublicBoards, Tenant};

use super::App;
use crate::auth::secret::new_secret;
use crate::error::AppError;

/// A workspace as a member of it enters it: the application behind the wall between
/// organizations, and what the user is in the workspace and in its organization.
#[derive(Debug, Clone)]
pub struct Entered {
    /// The application, reaching the rows of the workspace's organization and no
    /// other. It is not narrowed to the workspace: the organization's other
    /// workspaces are in its reach.
    pub app: App,
    /// What the user is in the workspace.
    pub role: WorkspaceRole,
    /// What the user is in the workspace's organization; `None` for somebody who is in
    /// the workspace without belonging to the organization.
    pub org_role: Option<OrgRole>,
    /// Whether the workspace is in the organization this application stood in, the
    /// one of the routes that name no workspace.
    pub home: bool,
}

impl App {
    /// The same server reaching the rows of the organization `org_id`, for an
    /// administrator of it; `None` for anybody else, and for an organization that is not
    /// there, not in use or the system's.
    ///
    /// # Errors
    ///
    /// Returns the store's error if the membership cannot be read.
    pub async fn enter_org_as_admin(
        &self,
        user_id: &str,
        org_id: &str,
    ) -> Result<Option<Self>, AppError> {
        let Ok(tenant) = Tenant::org(org_id) else {
            return Ok(None);
        };
        let role = self.store().org_role_of_user(user_id, org_id).await?;
        Ok((role == Some(OrgRole::Admin)).then(|| self.scoped(tenant)))
    }

    /// The same server reaching the rows of the organization `org_id`, for a sign-in
    /// through its provider: the one a sign-in is started for, or the one a sign-in that
    /// comes back was started for. `None` for text that is no identifier.
    pub(crate) fn enter_org_for_sign_in(&self, org_id: &str) -> Option<Self> {
        Tenant::org(org_id).ok().map(|tenant| self.scoped(tenant))
    }
}

impl App {
    /// Lets a member into a workspace; `None` unless the user is a member of it,
    /// whether it exists or not. [`Entered::home`] is told against the organization
    /// this application stands in: the server's, for the one the routes are given.
    ///
    /// # Errors
    ///
    /// Returns the store's error if the membership cannot be read: that is no "not a
    /// member".
    pub async fn enter_workspace(
        &self,
        user_id: &str,
        workspace_id: &str,
    ) -> Result<Option<Entered>, AppError> {
        let Some(access) = self
            .store()
            .resolve_workspace(user_id, workspace_id)
            .await?
        else {
            return Ok(None);
        };
        let app = self.in_organization(&access.org_id)?;
        let home = app.store().tenant() == self.store().tenant();
        Ok(Some(Entered {
            app,
            role: access.workspace_role,
            org_role: access.org_role,
            home,
        }))
    }

    /// The application as the holder of a board's read token reaches the board: behind
    /// the wall of the board's own organization, whichever the request came in for.
    /// `None` if the token opens nothing, or is empty.
    ///
    /// # Errors
    ///
    /// Returns the store's error if the board's sharing cannot be read: that is no
    /// "no", and answered as one it would tell a reader with a good link that the link
    /// is dead.
    pub async fn enter_shared_board(
        &self,
        board_id: &str,
        read_token: &str,
    ) -> Result<Option<Self>, AppError> {
        if read_token.is_empty() {
            return Ok(None);
        }
        let org_id = self
            .authenticator()
            .org_of_read_token(board_id, read_token)
            .await?;
        org_id
            .map(|org_id| self.in_organization(&org_id))
            .transpose()
    }

    /// The application behind the wall of the organization the store named.
    fn in_organization(&self, org_id: &str) -> Result<Self, AppError> {
        // What the store answered is an identifier; one that is none is the server's
        // fault, not the request's.
        let tenant = Tenant::org(org_id).map_err(|error| {
            AppError::from(anyhow::Error::from(error))
                .context("the organization of a row as the store has it")
        })?;
        Ok(self.scoped(tenant))
    }

    /// Whether the user works in the workspace: a member of the workspace who is no guest.
    /// This is what the routes that name a workspace in their body or query, and not under
    /// `/w/{workspaceId}`, ask before anything else; for anybody else the workspace is not
    /// there. The routes under a workspace ask it of their path (`WorksIn`).
    ///
    /// A guest of a workspace is on the boards they were invited to and reaches those
    /// by their board; what a member of the workspace may do (see its boards, make one,
    /// join an open one) a guest may not, until the roles of a workspace say more
    /// (`WorkspaceAccess::works_there`).
    ///
    /// # Errors
    ///
    /// Returns the store's error if the membership cannot be read: that is no "no".
    pub async fn works_in_workspace(
        &self,
        user_id: &str,
        workspace_id: &str,
    ) -> Result<bool, AppError> {
        if user_id.is_empty() || workspace_id.is_empty() {
            return Ok(false);
        }
        let access = self
            .store()
            .resolve_workspace(user_id, workspace_id)
            .await?;
        Ok(access.is_some_and(|access| access.works_there()))
    }

    /// Whether the user is in the workspace at all, as a guest too: who may be put on a
    /// board of the workspace. Anybody else is, for the workspace, a user who does not exist.
    ///
    /// # Errors
    ///
    /// Returns the store's error if the membership cannot be read: that is no "no".
    pub async fn is_in_workspace(
        &self,
        user_id: &str,
        workspace_id: &str,
    ) -> Result<bool, AppError> {
        if user_id.is_empty() || workspace_id.is_empty() {
            return Ok(false);
        }
        let access = self
            .store()
            .resolve_workspace(user_id, workspace_id)
            .await?;
        Ok(access.is_some())
    }

    /// The workspaces the user is a member of, in every organization: by organization,
    /// the oldest first, and in each the oldest workspace first.
    ///
    /// # Errors
    ///
    /// Returns the store's error if the query fails.
    pub async fn workspaces_of_user(&self, user_id: &str) -> Result<Vec<UserWorkspace>, AppError> {
        Ok(self.store().workspaces_of_user(user_id).await?)
    }

    /// The boards of a workspace that the user is a member of and, unless the user is
    /// a guest of the workspace, its open boards. Templates are left out. Oldest first.
    ///
    /// The handle is the one a member entered the workspace with
    /// ([`App::enter_workspace`]).
    ///
    /// # Errors
    ///
    /// Returns the store's error if the query fails.
    pub async fn workspace_boards(
        &self,
        user_id: &str,
        workspace_id: &str,
        role: WorkspaceRole,
    ) -> Result<Vec<Board>, AppError> {
        let public_boards = match role {
            WorkspaceRole::Guest => PublicBoards::Exclude,
            WorkspaceRole::Admin | WorkspaceRole::Member => PublicBoards::Include,
        };
        self.boards_for_user_and_workspace(user_id, workspace_id, public_boards)
            .await
    }

    /// Returns the root workspace: the oldest workspace of the organization, made with
    /// the organization.
    ///
    /// # Errors
    ///
    /// Returns the store's error if the workspaces cannot be read, and a server error if the
    /// organization has none.
    pub async fn root_workspace(&self) -> Result<Workspace, AppError> {
        self.store()
            .get_workspaces()
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| AppError::internal("the organization has no workspace"))
    }

    /// Returns the workspace with the given identifier.
    ///
    /// # Errors
    ///
    /// Returns a 404 if there is no such workspace, and the store's error if the query fails.
    pub async fn workspace(&self, workspace_id: &str) -> Result<Workspace, AppError> {
        match self.store().get_workspace(workspace_id).await {
            Ok(workspace) => Ok(workspace),
            Err(error) if error.is_not_found() => Err(AppError::not_found("workspace")),
            Err(error) => Err(error.into()),
        }
    }

    /// Returns every workspace, oldest first.
    ///
    /// # Errors
    ///
    /// Returns the store's error if the query fails.
    pub async fn workspaces(&self) -> Result<Vec<Workspace>, AppError> {
        Ok(self.store().get_workspaces().await?)
    }

    /// Replaces a workspace's signup token with a fresh one, on behalf of a user, and
    /// returns the workspace with it. The old token lets nobody in any more.
    ///
    /// # Errors
    ///
    /// Returns a 404 if there is no such workspace, and the store's error if the write fails.
    pub async fn regenerate_signup_token(
        &self,
        workspace_id: &str,
        user_id: &str,
    ) -> Result<Workspace, AppError> {
        match self
            .store()
            .set_workspace_signup_token(workspace_id, &new_secret(), user_id)
            .await
        {
            Ok(()) => self.workspace(workspace_id).await,
            Err(error) if error.is_not_found() => Err(AppError::not_found("workspace")),
            Err(error) => Err(error.into()),
        }
    }
}
