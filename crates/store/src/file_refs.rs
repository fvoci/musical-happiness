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
        self.with_conn(async |scope| {
            // One statement decides and deletes: a file that a block shows again by the
            // time its row is deleted keeps its row (PostgreSQL checks the condition
            // again against the row as committed).
            let mut sql = scope.sql(
                "DELETE FROM file_info WHERE unreferenced_at IS NOT NULL AND unreferenced_at < ",
            );
            sql.bind(before)
                .push(&format!(" RETURNING {FILE_INFO_COLUMNS}"));
            scope
                .fetch_all(sql)
                .await?
                .iter()
                .map(file_info_from_row)
                .collect()
        })
        .await
    }
}

/// Puts down the files a block shows now, in place of those it showed, and marks the
/// files of its board whose state that changes.
pub(crate) async fn refresh(scope: &mut Scope<'_>, block: &Block) -> Result<(), StoreError> {
    let mut sql = scope.sql("DELETE FROM file_refs WHERE block_id = ");
    sql.bind(Id(&block.id)).push(" RETURNING name");
    let mut names = scope
        .fetch_all(sql)
        .await?
        .iter()
        .map(|row| row.text(0))
        .collect::<Result<Vec<_>, _>>()?;
    for name in block.shown_files() {
        let mut sql = scope.sql("INSERT INTO file_refs (block_id, board_id, name) VALUES (");
        sql.bind(Id(&block.id))
            .push_bind(", ", Id(&block.board_id))
            .push_bind(", ", name.as_str())
            .push(")");
        scope.execute(sql).await?;
        names.push(name);
    }
    if names.is_empty() {
        return Ok(());
    }
    names.sort_unstable();
    names.dedup();
    mark(scope, Some((&block.board_id, &names))).await
}

/// Forgets what the blocks that are gone for good showed: neither on a board nor in the
/// trash.
pub(crate) async fn forget_gone_blocks(scope: &mut Scope<'_>) -> Result<(), StoreError> {
    let sql = scope.sql(
        "DELETE FROM file_refs WHERE NOT EXISTS (SELECT 1 FROM blocks b WHERE b.id = file_refs.block_id) \
         AND NOT EXISTS (SELECT 1 FROM deleted_blocks d WHERE d.id = file_refs.block_id)",
    );
    scope.execute(sql).await?;
    Ok(())
}

/// Marks file records: one that a block shows has no mark, and one that no block shows
/// gets the time of now if it had none. Only the records whose state flips are written.
/// `only` narrows it to some stored names of one board; `None` looks at every record.
pub(crate) async fn mark(
    scope: &mut Scope<'_>,
    only: Option<(&str, &[String])>,
) -> Result<(), StoreError> {
    let mut sql =
        scope.sql("UPDATE file_info SET unreferenced_at = CASE WHEN unreferenced_at IS NULL THEN ");
    sql.bind(now_millis()).push(
        " ELSE NULL END WHERE (unreferenced_at IS NULL) = (NOT EXISTS (\
         SELECT 1 FROM file_refs r WHERE r.board_id = file_info.board_id \
         AND r.name = file_info.name))",
    );
    if let Some((board_id, names)) = only {
        sql.push_bind(" AND board_id = ", Id(board_id));
        for (index, name) in names.iter().enumerate() {
            sql.push_bind(
                if index == 0 { " AND name IN (" } else { ", " },
                name.as_str(),
            );
        }
        sql.push(")");
    }
    scope.execute(sql).await?;
    Ok(())
}
