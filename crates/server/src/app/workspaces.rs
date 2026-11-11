//! Workspaces: which a user is a member of, and the way into one. A workspace is the
//! team of the routes that are not moved under a workspace yet.

use fvoci_model::{Board, OrgRole, UserWorkspace, WorkspaceRole};
use fvoci_store::{PublicBoards, Tenant};

use super::App;
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
}

impl App {
    /// Lets a member into a workspace; `None` unless the user is a member of it,
    /// whether it exists or not.
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
        // What the store answered is an identifier; one that is none is the server's
        // fault, not the request's.
        let tenant = Tenant::org(&access.org_id).map_err(|error| {
            AppError::from(anyhow::Error::from(error))
                .context("the organization of a workspace as the store has it")
        })?;
        Ok(Some(Entered {
            app: self.scoped(tenant),
            role: access.workspace_role,
            org_role: access.org_role,
        }))
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
        self.boards_for_user_and_team(user_id, workspace_id, public_boards)
            .await
    }
}
