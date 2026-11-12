//! Who is in a workspace: its members, what each is in it, and taking one out.
//!
//! The caller has asked whether the user may manage the members
//! (`permissions::WorkspacePermission::ManageMembers`); what a change of role makes of
//! a member is `permissions::may_become`.

use fvoci_model::{BoardMember, BoardRole, WorkspaceMember, WorkspaceRole, is_id};
use fvoci_store::MembershipChange;

use super::App;
use crate::error::{AppError, Reason};
use crate::permissions;

/// What a change is refused with that would leave a workspace without an administrator.
const LAST_ADMIN: &str = "the workspace must keep an administrator";

fn no_such_member() -> AppError {
    AppError::not_found("member")
}

impl App {
    /// The members of the workspace, its guests too, with their names: the oldest
    /// membership first.
    ///
    /// # Errors
    ///
    /// Returns the store's error if they cannot be read.
    pub async fn workspace_members(
        &self,
        workspace_id: &str,
    ) -> Result<Vec<WorkspaceMember>, AppError> {
        Ok(self.store().get_workspace_members(workspace_id).await?)
    }

    /// Makes a member of the workspace what `role` says, on behalf of `by`, and returns
    /// the member. Who becomes a guest stops being an admin of the workspace's boards,
    /// and a board that would be left without one passes to `by`.
    ///
    /// # Errors
    ///
    /// Returns a 404 for somebody who is no member of the workspace, a 400 for a change
    /// that is not made so (a guest to a member, an administrator to a guest), and a
    /// 409 (reason `last_admin`) if the workspace would be left without an
    /// administrator.
    pub async fn change_workspace_role(
        &self,
        workspace_id: &str,
        user_id: &str,
        role: WorkspaceRole,
        by: &str,
    ) -> Result<WorkspaceMember, AppError> {
        let member = self.workspace_member(workspace_id, user_id).await?;
        if member.role == role {
            return Ok(member);
        }
        if !permissions::may_become(member.role, role) {
            let how = match member.role {
                WorkspaceRole::Guest => "a guest becomes a member by an invitation for members",
                WorkspaceRole::Admin | WorkspaceRole::Member => {
                    "an administrator is made a member before a guest"
                }
            };
            return Err(AppError::bad_request(how).with_field("role"));
        }
        let change = self
            .store()
            .change_workspace_role(workspace_id, user_id, role, by)
            .await?;
        self.settle(workspace_id, by, change)?;
        tracing::info!(
            workspace_id,
            user_id,
            by,
            role = role.as_str(),
            "what a member is in a workspace was changed"
        );
        self.workspace_member(workspace_id, user_id).await
    }

    /// Takes a member out of the workspace, on behalf of `by`: its workspace is not
    /// there for them from then on, and their roles on its boards go with them. What
    /// they wrote stays, under their name. A board that would be left without an admin
    /// passes to `by`.
    ///
    /// # Errors
    ///
    /// Returns a 404 for somebody who is no member of the workspace, a 403 for `by`
    /// themselves (somebody else takes an administrator out), and a 409 (reason
    /// `last_admin`) if the workspace would be left without an administrator.
    pub async fn remove_workspace_member(
        &self,
        workspace_id: &str,
        user_id: &str,
        by: &str,
    ) -> Result<(), AppError> {
        self.workspace_member(workspace_id, user_id).await?;
        if user_id == by {
            return Err(AppError::forbidden(
                "another administrator takes an administrator out of the workspace",
            ));
        }
        let change = self
            .store()
            .remove_workspace_member(workspace_id, user_id, by)
            .await?;
        self.settle(workspace_id, by, change)?;
        tracing::info!(
            workspace_id,
            user_id,
            by,
            "a member was taken out of a workspace"
        );
        Ok(())
    }

    /// One member of the workspace; 404 for somebody who is none.
    async fn workspace_member(
        &self,
        workspace_id: &str,
        user_id: &str,
    ) -> Result<WorkspaceMember, AppError> {
        if !is_id(user_id) {
            return Err(no_such_member());
        }
        self.store()
            .get_workspace_member(workspace_id, user_id)
            .await?
            .ok_or_else(no_such_member)
    }

    /// What the store made of a change, as the client is told it; the boards that
    /// passed to `by` are announced to who is on them.
    fn settle(
        &self,
        workspace_id: &str,
        by: &str,
        change: MembershipChange,
    ) -> Result<(), AppError> {
        match change {
            MembershipChange::Done { taken_over } => {
                for board_id in taken_over {
                    tracing::info!(
                        workspace_id,
                        board_id,
                        by,
                        "a board passed to who manages the workspace"
                    );
                    let admin = BoardMember {
                        board_id,
                        user_id: by.to_owned(),
                        ..BoardMember::default()
                    }
                    .with_role(Some(BoardRole::Admin));
                    self.broadcast_member_change_in_background(workspace_id.to_owned(), admin);
                }
                Ok(())
            }
            MembershipChange::NotAMember => Err(no_such_member()),
            MembershipChange::LastAdmin => {
                Err(AppError::conflict(LAST_ADMIN).with_reason(Reason::LastAdmin))
            }
        }
    }
}
