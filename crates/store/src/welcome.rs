//! Whether an account was given its welcome board. It is given one once, whatever
//! organizations it comes into: the record is the account's, not an organization's,
//! and is made with the board, in the transaction that stores it.

use fvoci_model::{ContainerMember, ContainersAndBlocks, DeleteContainersAndBlocks};

use crate::blocks::{CopyChanges, CopyTitles, DuplicateAs};
use crate::containers_and_blocks::{delete_containers_and_blocks, duplicate_container};
use crate::db::{Id, Scope};
use crate::{Store, StoreError};

impl Store {
    /// Copies the welcome board `template_id` into the workspace as a private board of
    /// the user's ([`Store::duplicate_container`], with `titles` and `changes`) and
    /// records at `now` that their account is given it, in one transaction: an account
    /// that was given one before gets `None` and nothing is copied, of calls made at
    /// once one copies, and a copy that fails records nothing.
    ///
    /// # Errors
    ///
    /// As [`Store::duplicate_container`].
    pub async fn duplicate_welcome_board(
        &self,
        template_id: &str,
        user_id: &str,
        workspace_id: &str,
        titles: &CopyTitles,
        changes: CopyChanges<'_>,
        now: i64,
    ) -> Result<Option<(ContainersAndBlocks, Vec<ContainerMember>)>, StoreError> {
        self.transaction(async |scope| {
            // First: the calls made for an account at once wait here for one another.
            if !claim(scope, user_id, now).await? {
                return Ok(None);
            }
            duplicate_container(
                scope,
                template_id,
                user_id,
                Some(workspace_id),
                DuplicateAs::Copy,
                titles,
                changes,
            )
            .await
            .map(Some)
        })
        .await
    }

    /// Records at `now` that the account has its welcome board, whatever was recorded:
    /// for a board made for whoever asked, which is made whatever is recorded. The
    /// record is this board's from here on, and no call that recorded before takes it
    /// back with a board of its own ([`Store::take_back_welcome_board`]).
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the statement fails.
    pub async fn record_welcome(&self, user_id: &str, now: i64) -> Result<(), StoreError> {
        self.with_write_conn(async |scope| {
            let mut sql = scope.sql("UPDATE users SET ");
            sql.push_bind("welcomed_at = ", now)
                .push_bind(" WHERE id = ", Id(user_id));
            scope.execute(sql).await?;
            Ok(())
        })
        .await
    }

    /// Takes back a welcome board that [`Store::duplicate_welcome_board`] stored and
    /// recorded at `recorded_at`: deletes `copies` as
    /// [`Store::delete_containers_and_blocks`] does and takes back the record, in one
    /// transaction, so that the account is given its board the next time. A record
    /// made at another time is another board's, and stays.
    ///
    /// # Errors
    ///
    /// As [`Store::delete_containers_and_blocks`]; neither is taken back then.
    pub async fn take_back_welcome_board(
        &self,
        copies: &DeleteContainersAndBlocks,
        user_id: &str,
        recorded_at: i64,
    ) -> Result<(), StoreError> {
        self.transaction(async |scope| {
            // Lock order: `organizations -> containers` (inside
            // `delete_containers_and_blocks`) before `users`, matching every role and
            // membership change (`change_instance_role`, `org_admin::set_org_role`).
            delete_containers_and_blocks(scope, copies, user_id).await?;
            let mut sql = scope.sql("UPDATE users SET welcomed_at = NULL");
            sql.push_bind(" WHERE id = ", Id(user_id))
                .push_bind(" AND welcomed_at = ", recorded_at);
            scope.execute(sql).await?;
            Ok(())
        })
        .await
    }
}

/// Records at `now` that the account is given its welcome board, unless it was given
/// one before. Returns whether this recorded it: one statement, so that one of those
/// made at once does.
async fn claim(scope: &mut Scope<'_>, user_id: &str, now: i64) -> Result<bool, StoreError> {
    let mut sql = scope.sql("UPDATE users SET ");
    sql.push_bind("welcomed_at = ", now)
        .push_bind(" WHERE id = ", Id(user_id))
        .push(" AND welcomed_at IS NULL");
    Ok(scope.execute(sql).await? > 0)
}
