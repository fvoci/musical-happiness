//! The trash: deleting boards and blocks, and bringing them back.
//!
//! The tables of boards and blocks hold what exists. Deleting moves a row out of them
//! into `deleted_boards` or `deleted_blocks`, whole, as JSON; restoring moves it back.
//! So no query of what exists has to leave out what is deleted, and the foreign keys of
//! what exists hold without exceptions.
//!
//! Blocks that go with one deletion share a `deletion_id` and come back together: a
//! card with what is on it, a board with all its blocks. A board takes its memberships
//! and its file records into the trash and brings them back; its sharing, the places
//! users filed it under and the subscriptions to its blocks end with the deletion.
//!
//! What has been in the trash for longer than the retention period is removed for good
//! by [`Store::purge_trash`].

use std::collections::{HashMap, HashSet};

use fvoci_model::{Block, Board, BoardMember, Body, FileInfo, new_id};
use serde::de::DeserializeOwned;

use crate::Store;
use crate::blocks::{self, parents_first};
use crate::boards;
use crate::db::{Dialect, Id, OptId, Param, Row, Scope, now_millis};
use crate::error::StoreError;
use crate::file_info;
use crate::users;

/// A block in the trash.
#[derive(Debug, Clone, PartialEq)]
pub struct DeletedBlock {
    /// The block as it was, with its `delete_at` set to when it was deleted.
    pub block: Block,
    /// The identifier of the user who deleted it; empty if nobody did.
    pub deleted_by: String,
}

/// A board in the trash.
#[derive(Debug, Clone, PartialEq)]
pub struct DeletedBoard {
    /// The board as it was, with its `delete_at` set to when it was deleted.
    pub board: Board,
    /// The memberships the board had.
    pub members: Vec<BoardMember>,
    /// The identifier of the user who deleted it; empty if nobody did.
    pub deleted_by: String,
}

impl Store {
    /// Deletes a block and everything below it: moves them to the trash, as one
    /// deletion. Subscriptions to them end. Deleting a block that does not exist
    /// succeeds.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if a statement fails; nothing is deleted then.
    pub async fn delete_block(&self, block_id: &str, user_id: &str) -> Result<(), StoreError> {
        self.transaction(async |scope| delete_block(scope, block_id, user_id).await)
            .await
    }

    /// Returns a block that is in the trash.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NotFound`] if no block of that identifier is in the trash.
    pub async fn get_deleted_block(&self, block_id: &str) -> Result<DeletedBlock, StoreError> {
        self.with_conn(async |scope| {
            let mut sql = scope.sql("SELECT block, deleted_by FROM deleted_blocks WHERE id = ");
            sql.bind(Id(block_id));
            let row = scope
                .fetch_optional(sql)
                .await?
                .ok_or_else(block_not_in_trash)?;
            Ok(DeletedBlock {
                block: block_from_json(&row, 0)?,
                deleted_by: row.id(1)?,
            })
        })
        .await
    }

    /// Restores a block from the trash, with every block that was deleted along with
    /// it, and returns the block. The restored blocks are marked as modified by
    /// `user_id` now.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NotFound`] if no block of that identifier is in the trash,
    /// or there is nothing to restore it to: its board does not exist (any more, or is
    /// in the trash itself), or the block it was on does not.
    pub async fn undelete_block(&self, block_id: &str, user_id: &str) -> Result<Block, StoreError> {
        self.transaction(async |scope| undelete_block(scope, block_id, user_id).await)
            .await
    }

    /// Deletes a board with all its blocks: moves them to the trash, together with the
    /// board's memberships and file records. The board's sharing, the places users
    /// filed it under and the subscriptions to its blocks end.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NotFound`] if there is no such board.
    pub async fn delete_board(&self, board_id: &str, user_id: &str) -> Result<(), StoreError> {
        self.transaction(async |scope| delete_board(scope, board_id, user_id).await)
            .await
    }

    /// Returns a board that is in the trash, with the memberships it had.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NotFound`] if no board of that identifier is in the trash.
    pub async fn get_deleted_board(&self, board_id: &str) -> Result<DeletedBoard, StoreError> {
        self.with_conn(async |scope| {
            let mut sql =
                scope.sql("SELECT board, members, deleted_by FROM deleted_boards WHERE id = ");
            sql.bind(Id(board_id));
            let row = scope
                .fetch_optional(sql)
                .await?
                .ok_or_else(board_not_in_trash)?;
            Ok(DeletedBoard {
                board: from_json(&row, 0, "board")?,
                members: from_json(&row, 1, "members")?,
                deleted_by: row.id(2)?,
            })
        })
        .await
    }

    /// Restores a board from the trash and returns it: the board, the blocks that were
    /// deleted with it, its file records, and the memberships of the users who still
    /// exist. The board is marked as modified by `user_id` now. Its sharing does not
    /// come back: a restored board is not shared until someone shares it again.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NotFound`] if no board of that identifier is in the trash.
    pub async fn undelete_board(&self, board_id: &str, user_id: &str) -> Result<Board, StoreError> {
        self.transaction(async |scope| undelete_board(scope, board_id, user_id).await)
            .await
    }

    /// Removes for good what was deleted before `deleted_before` (milliseconds since
    /// the Unix epoch), and the deleted blocks of boards that are gone for good. Returns
    /// how many boards and blocks went, and the file records of the boards that went:
    /// nothing else says where their files are, so whoever keeps the files removes them
    /// now or never.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if a statement fails; nothing is removed then.
    pub async fn purge_trash(&self, deleted_before: i64) -> Result<Purged, StoreError> {
        self.transaction(async |scope| {
            let mut sql = scope.sql("SELECT files FROM deleted_boards WHERE deleted_at < ");
            sql.bind(deleted_before);
            let mut files = Vec::new();
            for row in scope.fetch_all(sql).await? {
                let of_board: StoredFiles = from_json(&row, 0, "files")?;
                files.extend(Vec::<FileInfo>::from(of_board));
            }

            let mut records = 0;
            for table in ["deleted_boards", "deleted_blocks"] {
                let mut sql = scope.sql(&format!("DELETE FROM {table} WHERE deleted_at < "));
                sql.bind(deleted_before);
                records += scope.execute(sql).await?;
            }
            // A block that was deleted on its own while its board existed has nothing to
            // come back to once the board is neither there nor in the trash.
            let sql = scope.sql(
                "DELETE FROM deleted_blocks WHERE board_id NOT IN (SELECT id FROM boards) \
                 AND board_id NOT IN (SELECT id FROM deleted_boards)",
            );
            records += scope.execute(sql).await?;
            Ok(Purged { records, files })
        })
        .await
    }
}

/// What a purge of the trash removed for good.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Purged {
    /// How many boards and blocks went.
    pub records: u64,
    /// The file records of the boards that went. Their files belong to nothing any
    /// more.
    pub files: Vec<FileInfo>,
}

fn block_not_in_trash() -> StoreError {
    StoreError::not_found("deleted block")
}

fn board_not_in_trash() -> StoreError {
    StoreError::not_found("deleted board")
}

/// Reads a block of the trash. Its body is taken as it was stored: it was checked when
/// it was written, against the allowlist of that time, which a later one may not
/// accept.
fn block_from_json(row: &Row, index: usize) -> Result<Block, StoreError> {
    let corrupt = |error: serde_json::Error| StoreError::Corrupt {
        column: "block",
        reason: error.to_string(),
    };
    let mut value = row.json(index, "block")?;
    let body = value
        .as_object_mut()
        .and_then(|members| members.remove("body"))
        .filter(|body| !body.is_null());
    let mut block: Block = serde_json::from_value(value).map_err(corrupt)?;
    block.body = body.map(Body::from_stored);
    Ok(block)
}

/// Reads a JSON column of the trash into the type it was written from.
fn from_json<T: DeserializeOwned>(
    row: &Row,
    index: usize,
    column: &'static str,
) -> Result<T, StoreError> {
    serde_json::from_value(row.json(index, column)?).map_err(|error| StoreError::Corrupt {
        column,
        reason: error.to_string(),
    })
}

/// Starts a statement over a block and everything below it: a `WITH` clause that
/// defines `tree`, the identifiers of those blocks.
///
/// `UNION` and not `UNION ALL`: it also ends the recursion if parents ever form a cycle.
fn with_tree_of(scope: &Scope<'_>, block_id: &str) -> crate::db::Sql {
    let mut sql = scope.sql("WITH RECURSIVE tree(id) AS (SELECT id FROM blocks WHERE id = ");
    sql.bind(Id(block_id))
        .push(" UNION SELECT b.id FROM blocks AS b JOIN tree ON b.parent_id = tree.id) ");
    sql
}

pub(crate) async fn delete_block(
    scope: &mut Scope<'_>,
    block_id: &str,
    user_id: &str,
) -> Result<(), StoreError> {
    // The block says which board it is on, and the board is then held: what is below
    // the block is read, written to the trash and deleted without anything being added
    // or taken away in between. Read again under the lock, the block may be gone.
    let Some(block) = blocks::find_block(scope, block_id).await? else {
        tracing::debug!(block_id, "delete_block: block not found");
        return Ok(());
    };
    boards::lock_board(scope, &block.board_id).await?;
    let mut sql = with_tree_of(scope, block_id);
    sql.push(&format!(
        "SELECT {} FROM blocks WHERE id IN (SELECT id FROM tree)",
        blocks::BLOCK_COLUMNS
    ));
    let doomed = blocks::fetch_blocks(scope, sql).await?;
    if doomed.is_empty() {
        tracing::debug!(block_id, "delete_block: block deleted meanwhile");
        return Ok(());
    }

    put_blocks_in_trash(scope, &doomed, &new_id(), now_millis(), user_id).await?;

    // One statement for parents and children: the reference from a block to its parent
    // is checked when the statement is done.
    let mut sql = with_tree_of(scope, block_id);
    sql.push("DELETE FROM blocks WHERE id IN (SELECT id FROM tree)");
    scope.execute(sql).await?;
    Ok(())
}

/// Writes the blocks into the trash as one deletion.
async fn put_blocks_in_trash(
    scope: &mut Scope<'_>,
    blocks: &[Block],
    deletion_id: &str,
    deleted_at: i64,
    deleted_by: &str,
) -> Result<(), StoreError> {
    for block in blocks {
        let in_trash = Block {
            delete_at: deleted_at,
            ..block.clone()
        };
        let mut sql = scope.sql(
            "INSERT INTO deleted_blocks \
             (id, board_id, parent_id, deleted_at, deleted_by, deletion_id, block) VALUES (",
        );
        sql.bind(Id(&block.id))
            .push_bind(", ", Id(&block.board_id))
            .push_bind(", ", OptId(&block.parent_id))
            .push_bind(", ", deleted_at)
            .push_bind(", ", OptId(deleted_by))
            .push_bind(", ", Id(deletion_id))
            .push_bind(", ", Param::json(&in_trash)?)
            .push(")");
        scope.execute(sql).await?;
    }
    Ok(())
}

/// Takes the blocks of one deletion out of the trash and puts them back among the
/// blocks, marked as modified by `user_id` at `now`. Returns them.
async fn take_blocks_out_of_trash(
    scope: &mut Scope<'_>,
    deletion_id: &str,
    user_id: &str,
    now: i64,
) -> Result<Vec<Block>, StoreError> {
    let mut sql = scope.sql("SELECT block FROM deleted_blocks WHERE deletion_id = ");
    sql.bind(Id(deletion_id));
    let mut restored: Vec<Block> = scope
        .fetch_all(sql)
        .await?
        .iter()
        .map(|row| block_from_json(row, 0))
        .collect::<Result<_, _>>()?;
    let mut known_users = KnownUsers::default();
    for block in &mut restored {
        block.delete_at = 0;
        block.update_at = now;
        user_id.clone_into(&mut block.modified_by);
        known_users
            .forget_if_gone(scope, &mut block.created_by)
            .await?;
    }
    // A statement per block: each is checked on its own, so a parent goes first.
    for index in parents_first(&restored) {
        blocks::insert_block_row(scope, &restored[index]).await?;
    }
    let mut sql = scope.sql("DELETE FROM deleted_blocks WHERE deletion_id = ");
    sql.bind(Id(deletion_id));
    scope.execute(sql).await?;
    Ok(restored)
}

pub(crate) async fn undelete_block(
    scope: &mut Scope<'_>,
    block_id: &str,
    user_id: &str,
) -> Result<Block, StoreError> {
    // The trash says which board the block was on. The board must be there to come back
    // to, and is then held: read again under the lock, the block may be restored already.
    let mut sql = scope.sql("SELECT board_id FROM deleted_blocks WHERE id = ");
    sql.bind(Id(block_id));
    let row = scope
        .fetch_optional(sql)
        .await?
        .ok_or_else(block_not_in_trash)?;
    if !boards::lock_board(scope, &row.id(0)?).await? {
        return Err(block_not_in_trash());
    }
    let mut sql = scope.sql("SELECT deletion_id FROM deleted_blocks WHERE id = ");
    sql.bind(Id(block_id));
    let row = scope
        .fetch_optional(sql)
        .await?
        .ok_or_else(block_not_in_trash)?;
    let deletion_id = row.id(0)?;

    // For the blocks at the top of what was deleted, the block they were on must be
    // there too.
    let mut sql = scope.sql("SELECT id, parent_id FROM deleted_blocks WHERE deletion_id = ");
    sql.bind(Id(&deletion_id));
    let family: Vec<(String, String)> = scope
        .fetch_all(sql)
        .await?
        .iter()
        .map(|row| Ok((row.id(0)?, row.id(1)?)))
        .collect::<Result<_, StoreError>>()?;
    let coming_back: HashSet<&str> = family.iter().map(|(id, _)| id.as_str()).collect();
    for (_, parent_id) in &family {
        if parent_id.is_empty() || coming_back.contains(parent_id.as_str()) {
            continue;
        }
        if blocks::find_block(scope, parent_id).await?.is_none() {
            return Err(block_not_in_trash());
        }
    }

    let restored = take_blocks_out_of_trash(scope, &deletion_id, user_id, now_millis()).await?;
    restored
        .into_iter()
        .find(|block| block.id == block_id)
        .ok_or_else(block_not_in_trash)
}

pub(crate) async fn delete_board(
    scope: &mut Scope<'_>,
    board_id: &str,
    user_id: &str,
) -> Result<(), StoreError> {
    // Held from before anything of the board is read: nothing is added to the board, or
    // stored for it, between what goes to the trash and what is deleted.
    boards::lock_board_for_deletion(scope, board_id).await?;
    let board = boards::get_board(scope, board_id).await?;
    let members = boards::get_members_for_board(scope, board_id).await?;
    let files = file_info::file_infos_of_board(scope, board_id).await?;
    let board_blocks = blocks::get_blocks(scope, &blocks::BlockFilter::board(board_id)).await?;

    let now = now_millis();
    let deletion_id = new_id();
    put_blocks_in_trash(scope, &board_blocks, &deletion_id, now, user_id).await?;
    let in_trash = Board {
        delete_at: now,
        ..board
    };
    let mut sql = scope.sql(
        "INSERT INTO deleted_boards \
         (id, team_id, deleted_at, deleted_by, deletion_id, board, members, files) VALUES (",
    );
    sql.bind(Id(board_id))
        .push_bind(", ", Id(&in_trash.team_id))
        .push_bind(", ", now)
        .push_bind(", ", OptId(user_id))
        .push_bind(", ", Id(&deletion_id))
        .push_bind(", ", Param::json(&in_trash)?)
        .push_bind(", ", Param::json(&members)?)
        .push_bind(", ", Param::json(&StoredFiles::from(files))?)
        .push(")");
    scope.execute(sql).await?;

    // The blocks first: a board does not go while it has any. With the board go its
    // memberships, its sharing, its file records and where users filed it.
    let mut sql = scope.sql("DELETE FROM blocks WHERE board_id = ");
    sql.bind(Id(board_id));
    scope.execute(sql).await?;
    let mut sql = scope.sql("DELETE FROM boards WHERE id = ");
    sql.bind(Id(board_id));
    scope.execute(sql).await?;
    Ok(())
}

pub(crate) async fn undelete_board(
    scope: &mut Scope<'_>,
    board_id: &str,
    user_id: &str,
) -> Result<Board, StoreError> {
    let mut sql =
        scope.sql("SELECT board, members, files, deletion_id FROM deleted_boards WHERE id = ");
    sql.bind(Id(board_id));
    // Of two requests that restore the board at once, the second waits here and then
    // finds the trash without it.
    if scope.dialect() == Dialect::Postgres {
        sql.push(" FOR UPDATE");
    }
    let row = scope
        .fetch_optional(sql)
        .await?
        .ok_or_else(board_not_in_trash)?;
    let mut board: Board = from_json(&row, 0, "board")?;
    let members: Vec<BoardMember> = from_json(&row, 1, "members")?;
    let files: StoredFiles = from_json(&row, 2, "files")?;
    let deletion_id = row.id(3)?;

    let now = now_millis();
    board.delete_at = 0;
    board.update_at = now;
    user_id.clone_into(&mut board.modified_by);
    let mut known_users = KnownUsers::default();
    known_users
        .forget_if_gone(scope, &mut board.created_by)
        .await?;
    boards::insert_board_row(scope, &board).await?;

    for member in &members {
        // A membership of a user whose row is gone since does not come back.
        if known_users.has(scope, &member.user_id).await? {
            boards::save_member(scope, member).await?;
        }
    }
    for file in Vec::<FileInfo>::from(files) {
        file_info::save_file_info(scope, &file).await?;
    }
    take_blocks_out_of_trash(scope, &deletion_id, user_id, now).await?;

    let mut sql = scope.sql("DELETE FROM deleted_boards WHERE id = ");
    sql.bind(Id(board_id));
    scope.execute(sql).await?;
    Ok(board)
}

/// Which users still have a row, asked once per user.
///
/// What is in the trash refers to nothing, so a user's row can be removed while a board
/// or block that names the user is there. What comes back cannot name such a user.
#[derive(Debug, Default)]
struct KnownUsers(HashMap<String, bool>);

impl KnownUsers {
    async fn has(&mut self, scope: &mut Scope<'_>, user_id: &str) -> Result<bool, StoreError> {
        if let Some(known) = self.0.get(user_id) {
            return Ok(*known);
        }
        let known = users::user_row_exists(scope, user_id).await?;
        self.0.insert(user_id.to_owned(), known);
        Ok(known)
    }

    /// Clears `user_id` if it names a user whose row is gone: made by nobody we know.
    async fn forget_if_gone(
        &mut self,
        scope: &mut Scope<'_>,
        user_id: &mut String,
    ) -> Result<(), StoreError> {
        if !user_id.is_empty() && !self.has(scope, user_id).await? {
            user_id.clear();
        }
        Ok(())
    }
}

/// The file records of a board as they are kept with the board in the trash. A file
/// record's own JSON form is the one for clients, which leaves out where the file is.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct StoredFiles(Vec<StoredFile>);

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct StoredFile {
    id: String,
    board_id: String,
    stored_name: String,
    name: String,
    extension: String,
    size: i64,
    path: String,
    create_at: i64,
}

impl From<Vec<FileInfo>> for StoredFiles {
    fn from(files: Vec<FileInfo>) -> Self {
        Self(
            files
                .into_iter()
                .map(|file| StoredFile {
                    id: file.id,
                    board_id: file.board_id,
                    stored_name: file.stored_name,
                    name: file.name,
                    extension: file.extension,
                    size: file.size,
                    path: file.path,
                    create_at: file.create_at,
                })
                .collect(),
        )
    }
}

impl From<StoredFiles> for Vec<FileInfo> {
    fn from(files: StoredFiles) -> Self {
        files
            .0
            .into_iter()
            .map(|file| FileInfo {
                id: file.id,
                board_id: file.board_id,
                stored_name: file.stored_name,
                name: file.name,
                extension: file.extension,
                size: file.size,
                path: file.path,
                create_at: file.create_at,
            })
            .collect()
    }
}
