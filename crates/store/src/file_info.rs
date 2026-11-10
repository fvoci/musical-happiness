//! Records about uploaded files.
//!
//! A file is one board's. It is stored and asked for under a name the server made up,
//! and the record ties that name to the board, to the place in the file storage and to
//! the name the file was uploaded with.

use fvoci_model::FileInfo;

use crate::Store;
use crate::db::{Id, Row, Scope, now_millis};
use crate::error::StoreError;

const FILE_INFO_COLUMNS: &str = "id, board_id, name, original_name, extension, size, path, \
                                 archived, create_at, delete_at";

impl Store {
    /// Returns the record of the file a board has under a stored name, whether marked
    /// as deleted or not.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NotFound`] if the board has no file of that name. A file of
    /// another board is not found.
    pub async fn get_file_info(
        &self,
        board_id: &str,
        stored_name: &str,
    ) -> Result<FileInfo, StoreError> {
        self.with_conn(async |scope| {
            let mut sql = scope.sql(&format!(
                "SELECT {FILE_INFO_COLUMNS} FROM file_info WHERE board_id = "
            ));
            sql.bind(Id(board_id))
                .push_bind(" AND name = ", stored_name);
            let row = scope
                .fetch_optional(sql)
                .await?
                .ok_or_else(|| StoreError::not_found("file"))?;
            file_info_from_row(&row)
        })
        .await
    }

    /// Stores a record about a file. The record is stored as not archived and not
    /// deleted, whatever `file_info` says.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::InvalidId`] if the record's or the board's identifier is not
    /// one, and [`StoreError::Database`] if the statement fails, as it does when the
    /// board has a file of that stored name already.
    pub async fn save_file_info(&self, file_info: &FileInfo) -> Result<(), StoreError> {
        self.with_conn(async |scope| {
            let mut sql = scope.sql(&format!(
                "INSERT INTO file_info ({FILE_INFO_COLUMNS}) VALUES ("
            ));
            sql.bind(Id(&file_info.id))
                .push_bind(", ", Id(&file_info.board_id))
                .push_bind(", ", &file_info.stored_name)
                .push_bind(", ", &file_info.name)
                .push_bind(", ", &file_info.extension)
                .push_bind(", ", file_info.size)
                .push_bind(", ", &file_info.path)
                .push_bind(", ", false)
                .push_bind(", ", file_info.create_at)
                .push_bind(", ", 0_i64)
                .push(")");
            scope.execute(sql).await?;
            Ok(())
        })
        .await
    }
}

fn file_info_from_row(row: &Row) -> Result<FileInfo, StoreError> {
    Ok(FileInfo {
        id: row.id(0)?,
        board_id: row.id(1)?,
        stored_name: row.text(2)?,
        name: row.text(3)?,
        extension: row.text(4)?,
        size: row.int(5)?,
        path: row.text(6)?,
        archived: row.bool(7)?,
        create_at: row.int(8)?,
        delete_at: row.int(9)?,
    })
}

/// Marks as deleted the records of the files a board has under the given stored names.
pub(crate) async fn mark_files_deleted(
    scope: &mut Scope<'_>,
    board_id: &str,
    stored_names: &[String],
) -> Result<(), StoreError> {
    if stored_names.is_empty() {
        return Ok(());
    }
    let mut sql = scope.sql("UPDATE file_info SET delete_at = ");
    sql.bind(now_millis())
        .push_bind(" WHERE board_id = ", Id(board_id))
        .push(" AND name IN (")
        .bind_list(stored_names)
        .push(")");
    scope.execute(sql).await?;
    Ok(())
}
