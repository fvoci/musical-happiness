//! Which block shows which file of its board, and since when a file is shown by none
//! (`docs/s6-design.md` 5.7, the C of the files a body no longer shows).
//!
//! Every write of a block puts down the files it shows. A block in the trash keeps them:
//! undoing and restoring stay safe for the grace period. A file that no block shows,
//! live or in the trash, is marked with the time it became so, and a file marked longer
//! ago than the grace period is removed by [`Store::collect_unreferenced_files`].

use fvoci_model::{Block, FileInfo};

use crate::Store;
use crate::db::{Id, Scope, now_millis};
use crate::error::StoreError;
use crate::file_info::{FILE_INFO_COLUMNS, file_info_from_row};

impl Store {
    /// Removes the records of the files that no block has shown since before `before`
    /// (milliseconds), and returns them: their files are to be removed from the file
    /// storage.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if a statement fails; nothing is removed then.
    pub async fn collect_unreferenced_files(
        &self,
        before: i64,
    ) -> Result<Vec<FileInfo>, StoreError> {
        self.transaction(async |scope| {
            let mut sql = scope.sql(&format!(
                "SELECT {FILE_INFO_COLUMNS} FROM file_info \
                 WHERE unreferenced_at IS NOT NULL AND unreferenced_at < "
            ));
            sql.bind(before);
            let files = scope
                .fetch_all(sql)
                .await?
                .iter()
                .map(file_info_from_row)
                .collect::<Result<Vec<_>, _>>()?;
            for file in &files {
                let mut sql = scope.sql("DELETE FROM file_info WHERE id = ");
                sql.bind(Id(&file.id));
                scope.execute(sql).await?;
            }
            Ok(files)
        })
        .await
    }
}

/// Puts down the files a block shows now, in place of those it showed, and marks the
/// files of its board.
pub(crate) async fn refresh(scope: &mut Scope<'_>, block: &Block) -> Result<(), StoreError> {
    let mut sql = scope.sql("DELETE FROM file_refs WHERE block_id = ");
    sql.bind(Id(&block.id));
    scope.execute(sql).await?;
    for name in block.shown_files() {
        let mut sql = scope.sql("INSERT INTO file_refs (block_id, board_id, name) VALUES (");
        sql.bind(Id(&block.id))
            .push_bind(", ", Id(&block.board_id))
            .push_bind(", ", name.as_str())
            .push(")");
        scope.execute(sql).await?;
    }
    mark(scope, Some(&block.board_id)).await
}

/// Forgets what the blocks that are gone for good showed: neither on a board nor in the
/// trash.
pub(crate) async fn forget_gone_blocks(scope: &mut Scope<'_>) -> Result<(), StoreError> {
    let sql = scope.sql(
        "DELETE FROM file_refs WHERE block_id NOT IN (SELECT id FROM blocks) \
         AND block_id NOT IN (SELECT id FROM deleted_blocks)",
    );
    scope.execute(sql).await?;
    Ok(())
}

/// Marks the files of a board, or of every board: a file a block shows is not marked,
/// and one no block shows keeps its mark or gets the time of now.
pub(crate) async fn mark(scope: &mut Scope<'_>, board_id: Option<&str>) -> Result<(), StoreError> {
    let mut sql = scope.sql(
        "UPDATE file_info SET unreferenced_at = CASE WHEN EXISTS (\
         SELECT 1 FROM file_refs r WHERE r.board_id = file_info.board_id \
         AND r.name = file_info.name) THEN NULL ELSE COALESCE(unreferenced_at, ",
    );
    sql.bind(now_millis()).push(") END");
    if let Some(board_id) = board_id {
        sql.push_bind(" WHERE board_id = ", Id(board_id));
    }
    scope.execute(sql).await?;
    Ok(())
}
