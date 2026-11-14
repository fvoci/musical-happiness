//! Making a user a member of a board, and recording it where that is to be accounted for.

use fvoci_model::{BoardMember, BoardRole, WorkspaceRole};

use crate::Store;
use crate::audit::{self, AuditEntry};
use crate::boards::{
    MemberChange, fetch_members, find_member, hold_board, lock_board, save_member, select_members,
};
use crate::db::{Id, Scope};
use crate::error::StoreError;
use crate::{members, org_admin};

/// What the user who asks for a change of a board's members was, as the check of their
/// permission found them: their role in the board's workspace and their membership of
/// the board. The change is made only if they still are that when it is written.
#[derive(Debug, Clone, Copy)]
pub struct Standing<'a> {
    /// Who asks.
    pub user_id: &'a str,
    /// Their role in the board's workspace; `None` if they were no member of it.
    pub workspace_role: Option<WorkspaceRole>,
    /// Their membership of the board; `None` if they were no member of it.
    pub member: Option<&'a BoardMember>,
    /// Their highest inherited board role when authorization was checked.
    pub team_role: Option<BoardRole>,
}

impl Store {
    /// Makes a user a member of a board as [`Store::add_member`] does, on the word of
    /// who asked: only if they are still what `asked` says, read with the organization
    /// held, as taking back a joining holds it (`joinings::give_back_roles`). One whose
    /// role went meanwhile gives none, and the board is not there for them.
    ///
    /// # Errors
    ///
    /// As [`Store::add_member`], and [`StoreError::NotFound`] (the board) if who asked
    /// is not what they were.
    pub async fn add_member_asked(
        &self,
        member: &BoardMember,
        asked: Standing<'_>,
    ) -> Result<Option<BoardMember>, StoreError> {
        self.transaction(async |scope| {
            standing_holds(scope, &member.board_id, asked).await?;
            add_member(scope, member).await
        })
        .await
    }

    /// Replaces the roles of a membership as [`Store::change_member`] does, on the word
    /// of who asked, as [`Store::add_member_asked`] takes it.
    ///
    /// # Errors
    ///
    /// As [`Store::change_member`], and [`StoreError::NotFound`] (the board) if who
    /// asked is not what they were.
    pub async fn change_member_asked(
        &self,
        member: &BoardMember,
        asked: Standing<'_>,
    ) -> Result<MemberChange, StoreError> {
        self.transaction(async |scope| {
            standing_holds(scope, &member.board_id, asked).await?;
            change_member(scope, member).await
        })
        .await
    }

    /// Returns the members of a board who are members of its workspace now, by the
    /// identifier of the user: who left it keeps a membership that opens nothing, and
    /// is not listed.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the query fails.
    pub async fn get_present_members_for_board(
        &self,
        board_id: &str,
    ) -> Result<Vec<BoardMember>, StoreError> {
        self.with_conn(async |scope| {
            let mut sql = select_members(scope);
            sql.push(
                " JOIN boards AS b ON b.id = bm.board_id JOIN workspace_members AS wm \
                 ON wm.workspace_id = b.workspace_id AND wm.user_id = bm.user_id \
                 JOIN workspaces AS w ON w.id = b.workspace_id \
                 JOIN organizations AS o ON o.id = w.org_id",
            )
            .push_bind(" WHERE bm.board_id = ", Id(board_id))
            .push(" AND w.delete_at = 0 AND o.delete_at = 0")
            .push(&scope.same_organization(&[("bm", "b"), ("wm", "b"), ("w", "b")]))
            .push(" ORDER BY bm.user_id");
            fetch_members(scope, sql).await
        })
        .await
    }

    /// Replaces the scheme flags of a membership, unless that would leave the board
    /// without an administrator.
    ///
    /// The check and the write are one step: of two administrators who give up the role
    /// at once, one is refused.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if a statement fails.
    pub async fn change_member(&self, member: &BoardMember) -> Result<MemberChange, StoreError> {
        self.transaction(async |scope| change_member(scope, member).await)
            .await
    }

    /// Replaces the scheme flags of a membership as [`Store::change_member`] does and, if
    /// it is changed, records `entry` with it, in one step.
    ///
    /// # Errors
    ///
    /// As [`Store::change_member`], and [`StoreError::Database`] if the record cannot be
    /// written; the membership is not changed then.
    pub async fn change_member_recorded(
        &self,
        member: &BoardMember,
        entry: &AuditEntry,
    ) -> Result<MemberChange, StoreError> {
        self.transaction(async |scope| {
            manager_still(scope, entry).await?;
            let change = change_member(scope, member).await?;
            if change == MemberChange::Done {
                audit::record_entry(scope, entry).await?;
            }
            Ok(change)
        })
        .await
    }

    /// Makes a user a member of a board unless they are one already, and says which it
    /// was: `None` if the membership is new, the membership as it stands if there was
    /// one. Two requests that add the same user at once end in one membership, the one
    /// that was stored first.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NotFound`] if the board or the user does not exist, and
    /// [`StoreError::InvalidId`] if the user's identifier is not one. Text that is no
    /// identifier names no board, as an identifier does that nothing has.
    pub async fn add_member(
        &self,
        member: &BoardMember,
    ) -> Result<Option<BoardMember>, StoreError> {
        self.transaction(async |scope| add_member(scope, member).await)
            .await
    }

    /// Makes a user a member of a board as [`Store::add_member`] does and, if the
    /// membership is new, records `entry` with it, in one step: nothing is recorded of a
    /// membership that was there, and no membership is made without its record.
    ///
    /// # Errors
    ///
    /// As [`Store::add_member`], and [`StoreError::Database`] if the record cannot be
    /// written; the membership is not made then.
    pub async fn add_member_recorded(
        &self,
        member: &BoardMember,
        entry: &AuditEntry,
    ) -> Result<Option<BoardMember>, StoreError> {
        self.transaction(async |scope| {
            manager_still(scope, entry).await?;
            let existing = add_member(scope, member).await?;
            if existing.is_none() {
                audit::record_entry(scope, entry).await?;
            }
            Ok(existing)
        })
        .await
    }
}

async fn add_member(
    scope: &mut Scope<'_>,
    member: &BoardMember,
) -> Result<Option<BoardMember>, StoreError> {
    // Held so that "there was one already" is still true when it is answered:
    // without the lock a removal in between would make this say "new" of a
    // membership that does not exist.
    hold_board(scope, &member.board_id).await?;
    let mut sql = scope.sql(
        "INSERT INTO board_members (board_id, user_id, scheme_admin, scheme_editor, \
         scheme_commenter, scheme_viewer) VALUES (",
    );
    sql.bind(Id(&member.board_id))
        .push_bind(", ", Id(&member.user_id))
        .push_bind(", ", member.scheme_admin)
        .push_bind(", ", member.scheme_editor)
        .push_bind(", ", member.scheme_commenter)
        .push_bind(", ", member.scheme_viewer)
        .push(") ON CONFLICT (board_id, user_id) DO NOTHING");
    let added = scope
        .execute(sql)
        .await
        .map_err(|error| error.missing_reference("user"))?;
    if added > 0 {
        return Ok(None);
    }
    find_member(scope, &member.board_id, &member.user_id).await
}

async fn change_member(
    scope: &mut Scope<'_>,
    member: &BoardMember,
) -> Result<MemberChange, StoreError> {
    if !lock_board(scope, &member.board_id).await? {
        return Ok(MemberChange::NotAMember);
    }
    let Some(current) = find_member(scope, &member.board_id, &member.user_id).await? else {
        return Ok(MemberChange::NotAMember);
    };
    if current.scheme_admin
        && !member.scheme_admin
        && members::is_only_admin(scope, &member.board_id, &member.user_id).await?
    {
        return Ok(MemberChange::LastAdmin);
    }
    save_member(scope, member).await?;
    Ok(MemberChange::Done)
}

/// Whether who gives the role `entry` records still administers the board's workspace,
/// in the transaction that gives it: one whose administration ended meanwhile (an
/// organization's administrator whose joining was taken back) gives none, and the board
/// is not there for them.
async fn manager_still(scope: &mut Scope<'_>, entry: &AuditEntry) -> Result<(), StoreError> {
    if org_admin::still_workspace_admin(scope, &entry.workspace_id, &entry.actor_id).await? {
        Ok(())
    } else {
        Err(StoreError::not_found("board"))
    }
}

/// Whether who asks is still what their request was let in as: the same role in the
/// board's workspace, which is in use, and the same membership of the board, read with
/// the organization's row held and then the board's, so that a removal of the asker
/// under way is waited for and seen.
async fn standing_holds(
    scope: &mut Scope<'_>,
    board_id: &str,
    asked: Standing<'_>,
) -> Result<(), StoreError> {
    org_admin::hold_organization_row(scope).await?;
    if !lock_board(scope, board_id).await? {
        return Err(StoreError::not_found("board"));
    }
    let mut sql = scope.sql(
        "SELECT b.workspace_id FROM boards AS b JOIN workspaces AS w ON w.id = b.workspace_id \
         WHERE w.delete_at = 0 AND b.id = ",
    );
    sql.bind(Id(board_id));
    let Some(row) = scope.fetch_optional(sql).await? else {
        return Err(StoreError::not_found("board"));
    };
    let workspace_id = row.id(0)?;
    let now_role = org_admin::membership(scope, &workspace_id, asked.user_id)
        .await?
        .map(|(role, _)| role);
    let now_member = find_member(scope, board_id, asked.user_id).await?;
    let flags = |member: &BoardMember| {
        [
            member.scheme_admin,
            member.scheme_editor,
            member.scheme_commenter,
            member.scheme_viewer,
        ]
    };
    let team_role = crate::board_team_access::role_for(scope, board_id, asked.user_id).await?;
    if now_role == asked.workspace_role
        && now_member.as_ref().map(flags) == asked.member.map(flags)
        && team_role == asked.team_role
    {
        Ok(())
    } else {
        Err(StoreError::not_found("board"))
    }
}
