//! Which block shows which file of its board, and since when a file is shown by none
//! (`docs/s6-design.md` 5.7, the C of the files a body no longer shows).
//!
//! Every write of a block puts down the files it shows. A block in the trash keeps them:
//! undoing and restoring stay safe for the grace period. A file that no block shows,
//! live or in the trash, is marked with the time it became so, and a file marked longer
//! ago than the grace period is removed by [`Store::collect_unreferenced_files`], and
//! its object doomed.
//!
//! A write that makes a block show an image newly asks, in its transaction, that the
//! board has the image's file, and holds its record until it commits
//! ([`NewImages::Recorded`]): what a request checked before the write began is not
//! collected under it.

use std::collections::BTreeSet;

use fvoci_model::{Block, BlockError, FileInfo};

use crate::Store;
use crate::comment_versions;
use crate::db::{Dialect, Id, Scope, now_millis};
use crate::error::StoreError;
use crate::file_info::{FILE_INFO_COLUMN_COUNT, FILE_INFO_COLUMNS, file_info_from_row};
use crate::loose_objects;

impl Store {
    /// Removes the records of the files that no block has shown since before `before`
    /// (milliseconds), dooms their objects in the same transaction
    /// ([`Store::due_doomed_objects`]), and returns the records.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if a statement fails; nothing is removed then.
    pub async fn collect_unreferenced_files(
        &self,
        before: i64,
    ) -> Result<Vec<FileInfo>, StoreError> {
        self.transaction(async |scope| {
            // One statement decides and deletes. On PostgreSQL a record that a write
            // holds (`hold_new_images`) or changes is skipped, not waited for: left to
            // the next run, and no two transactions wait for each other's records. A
            // record whose mark a write took away since the statement began is looked
            // at again as committed, and kept.
            let org = loose_objects::org_column(scope);
            let skip_held = match scope.dialect() {
                Dialect::Postgres => " FOR UPDATE SKIP LOCKED",
                Dialect::Sqlite => "",
            };
            let mut sql = scope.sql(
                "DELETE FROM file_info WHERE id IN (SELECT id FROM file_info \
                 WHERE unreferenced_at IS NOT NULL AND unreferenced_at < ",
            );
            sql.bind(before).push(&format!(
                "{skip_held}) RETURNING {FILE_INFO_COLUMNS}{}",
                org.map_or(String::new(), |column| format!(", {column}"))
            ));
            let rows = scope.fetch_all(sql).await?;
            let mut files = Vec::with_capacity(rows.len());
            let mut doomed = Vec::with_capacity(rows.len());
            for row in &rows {
                let file = file_info_from_row(row)?;
                let org_id = org.map(|_| row.id(FILE_INFO_COLUMN_COUNT)).transpose()?;
                doomed.push((file.path.clone(), org_id));
                files.push(file);
            }
            loose_objects::doom(scope, &doomed).await?;
            Ok(files)
        })
        .await
    }
}

/// What a write of blocks asks of the files that the images of their bodies come to
/// show: those a block did not show before the write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewImages {
    /// Each is a file of the block's board, asked in the write's transaction, and its
    /// record is held until the write ends: the collector of unreferenced files
    /// ([`Store::collect_unreferenced_files`]) waits for it, or took the record first
    /// and the write is refused with [`BlockError::ImageWithoutFile`]. For what a
    /// client writes.
    Recorded,
    /// Not asked: for a write that copies blocks and points their images at copies of
    /// their files afterwards, as a duplicate and an import do.
    Unchecked,
}

/// Puts down the files a block shows now, in place of those it showed, and marks the
/// files of its board whose state that changes. A comment shows the files of its kept
/// versions too, also while it is made a block of another type. What the images of
/// the body newly show is asked as `new_images` says.
pub(crate) async fn refresh(
    scope: &mut Scope<'_>,
    block: &Block,
    new_images: NewImages,
) -> Result<(), StoreError> {
    let mut sql = scope.sql("DELETE FROM file_refs WHERE block_id = ");
    sql.bind(Id(&block.id)).push(" RETURNING name");
    let mut names = scope
        .fetch_all(sql)
        .await?
        .iter()
        .map(|row| row.text(0))
        .collect::<Result<Vec<_>, _>>()?;
    if new_images == NewImages::Recorded {
        hold_new_images(scope, block, &names).await?;
    }
    // What the block shows, and what the kept versions of a comment show, also of one
    // that is no comment for now.
    let mut shown: BTreeSet<String> = block.shown_files().into_iter().collect();
    shown.extend(comment_versions::files_of(scope, &block.id).await?);
    for name in shown {
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

/// Holds the records of the files that the images of a block's body show and that it
/// did not show before (`before`), until the transaction ends; refuses the block if
/// its board has no record of one. On PostgreSQL the collector skips a record this
/// lock holds ([`Store::collect_unreferenced_files`]), and a record it deleted first is
/// not found here once its transaction ended; on SQLite the transaction is the only
/// writer.
async fn hold_new_images(
    scope: &mut Scope<'_>,
    block: &Block,
    before: &[String],
) -> Result<(), StoreError> {
    let Some(body) = &block.body else {
        return Ok(());
    };
    let new: BTreeSet<&str> = body
        .file_ids()
        .into_iter()
        .filter(|name| !before.iter().any(|shown| shown == name))
        .collect();
    if new.is_empty() {
        return Ok(());
    }
    let mut sql = scope.sql("SELECT name FROM file_info WHERE board_id = ");
    sql.bind(Id(&block.board_id))
        .push(" AND name IN (")
        .bind_list(new.iter().copied())
        .push(")");
    if scope.dialect() == Dialect::Postgres {
        sql.push(" FOR KEY SHARE");
    }
    let recorded = scope
        .fetch_all(sql)
        .await?
        .iter()
        .map(|row| row.text(0))
        .collect::<Result<BTreeSet<_>, _>>()?;
    if new.iter().all(|name| recorded.contains(*name)) {
        Ok(())
    } else {
        Err(StoreError::InvalidBlock {
            id: block.id.clone(),
            source: BlockError::ImageWithoutFile,
        })
    }
}

/// Forgets what the blocks that are gone for good showed: neither on a board nor in the
/// trash of the organization of the row.
pub(crate) async fn forget_gone_blocks(scope: &mut Scope<'_>) -> Result<(), StoreError> {
    let statement = [
        "DELETE FROM file_refs WHERE NOT EXISTS (SELECT 1 FROM blocks b WHERE b.id = file_refs.block_id",
        &scope.same_organization(&[("b", "file_refs")]),
        ") AND NOT EXISTS (SELECT 1 FROM deleted_blocks d WHERE d.id = file_refs.block_id",
        &scope.same_organization(&[("d", "file_refs")]),
        ")",
    ]
    .concat();
    let sql = scope.sql(&statement);
    scope.execute(sql).await?;
    Ok(())
}

/// Marks file records: one that a block of its organization shows has no mark, and one
/// that no such block shows gets the time of now if it had none. Only the records whose
/// state flips are written. `only` narrows it to some stored names of one board; `None`
/// looks at every record.
pub(crate) async fn mark(
    scope: &mut Scope<'_>,
    only: Option<(&str, &[String])>,
) -> Result<(), StoreError> {
    let shown = [
        " ELSE NULL END WHERE (unreferenced_at IS NULL) = (NOT EXISTS (\
         SELECT 1 FROM file_refs r WHERE r.board_id = file_info.board_id \
         AND r.name = file_info.name",
        &scope.same_organization(&[("r", "file_info")]),
        "))",
    ]
    .concat();
    let mut sql =
        scope.sql("UPDATE file_info SET unreferenced_at = CASE WHEN unreferenced_at IS NULL THEN ");
    sql.bind(now_millis()).push(&shown);
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
