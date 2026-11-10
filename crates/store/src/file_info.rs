//! Records about uploaded files.
//!
//! A file is one board's. It is stored and asked for under a name the server made up,
//! and the record ties that name to the board, to the place in the file storage and to
//! the name the file was uploaded with.

use fvoci_model::FileInfo;

use crate::Store;
use crate::db::{Id, Row, Scope};
use crate::error::StoreError;

const FILE_INFO_COLUMNS: &str =
    "id, board_id, name, original_name, extension, size, path, create_at";

impl Store {
    /// Returns the record of the file a board has under a stored name.
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

    /// Stores a record about a file.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::InvalidId`] if the record's or the board's identifier is not
    /// one, and [`StoreError::Database`] if the statement fails, as it does when the
    /// board does not exist or has a file of that stored name already.
    pub async fn save_file_info(&self, file_info: &FileInfo) -> Result<(), StoreError> {
        self.with_conn(async |scope| save_file_info(scope, file_info).await)
            .await
    }
}

pub(crate) async fn save_file_info(
    scope: &mut Scope<'_>,
    file_info: &FileInfo,
) -> Result<(), StoreError> {
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
        .push_bind(", ", file_info.create_at)
        .push(")");
    scope.execute(sql).await?;
    Ok(())
}

/// The records of every file of a board, oldest first.
pub(crate) async fn file_infos_of_board(
    scope: &mut Scope<'_>,
    board_id: &str,
) -> Result<Vec<FileInfo>, StoreError> {
    let mut sql = scope.sql(&format!(
        "SELECT {FILE_INFO_COLUMNS} FROM file_info WHERE board_id = "
    ));
    sql.bind(Id(board_id)).push(" ORDER BY create_at, id");
    scope
        .fetch_all(sql)
        .await?
        .iter()
        .map(file_info_from_row)
        .collect()
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
        create_at: row.int(7)?,
    })
}
