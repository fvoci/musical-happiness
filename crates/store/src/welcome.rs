//! Whether an account was given its welcome board. It is given one once, whatever
//! organizations it comes into: the record is the account's, not an organization's.

use crate::db::Id;
use crate::{Store, StoreError};

impl Store {
    /// Records at `now` that the account is given its welcome board, unless it was
    /// given one before. Returns whether this call recorded it: of calls made at once,
    /// one does, and that one makes the board.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the statement fails.
    pub async fn claim_welcome(&self, user_id: &str, now: i64) -> Result<bool, StoreError> {
        self.with_conn(async |scope| {
            let mut sql = scope.sql("UPDATE users SET ");
            sql.push_bind("welcomed_at = ", now)
                .push_bind(" WHERE id = ", Id(user_id))
                .push(" AND welcomed_at IS NULL");
            Ok(scope.execute(sql).await? > 0)
        })
        .await
    }

    /// Takes back what [`Store::claim_welcome`] recorded: the board could not be made
    /// after all, and the account is given one the next time.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the statement fails.
    pub async fn forget_welcome(&self, user_id: &str) -> Result<(), StoreError> {
        self.with_conn(async |scope| {
            let mut sql = scope.sql("UPDATE users SET welcomed_at = NULL");
            sql.push_bind(" WHERE id = ", Id(user_id));
            scope.execute(sql).await?;
            Ok(())
        })
        .await
    }
}
