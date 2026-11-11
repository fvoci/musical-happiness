//! Uploads in parts (`docs/s6-design.md` 5.6): the sessions and the parts received.
//!
//! A session is the user's who opened it: every lookup names the user, and another
//! user's session is not found, like one that does not exist.

use fvoci_model::{Upload, UploadPart};

use crate::Store;
use crate::db::{Id, Row};
use crate::error::StoreError;

/// What became of a part that was to be recorded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PartRecord {
    /// The part is recorded, in place of the one recorded under its number before, whose
    /// `part_id` this is.
    Recorded {
        /// The `part_id` of the part that was recorded under the number before, if any.
        replaced: Option<String>,
    },
    /// The upload is closed or gone; nothing is recorded.
    Closed,
}

const UPLOAD_COLUMNS: &str = "id, user_id, board_id, name, size, content_type, part_size, \
                              create_at, expires_at, closed_at, multipart_id";

/// What the open upload sessions hold back, for the limits on opening another.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct UploadUsage {
    /// How many sessions the user has open.
    pub user_sessions: i64,
    /// The sum of the sizes of the user's open sessions, in bytes.
    pub user_bytes: i64,
    /// The sum of the sizes of every open session of the server, in bytes.
    pub server_bytes: i64,
}

impl Store {
    /// Returns what the open upload sessions hold back at `now`: those that are not
    /// closed and have not ended. A session that was closed and is not thrown away yet,
    /// as after a crash, does not count.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the query fails.
    pub async fn upload_usage(&self, user_id: &str, now: i64) -> Result<UploadUsage, StoreError> {
        self.with_conn(async |scope| {
            let mut sql = scope.sql("SELECT CAST(COALESCE(SUM(CASE WHEN user_id = ");
            sql.bind(Id(user_id))
                .push(" THEN 1 ELSE 0 END), 0) AS BIGINT), CAST(COALESCE(SUM(CASE WHEN user_id = ")
                .bind(Id(user_id))
                .push(
                    " THEN size ELSE 0 END), 0) AS BIGINT), CAST(COALESCE(SUM(size), 0) AS BIGINT) \
                     FROM uploads WHERE closed_at = 0 AND expires_at > ",
                )
                .bind(now);
            let row = scope.fetch_optional(sql).await?.ok_or(StoreError::NoRows)?;
            Ok(UploadUsage {
                user_sessions: row.int(0)?,
                user_bytes: row.int(1)?,
                server_bytes: row.int(2)?,
            })
        })
        .await
    }

    /// Returns the upload sessions to throw away at `now`: those open that have ended,
    /// and those closed before `closed_before` that are still there, as when the server
    /// stopped between closing a session and throwing it away.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the query fails.
    pub async fn uploads_to_clean_up(
        &self,
        now: i64,
        closed_before: i64,
    ) -> Result<Vec<Upload>, StoreError> {
        self.with_conn(async |scope| {
            // A closed session is left to whoever closed it for the grace, also past its
            // end: a completion that began just before the end is not cut off.
            let mut sql = scope.sql(&format!(
                "SELECT {UPLOAD_COLUMNS} FROM uploads WHERE (closed_at = 0 AND expires_at <= "
            ));
            sql.bind(now)
                .push_bind(") OR (closed_at <> 0 AND closed_at < ", closed_before)
                .push(") ORDER BY expires_at");
            scope
                .fetch_all(sql)
                .await?
                .iter()
                .map(upload_from_row)
                .collect()
        })
        .await
    }

    /// Returns the identifiers of every upload session there is, whatever its state.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the query fails.
    pub async fn upload_ids(&self) -> Result<Vec<String>, StoreError> {
        self.with_conn(async |scope| {
            let sql = scope.sql("SELECT id FROM uploads");
            scope
                .fetch_all(sql)
                .await?
                .iter()
                .map(|row| row.text(0))
                .collect()
        })
        .await
    }

    /// Stores a new upload session.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::InvalidId`] if the user or the board is named by text that
    /// is no identifier, and [`StoreError::Database`] if the statement fails, also for a
    /// user or a board that does not exist.
    pub async fn create_upload(&self, upload: &Upload) -> Result<(), StoreError> {
        self.with_conn(async |scope| {
            let mut sql = scope.sql(&format!("INSERT INTO uploads ({UPLOAD_COLUMNS}) VALUES ("));
            sql.bind(&upload.id)
                .push_bind(", ", Id(&upload.user_id))
                .push_bind(", ", Id(&upload.board_id))
                .push_bind(", ", &upload.name)
                .push_bind(", ", upload.size)
                .push_bind(", ", &upload.content_type)
                .push_bind(", ", upload.part_size)
                .push_bind(", ", upload.create_at)
                .push_bind(", ", upload.expires_at)
                .push_bind(", ", upload.closed_at)
                .push_bind(", ", &upload.multipart_id)
                .push(")");
            scope.execute(sql).await?;
            Ok(())
        })
        .await
    }

    /// Returns the user's upload session with the given identifier that has not ended
    /// at `now`.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NotFound`] if there is none: no session of that identifier,
    /// another user's, or one that has ended.
    pub async fn get_upload(
        &self,
        upload_id: &str,
        user_id: &str,
        now: i64,
    ) -> Result<Upload, StoreError> {
        self.with_conn(async |scope| {
            let mut sql = scope.sql(&format!("SELECT {UPLOAD_COLUMNS} FROM uploads WHERE id = "));
            sql.bind(upload_id)
                .push_bind(" AND user_id = ", Id(user_id))
                .push_bind(" AND expires_at > ", now);
            let row = scope
                .fetch_optional(sql)
                .await
                // A user named by text that is no identifier has no session.
                .or_else(|error| match error {
                    StoreError::InvalidId(_) => Ok(None),
                    error => Err(error),
                })?
                .ok_or_else(|| StoreError::not_found("upload"))?;
            upload_from_row(&row)
        })
        .await
    }

    /// Returns the parts of an upload that were received, by number.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the query fails.
    pub async fn get_upload_parts(&self, upload_id: &str) -> Result<Vec<UploadPart>, StoreError> {
        self.with_conn(async |scope| {
            let mut sql =
                scope.sql("SELECT number, size, part_id FROM upload_parts WHERE upload_id = ");
            sql.bind(upload_id).push(" ORDER BY number");
            scope
                .fetch_all(sql)
                .await?
                .iter()
                .map(|row| {
                    Ok(UploadPart {
                        number: row.int(0)?,
                        size: row.int(1)?,
                        part_id: row.text(2)?,
                    })
                })
                .collect()
        })
        .await
    }

    /// Records a part of an upload that is open, replacing what was recorded under its
    /// number. Records nothing if the upload is closed or gone.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if a statement fails.
    pub async fn put_upload_part(
        &self,
        upload_id: &str,
        part: &UploadPart,
    ) -> Result<PartRecord, StoreError> {
        self.transaction(async |scope| {
            // The upload's row is held until the part is recorded: a close, or a
            // deletion, waits for this, or this sees it. On SQLite a transaction that
            // writes holds the whole database.
            let mut sql =
                scope.sql("UPDATE uploads SET closed_at = closed_at WHERE closed_at = 0 AND id = ");
            sql.bind(upload_id);
            if scope.execute(sql).await? == 0 {
                return Ok(PartRecord::Closed);
            }
            let mut sql = scope.sql("SELECT part_id FROM upload_parts WHERE upload_id = ");
            sql.bind(upload_id).push_bind(" AND number = ", part.number);
            let replaced = match scope.fetch_optional(sql).await? {
                Some(row) => Some(row.text(0)?),
                None => None,
            };
            let mut sql =
                scope.sql("INSERT INTO upload_parts (upload_id, number, size, part_id) VALUES (");
            sql.bind(upload_id)
                .push_bind(", ", part.number)
                .push_bind(", ", part.size)
                .push_bind(", ", &part.part_id)
                .push(
                    ") ON CONFLICT (upload_id, number) DO UPDATE \
                     SET size = EXCLUDED.size, part_id = EXCLUDED.part_id",
                );
            scope.execute(sql).await?;
            Ok(PartRecord::Recorded { replaced })
        })
        .await
    }

    /// Closes an open upload to new parts at `now`. Returns whether this call closed it:
    /// `false` if it was closed already or is gone, so that of two calls at once one
    /// goes on.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the statement fails.
    pub async fn close_upload(&self, upload_id: &str, now: i64) -> Result<bool, StoreError> {
        self.with_conn(async |scope| {
            let mut sql = scope.sql("UPDATE uploads SET closed_at = ");
            sql.bind(now)
                .push_bind(" WHERE closed_at = 0 AND id = ", upload_id);
            Ok(scope.execute(sql).await? == 1)
        })
        .await
    }

    /// Deletes an upload session and the record of its parts. One that is gone already
    /// is left gone.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if a statement fails.
    pub async fn delete_upload(&self, upload_id: &str) -> Result<(), StoreError> {
        self.transaction(async |scope| {
            let mut sql = scope.sql("DELETE FROM upload_parts WHERE upload_id = ");
            sql.bind(upload_id);
            scope.execute(sql).await?;
            let mut sql = scope.sql("DELETE FROM uploads WHERE id = ");
            sql.bind(upload_id);
            scope.execute(sql).await?;
            Ok(())
        })
        .await
    }
}

fn upload_from_row(row: &Row) -> Result<Upload, StoreError> {
    Ok(Upload {
        id: row.text(0)?,
        user_id: row.id(1)?,
        board_id: row.id(2)?,
        name: row.text(3)?,
        size: row.int(4)?,
        content_type: row.text(5)?,
        part_size: row.int(6)?,
        create_at: row.int(7)?,
        expires_at: row.int(8)?,
        closed_at: row.int(9)?,
        multipart_id: row.text(10)?,
    })
}
