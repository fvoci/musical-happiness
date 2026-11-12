//! Making a user a member of a board, and recording it where that is to be accounted for.

use fvoci_model::BoardMember;

use crate::Store;
use crate::audit::{self, AuditEntry};
use crate::boards::{MemberChange, find_member, hold_board, lock_board, save_member};
use crate::db::{Id, Scope};
use crate::error::StoreError;
use crate::members;

impl Store {
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
            let change = change_member(scope, member).await?;
            if change == MemberChange::Done {
                audit::record(scope, entry).await?;
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
            let existing = add_member(scope, member).await?;
            if existing.is_none() {
                audit::record(scope, entry).await?;
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
