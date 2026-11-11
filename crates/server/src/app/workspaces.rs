//! Workspaces: which a user is a member of, and the way into one. A workspace is the
//! team of the routes that are not moved under a workspace yet.

use fvoci_model::{Board, OrgRole, Team, UserWorkspace, WorkspaceRole};
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
        Ok(Some(Entered {
            app: self.in_organization(&access.org_id)?,
            role: access.workspace_role,
            org_role: access.org_role,
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

    /// Whether the user works in the team: a member of the workspace who is no guest.
    /// This is what the routes that name a team, and are not moved under a workspace
    /// yet, ask before anything else; for anybody else the team is not there.
    ///
    /// A guest of a workspace is on the boards they were invited to and reaches those
    /// by their board; what a member of the team may do (see its boards, make one,
    /// join an open one) a guest may not, until the roles of a workspace say more
    /// (`WorkspaceAccess::works_there`).
    ///
    /// # Errors
    ///
    /// Returns the store's error if the membership cannot be read: that is no "no".
    pub async fn works_in_team(&self, user_id: &str, team_id: &str) -> Result<bool, AppError> {
        if user_id.is_empty() || team_id.is_empty() {
            return Ok(false);
        }
        let access = self.store().resolve_workspace(user_id, team_id).await?;
        Ok(access.is_some_and(|access| access.works_there()))
    }

    /// Whether the user is in the team at all, as a guest too: who may be put on a
    /// board of the team. Anybody else is, for the team, a user who does not exist.
    ///
    /// # Errors
    ///
    /// Returns the store's error if the membership cannot be read: that is no "no".
    pub async fn is_in_team(&self, user_id: &str, team_id: &str) -> Result<bool, AppError> {
        if user_id.is_empty() || team_id.is_empty() {
            return Ok(false);
        }
        let access = self.store().resolve_workspace(user_id, team_id).await?;
        Ok(access.is_some())
    }

    /// The teams of the organization that the user works in, oldest first.
    ///
    /// # Errors
    ///
    /// Returns the store's error if a query fails.
    pub async fn teams_of_user(&self, user_id: &str) -> Result<Vec<Team>, AppError> {
        // The user's workspaces are of every organization; the teams are those of the
        // organization the routes that name a team work in.
        let works_in: Vec<String> = self
            .workspaces_of_user(user_id)
            .await?
            .into_iter()
            .filter(|workspace| workspace.role != WorkspaceRole::Guest)
            .map(|workspace| workspace.id)
            .collect();
        let mut teams = self.teams().await?;
        teams.retain(|team| works_in.contains(&team.id));
        Ok(teams)
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
