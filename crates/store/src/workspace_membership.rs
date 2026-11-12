//! Who is in a workspace: its members as its administrators list them, a change of what
//! one of them is there, and the removal of one.
//!
//! A change and a removal are one transaction each, under a lock of the workspace, so
//! that of two who would take the last administrator away at once one is refused. Who
//! leaves a workspace, and who becomes a guest of it, stops being an admin of its
//! boards; a board that would be left without an admin by that passes to who made the
//! change. Every statement names the organization itself: the handle's.

use fvoci_model::{BoardMember, BoardRole, WorkspaceMember, WorkspaceRole};

use crate::Store;
use crate::audit::{self, AuditAction, AuditEntry};
use crate::boards::{find_member, lock_boards, save_member};
use crate::db::{Dialect, Id, Row, Scope};
use crate::error::StoreError;
use crate::{members, trash};

/// What became of a change to who is in a workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MembershipChange {
    /// It is done. The boards that would have been left without an admin by it, of
    /// which who did it is the admin now.
    Done {
        /// The identifiers of those boards.
        taken_over: Vec<String>,
    },
    /// The user is no member of the workspace, or there is no such workspace in use.
    NotAMember,
    /// It would leave the workspace without an administrator.
    LastAdmin,
}

impl Store {
    /// The members of a workspace in use of the handle's organization, its guests too,
    /// with their names: the oldest membership first, and then by identifier.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NoOrganization`] if the handle names none, and
    /// [`StoreError::Database`] if the query fails.
    pub async fn get_workspace_members(
        &self,
        workspace_id: &str,
    ) -> Result<Vec<WorkspaceMember>, StoreError> {
        self.with_conn(async |scope| {
            let mut sql = scope.sql(SELECT_MEMBERS);
            sql.bind(Id(workspace_id))
                .push_bind(" AND t.org_id = ", scope.org()?)
                .push_bind(" AND t.delete_at = ", 0_i64)
                .push(" ORDER BY wm.create_at, wm.user_id");
            scope
                .fetch_all(sql)
                .await?
                .iter()
                .map(member_from_row)
                .collect()
        })
        .await
    }

    /// One member of a workspace in use of the handle's organization, with their names;
    /// `None` for somebody who is none.
    ///
    /// # Errors
    ///
    /// As [`Store::get_workspace_members`].
    pub async fn get_workspace_member(
        &self,
        workspace_id: &str,
        user_id: &str,
    ) -> Result<Option<WorkspaceMember>, StoreError> {
        self.with_conn(async |scope| {
            let mut sql = scope.sql(SELECT_MEMBERS);
            sql.bind(Id(workspace_id))
                .push_bind(" AND t.org_id = ", scope.org()?)
                .push_bind(" AND t.delete_at = ", 0_i64)
                .push_bind(" AND wm.user_id = ", Id(user_id));
            scope
                .fetch_optional(sql)
                .await?
                .as_ref()
                .map(member_from_row)
                .transpose()
        })
        .await
    }

    /// Makes a member of a workspace what `role` says, on behalf of `by`, unless that
    /// would leave the workspace without an administrator.
    ///
    /// Who becomes a guest is an editor, and no more, of the boards of the workspace
    /// they were an admin of; a board they were the only admin of passes to `by`, who
    /// is its admin from then on, and that is recorded for each board `by` could not
    /// read before.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NoOrganization`] if the handle names none, and
    /// [`StoreError::Database`] if a statement fails. Nothing is stored then.
    pub async fn change_workspace_role(
        &self,
        workspace_id: &str,
        user_id: &str,
        role: WorkspaceRole,
        by: &str,
    ) -> Result<MembershipChange, StoreError> {
        self.transaction(async |scope| {
            let Some(current) = held_role(scope, workspace_id, user_id).await? else {
                return Ok(MembershipChange::NotAMember);
            };
            if current == WorkspaceRole::Admin
                && role != WorkspaceRole::Admin
                && is_only_administrator(scope, workspace_id, user_id).await?
            {
                return Ok(MembershipChange::LastAdmin);
            }
            let taken_over = if role == WorkspaceRole::Guest {
                let taken_over = hand_over_boards(scope, workspace_id, user_id, by).await?;
                let mut sql = scope.sql("UPDATE board_members SET scheme_admin = ");
                sql.bind(false)
                    .push_bind(", scheme_editor = ", true)
                    .push_bind(" WHERE user_id = ", Id(user_id))
                    .push_bind(" AND scheme_admin = ", true)
                    .push_bind(
                        " AND board_id IN (SELECT id FROM boards WHERE workspace_id = ",
                        Id(workspace_id),
                    )
                    .push(")");
                scope.execute(sql).await?;
                taken_over
            } else {
                Vec::new()
            };
            let mut sql = scope.sql("UPDATE workspace_members SET role = ");
            sql.bind(role.as_str())
                .push_bind(" WHERE workspace_id = ", Id(workspace_id))
                .push_bind(" AND user_id = ", Id(user_id));
            scope.execute(sql).await?;
            Ok(MembershipChange::Done { taken_over })
        })
        .await
    }

    /// Removes a member from a workspace, on behalf of `by`, unless that would leave
    /// the workspace without an administrator.
    ///
    /// The roles the user has on the boards of the workspace go with the membership,
    /// those its boards in the trash would come back with too: who comes back later
    /// starts anew. A board they were the only admin of passes to `by`, as
    /// [`Store::change_workspace_role`] says. What they wrote stays, under their name.
    /// They stay a member of the organization.
    ///
    /// # Errors
    ///
    /// As [`Store::change_workspace_role`].
    pub async fn remove_workspace_member(
        &self,
        workspace_id: &str,
        user_id: &str,
        by: &str,
    ) -> Result<MembershipChange, StoreError> {
        self.transaction(async |scope| {
            let Some(current) = held_role(scope, workspace_id, user_id).await? else {
                return Ok(MembershipChange::NotAMember);
            };
            if current == WorkspaceRole::Admin
                && is_only_administrator(scope, workspace_id, user_id).await?
            {
                return Ok(MembershipChange::LastAdmin);
            }
            let taken_over = hand_over_boards(scope, workspace_id, user_id, by).await?;
            let mut sql = scope.sql("DELETE FROM board_members WHERE user_id = ");
            sql.bind(Id(user_id))
                .push_bind(
                    " AND board_id IN (SELECT id FROM boards WHERE workspace_id = ",
                    Id(workspace_id),
                )
                .push(")");
            scope.execute(sql).await?;
            trash::forget_member_of_deleted_boards(scope, workspace_id, user_id).await?;
            let mut sql = scope.sql("DELETE FROM workspace_members WHERE workspace_id = ");
            sql.bind(Id(workspace_id))
                .push_bind(" AND user_id = ", Id(user_id));
            scope.execute(sql).await?;
            Ok(MembershipChange::Done { taken_over })
        })
        .await
    }
}

/// The members of a workspace `t` of an organization; the workspace's identifier and
/// the conditions on `t` follow.
const SELECT_MEMBERS: &str = "SELECT wm.user_id, u.username, u.display_name, wm.role, \
     wm.create_at FROM workspace_members AS wm \
     JOIN workspaces AS t ON t.id = wm.workspace_id \
     JOIN users AS u ON u.id = wm.user_id WHERE wm.workspace_id = ";

fn member_from_row(row: &Row) -> Result<WorkspaceMember, StoreError> {
    Ok(WorkspaceMember {
        user_id: row.id(0)?,
        username: row.text(1)?,
        display_name: row.text(2)?,
        role: role_from(&row.text(3)?)?,
        create_at: row.int(4)?,
    })
}

fn role_from(text: &str) -> Result<WorkspaceRole, StoreError> {
    text.parse()
        .map_err(|error: fvoci_model::UnknownRole| StoreError::Corrupt {
            column: "role",
            reason: error.to_string(),
        })
}

/// Holds a workspace in use of the scope's organization against other changes of who
/// is in it, and returns what the user is there; `None` if there is no such workspace
/// or the user is no member of it.
async fn held_role(
    scope: &mut Scope<'_>,
    workspace_id: &str,
    user_id: &str,
) -> Result<Option<WorkspaceRole>, StoreError> {
    let mut sql = scope.sql("SELECT id FROM workspaces WHERE id = ");
    sql.bind(Id(workspace_id))
        .push_bind(" AND org_id = ", scope.org()?)
        .push_bind(" AND delete_at = ", 0_i64);
    if scope.dialect() == Dialect::Postgres {
        sql.push(" FOR UPDATE");
    }
    if scope.fetch_optional(sql).await?.is_none() {
        return Ok(None);
    }
    let mut sql = scope.sql("SELECT role FROM workspace_members WHERE workspace_id = ");
    sql.bind(Id(workspace_id))
        .push_bind(" AND user_id = ", Id(user_id));
    match scope.fetch_optional(sql).await? {
        Some(row) => role_from(&row.text(0)?).map(Some),
        None => Ok(None),
    }
}

/// Whether no other member of the workspace than the user administers it.
async fn is_only_administrator(
    scope: &mut Scope<'_>,
    workspace_id: &str,
    user_id: &str,
) -> Result<bool, StoreError> {
    let mut sql = scope.sql("SELECT 1 FROM workspace_members WHERE workspace_id = ");
    sql.bind(Id(workspace_id))
        .push_bind(" AND user_id <> ", Id(user_id))
        .push_bind(" AND role = ", WorkspaceRole::Admin.as_str())
        .push(" LIMIT 1");
    Ok(scope.fetch_optional(sql).await?.is_none())
}

/// Makes `by` the admin of every board of the workspace that `user_id` is the only
/// admin of, before `user_id` stops being one, and returns those boards. A board `by`
/// had no role on, and so could not read, is recorded as taken.
async fn hand_over_boards(
    scope: &mut Scope<'_>,
    workspace_id: &str,
    user_id: &str,
    by: &str,
) -> Result<Vec<String>, StoreError> {
    let mut sql = scope.sql(
        "SELECT bm.board_id FROM board_members AS bm JOIN boards AS b ON b.id = bm.board_id \
         WHERE b.workspace_id = ",
    );
    sql.bind(Id(workspace_id))
        .push_bind(" AND bm.user_id = ", Id(user_id))
        .push_bind(" AND bm.scheme_admin = ", true)
        .push(" ORDER BY bm.board_id");
    let mut administered = Vec::new();
    for row in scope.fetch_all(sql).await? {
        administered.push(row.id(0)?);
    }
    lock_boards(scope, administered.iter().map(String::as_str)).await?;
    let mut taken_over = Vec::new();
    for board_id in administered {
        if !members::is_only_admin(scope, &board_id, user_id).await? {
            continue;
        }
        let was_there = find_member(scope, &board_id, by).await?.is_some();
        let admin = BoardMember {
            board_id: board_id.clone(),
            user_id: by.to_owned(),
            ..BoardMember::default()
        }
        .with_role(Some(BoardRole::Admin));
        save_member(scope, &admin).await?;
        if !was_there {
            let entry = AuditEntry {
                workspace_id: workspace_id.to_owned(),
                actor_id: by.to_owned(),
                action: AuditAction::SelfAddedToBoard,
                target_id: board_id.clone(),
                create_at: 0,
            };
            audit::record(scope, &entry).await?;
        }
        taken_over.push(board_id);
    }
    Ok(taken_over)
}
