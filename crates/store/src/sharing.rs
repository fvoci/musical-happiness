//! Public sharing of boards through read tokens.

use fvoci_model::Sharing;

use crate::Store;
use crate::db::{Scope, now_millis};
use crate::error::StoreError;

impl Store {
    /// Stores the sharing state of the board that `sharing` names, replacing any earlier
    /// state.
    ///
    /// The modification time stored is the current time, not `sharing`'s.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the statement fails.
    pub async fn upsert_sharing(&self, sharing: &Sharing) -> Result<(), StoreError> {
        self.with_conn(async |scope| upsert_sharing(scope, sharing).await)
            .await
    }

    /// Returns the sharing state of a board.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NoRows`] if no sharing state was ever stored for the board.
    pub async fn get_sharing(&self, board_id: &str) -> Result<Sharing, StoreError> {
        self.with_conn(async |scope| get_sharing(scope, board_id).await)
            .await
    }
}

pub(crate) async fn upsert_sharing(
    scope: &mut Scope<'_>,
    sharing: &Sharing,
) -> Result<(), StoreError> {
    let mut sql = scope.sql(&format!(
        "INSERT INTO {} (id, enabled, token, modified_by, update_at) VALUES (",
        scope.table("sharing")
    ));
    sql.bind(&sharing.id)
        .push_bind(", ", sharing.enabled)
        .push_bind(", ", &sharing.token)
        .push_bind(", ", &sharing.modified_by)
        .push_bind(", ", now_millis())
        .push(
            ") ON CONFLICT (id) DO UPDATE SET enabled = EXCLUDED.enabled, \
             token = EXCLUDED.token, modified_by = EXCLUDED.modified_by, \
             update_at = EXCLUDED.update_at",
        );
    scope.execute(sql).await?;
    Ok(())
}

pub(crate) async fn get_sharing(
    scope: &mut Scope<'_>,
    board_id: &str,
) -> Result<Sharing, StoreError> {
    let mut sql = scope.sql(&format!(
        "SELECT id, enabled, token, modified_by, update_at FROM {} WHERE id = ",
        scope.table("sharing")
    ));
    sql.bind(board_id);
    // Preserved quirk: a board without sharing state is reported with the database
    // layer's "no rows" text, which clients of the sharing API see
    // (`server/services/store/sqlstore/sharing.go:57-66`).
    let row = scope.fetch_optional(sql).await?.ok_or(StoreError::NoRows)?;
    Ok(Sharing {
        id: row.text(0)?,
        enabled: row.bool(1)?,
        token: row.text(2)?,
        modified_by: row.text(3)?,
        update_at: row.int(4)?,
    })
}
