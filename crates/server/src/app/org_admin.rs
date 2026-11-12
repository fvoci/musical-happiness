//! What an organization's administrators do to it: its name and public links, the
//! roles of its members, its workspaces and their trash, and stepping into a workspace
//! as its administrator. And the audit logs that record it, for whom they are kept.
//!
//! The application here is the one the route of the organization entered
//! (`App::enter_org_as_admin`), reaching the organization's rows.

use std::collections::HashMap;

use fvoci_model::{
    AuditAction, AuditRecord, MAX_NAME_CHARS, OrgMember, OrgRole, OrgWorkspace, Organization,
    OrganizationPatch, Workspace, is_id, new_id,
};
use fvoci_store::{
    AuditPlace, Joining, OrgRoleChange, Paging, StoreError, Taken, WorkspaceTrashing,
};

use super::App;
use crate::auth::secret::new_secret;
use crate::error::{AppError, Reason};

/// How many records or members one page holds when the request does not say.
pub const DEFAULT_PER_PAGE: u32 = 50;

/// The most records or members one page holds; a request for more gets this many.
pub const MAX_PER_PAGE: u32 = 200;

/// One page of a list as a request asks for it, by its `page` and `perPage`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageAsked {
    /// The page, counted from 0.
    pub page: i64,
    /// How many a page has; zero or less is [`DEFAULT_PER_PAGE`], more than
    /// [`MAX_PER_PAGE`] is that many.
    pub per_page: i64,
}

impl PageAsked {
    /// The page as the store reads it; `None` for a page before the first.
    fn paging(self) -> Option<Paging> {
        let page = u32::try_from(self.page).ok()?;
        let per_page = if self.per_page <= 0 {
            DEFAULT_PER_PAGE
        } else {
            u32::try_from(self.per_page)
                .unwrap_or(MAX_PER_PAGE)
                .min(MAX_PER_PAGE)
        };
        Some(Paging { page, per_page })
    }
}

/// The name of a workspace as it is stored: without the spaces around it, 1 to
/// [`MAX_NAME_CHARS`] characters.
fn workspace_name(name: &str) -> Result<&str, AppError> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > MAX_NAME_CHARS {
        return Err(AppError::bad_request(format!(
            "a workspace's name has 1 to {MAX_NAME_CHARS} characters"
        ))
        .with_field("name"));
    }
    Ok(name)
}

fn name_taken() -> AppError {
    AppError::conflict("another workspace of the organization has the name")
        .with_reason(Reason::WorkspaceNameTaken)
        .with_field("name")
}

fn workspace_not_found() -> AppError {
    AppError::not_found("workspace")
}

/// A workspace named by text that is no identifier is not there, as one nobody has.
fn known_workspace(workspace_id: &str) -> Result<(), AppError> {
    if is_id(workspace_id) {
        Ok(())
    } else {
        Err(workspace_not_found())
    }
}

impl App {
    /// The organization of the application.
    ///
    /// # Errors
    ///
    /// Returns a 404 if it is not there, and the store's error if it cannot be read.
    pub async fn organization(&self) -> Result<Organization, AppError> {
        match self.store().organization().await {
            Ok(organization) => Ok(organization),
            Err(error) if error.is_not_found() => Err(AppError::not_found("organization")),
            Err(error) => Err(error.into()),
        }
    }

    /// Changes the organization's name or whether its boards may be read by a public
    /// link, on behalf of `actor_id`. Turned off, the links of its boards close at once,
    /// for whoever follows a board through one too.
    ///
    /// # Errors
    ///
    /// Returns a 400 for a name over [`MAX_NAME_CHARS`] characters, and the store's
    /// error.
    pub async fn update_organization(
        &self,
        patch: &OrganizationPatch,
        actor_id: &str,
    ) -> Result<Organization, AppError> {
        let mut patch = patch.clone();
        if let Some(name) = &mut patch.name {
            let trimmed = name.trim();
            if trimmed.chars().count() > MAX_NAME_CHARS {
                return Err(AppError::bad_request(format!(
                    "an organization's name has at most {MAX_NAME_CHARS} characters"
                ))
                .with_field("name"));
            }
            *name = trimmed.to_owned();
        }
        let organization = match self.store().update_organization(&patch, actor_id).await {
            Ok(organization) => organization,
            Err(error) if error.is_not_found() => {
                return Err(AppError::not_found("organization"));
            }
            Err(error) => return Err(error.into()),
        };
        if !organization.public_links {
            self.close_shared_readers(None).await?;
        }
        Ok(organization)
    }

    /// Closes the connections that follow a board of the organization, or of one of
    /// its workspaces, by its read token: its links open nothing now.
    async fn close_shared_readers(&self, workspace_id: Option<&str>) -> Result<(), AppError> {
        for board_id in self.store().shared_board_ids(workspace_id).await? {
            self.ws().close_readers_of_board(&board_id);
        }
        Ok(())
    }

    /// One page of the organization's members, the oldest member first.
    ///
    /// # Errors
    ///
    /// Returns the store's error if they cannot be read.
    pub async fn org_members(&self, asked: PageAsked) -> Result<Vec<OrgMember>, AppError> {
        let Some(paging) = asked.paging() else {
            return Ok(Vec::new());
        };
        Ok(self.store().org_members(paging).await?)
    }

    /// Gives a member of the organization a role in it, on behalf of `actor_id`. Who is
    /// its administrator no more is taken out of the workspaces they joined as one.
    ///
    /// # Errors
    ///
    /// Returns a 404 if the user is no member, a 409 with the reason `last_org_admin` if
    /// they are its last administrator and would stop being one, and the store's error.
    pub async fn set_org_role(
        &self,
        user_id: &str,
        role: OrgRole,
        actor_id: &str,
    ) -> Result<(), AppError> {
        if !is_id(user_id) {
            return Err(AppError::not_found("member"));
        }
        match self.store().set_org_role(user_id, role, actor_id).await {
            Ok(OrgRoleChange::Done) => Ok(()),
            Ok(OrgRoleChange::NotAMember) => Err(AppError::not_found("member")),
            Ok(OrgRoleChange::LastAdmin) => Err(AppError::conflict(
                "the organization must keep an administrator",
            )
            .with_reason(Reason::LastOrgAdmin)),
            Err(error) if error.is_not_found() => Err(AppError::not_found("organization")),
            Err(error) => Err(error.into()),
        }
    }

    /// Every workspace of the organization, in use and in the trash, with what the
    /// administrator `viewer_id` is in each. Nothing of what is in them.
    ///
    /// # Errors
    ///
    /// Returns the store's error if they cannot be read.
    pub async fn org_workspaces(&self, viewer_id: &str) -> Result<Vec<OrgWorkspace>, AppError> {
        Ok(self.store().org_workspaces(viewer_id).await?)
    }

    /// The workspace of the organization as its administrators list it.
    async fn org_workspace(
        &self,
        viewer_id: &str,
        workspace_id: &str,
    ) -> Result<OrgWorkspace, AppError> {
        self.org_workspaces(viewer_id)
            .await?
            .into_iter()
            .find(|workspace| workspace.id == workspace_id)
            .ok_or_else(workspace_not_found)
    }

    /// Makes a workspace of the organization, on behalf of `actor_id`. Nobody is a
    /// member of it, its maker neither: an administrator of the organization joins it
    /// to work in it.
    ///
    /// # Errors
    ///
    /// Returns a 400 for a name that is no name, a 409 with the reason
    /// `workspace_name_taken` if another workspace in use has it, and the store's error.
    pub async fn create_org_workspace(
        &self,
        name: &str,
        actor_id: &str,
    ) -> Result<OrgWorkspace, AppError> {
        let workspace = Workspace {
            id: new_id(),
            title: workspace_name(name)?.to_owned(),
            signup_token: new_secret(),
            modified_by: actor_id.to_owned(),
            ..Workspace::default()
        };
        match self
            .store()
            .create_named_workspace(&workspace, actor_id)
            .await
        {
            Ok(()) => self.org_workspace(actor_id, &workspace.id).await,
            Err(StoreError::Conflict(Taken::WorkspaceName)) => Err(name_taken()),
            Err(error) => Err(error.into()),
        }
    }

    /// Renames a workspace of the organization that is in use, on behalf of `actor_id`.
    ///
    /// # Errors
    ///
    /// As [`App::create_org_workspace`], and a 404 if there is no such workspace in use.
    pub async fn rename_org_workspace(
        &self,
        workspace_id: &str,
        name: &str,
        actor_id: &str,
    ) -> Result<OrgWorkspace, AppError> {
        known_workspace(workspace_id)?;
        let name = workspace_name(name)?;
        match self
            .store()
            .rename_workspace(workspace_id, name, actor_id)
            .await
        {
            Ok(()) => self.org_workspace(actor_id, workspace_id).await,
            Err(StoreError::Conflict(Taken::WorkspaceName)) => Err(name_taken()),
            Err(error) if error.is_not_found() => Err(workspace_not_found()),
            Err(error) => Err(error.into()),
        }
    }

    /// Puts a workspace of the organization in the trash, on behalf of `actor_id`: it is
    /// there for nobody, its links open nothing, and it goes for good when it has been
    /// in the trash as long as a board is kept there.
    ///
    /// # Errors
    ///
    /// Returns a 404 if there is no such workspace in use, a 409 with the reason
    /// `default_workspace` for the organization's default workspace, and the store's
    /// error.
    pub async fn trash_org_workspace(
        &self,
        workspace_id: &str,
        actor_id: &str,
    ) -> Result<(), AppError> {
        known_workspace(workspace_id)?;
        match self.store().trash_workspace(workspace_id, actor_id).await {
            Ok(WorkspaceTrashing::Done) => {
                self.close_shared_readers(Some(workspace_id)).await?;
                Ok(())
            }
            Ok(WorkspaceTrashing::NotFound) => Err(workspace_not_found()),
            Ok(WorkspaceTrashing::Default) => Err(AppError::conflict(
                "the organization's default workspace cannot be deleted",
            )
            .with_reason(Reason::DefaultWorkspace)),
            Err(error) if error.is_not_found() => Err(AppError::not_found("organization")),
            Err(error) => Err(error.into()),
        }
    }

    /// Brings a workspace of the organization back from the trash as it was, on behalf
    /// of `actor_id`.
    ///
    /// # Errors
    ///
    /// Returns a 404 if no such workspace is in the trash, a 409 with the reason
    /// `workspace_name_taken` if a workspace in use took its name meanwhile, and the
    /// store's error.
    pub async fn restore_org_workspace(
        &self,
        workspace_id: &str,
        actor_id: &str,
    ) -> Result<OrgWorkspace, AppError> {
        known_workspace(workspace_id)?;
        match self.store().restore_workspace(workspace_id, actor_id).await {
            Ok(()) => self.org_workspace(actor_id, workspace_id).await,
            Err(StoreError::Conflict(Taken::WorkspaceName)) => Err(name_taken()),
            Err(error) if error.is_not_found() => Err(workspace_not_found()),
            Err(error) => Err(error.into()),
        }
    }

    /// Makes the administrator `user_id` of the organization an administrator of one of
    /// its workspaces in use, as the organization's, which the workspace's members see
    /// recorded. Nothing changes for one who is its administrator already.
    ///
    /// # Errors
    ///
    /// Returns a 404 if there is no such workspace in use, or the user administers the
    /// organization no more, and the store's error.
    pub async fn join_org_workspace(
        &self,
        workspace_id: &str,
        user_id: &str,
    ) -> Result<OrgWorkspace, AppError> {
        known_workspace(workspace_id)?;
        match self
            .store()
            .join_workspace_as_org_admin(workspace_id, user_id)
            .await?
        {
            Joining::Joined | Joining::Already => self.org_workspace(user_id, workspace_id).await,
            Joining::NotFound => Err(workspace_not_found()),
            Joining::NoOrgAdmin => Err(AppError::not_found("organization")),
        }
    }

    /// Takes the administrator `user_id` of the organization out of a workspace they
    /// joined as one: they are what they were in it before, or no member, which the
    /// workspace's members see recorded. Nothing changes for one who did not join it so.
    ///
    /// # Errors
    ///
    /// Returns a 404 if the organization has no such workspace, and the store's error.
    pub async fn leave_org_workspace(
        &self,
        workspace_id: &str,
        user_id: &str,
    ) -> Result<OrgWorkspace, AppError> {
        known_workspace(workspace_id)?;
        self.store()
            .leave_workspace_as_org_admin(workspace_id, user_id)
            .await?;
        self.org_workspace(user_id, workspace_id).await
    }

    /// One page of the records of the organization itself, the newest first.
    ///
    /// # Errors
    ///
    /// Returns the store's error if they cannot be read.
    pub async fn org_audit(&self, asked: PageAsked) -> Result<Vec<AuditRecord>, AppError> {
        let Some(paging) = asked.paging() else {
            return Ok(Vec::new());
        };
        Ok(self
            .store()
            .audit_records(AuditPlace::Organization, paging)
            .await?)
    }

    /// One page of the records of a workspace, the newest first, as the member
    /// `reader_id` reads them: the title of a board is told where the reader sees the
    /// board, and of no other.
    ///
    /// # Errors
    ///
    /// Returns the store's error if they cannot be read.
    pub async fn workspace_audit(
        &self,
        reader_id: &str,
        workspace_id: &str,
        asked: PageAsked,
    ) -> Result<Vec<AuditRecord>, AppError> {
        let Some(paging) = asked.paging() else {
            return Ok(Vec::new());
        };
        let mut records = self
            .store()
            .audit_records(AuditPlace::Workspace(workspace_id), paging)
            .await?;
        let mut titles: HashMap<String, Option<String>> = HashMap::new();
        for record in &mut records {
            if !matches!(
                record.action,
                AuditAction::SelfAddedToBoard | AuditAction::AddedToBoard
            ) {
                continue;
            }
            if !titles.contains_key(&record.target_id) {
                let title = self
                    .board_access(reader_id, &record.target_id)
                    .await?
                    .filter(|access| {
                        access.grant.sees() && access.board.workspace_id == workspace_id
                    })
                    .map(|access| access.board.title);
                titles.insert(record.target_id.clone(), title);
            }
            record.target_name = titles.get(&record.target_id).cloned().flatten();
        }
        Ok(records)
    }

    /// Whether the boards of the application's organization may be read by a public
    /// link.
    ///
    /// # Errors
    ///
    /// Returns the store's error if the organization cannot be read.
    pub async fn public_links_on(&self) -> Result<bool, AppError> {
        Ok(self.organization().await?.public_links)
    }
}
