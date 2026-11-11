//! Uploads in parts (`docs/s6-design.md` 5.6): the sessions and the parts received.
//!
//! A session is the user's who opened it: every lookup names the user, and another
//! user's session is not found, like one that does not exist.

use fvoci_model::{Upload, UploadPart};

use crate::Store;
use crate::db::{Id, Row};
use crate::error::StoreError;

const UPLOAD_COLUMNS: &str = "id, user_id, board_id, name, size, content_type, part_size, \
                              create_at, expires_at, closed_at, multipart_id";

impl Store {
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
    /// number. Returns `false`, and records nothing, if the upload is closed or gone.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if a statement fails.
    pub async fn put_upload_part(
        &self,
        upload_id: &str,
        part: &UploadPart,
    ) -> Result<bool, StoreError> {
        self.transaction(async |scope| {
            let mut sql = scope.sql("SELECT 1 FROM uploads WHERE closed_at = 0 AND id = ");
            sql.bind(upload_id);
            if scope.fetch_optional(sql).await?.is_none() {
                return Ok(false);
            }
            // Two requests for one number at once both get here; the later one wins.
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
            Ok(true)
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
