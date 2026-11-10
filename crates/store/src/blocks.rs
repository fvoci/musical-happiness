//! Blocks and their history.
//!
//! Every write of a block also appends a row to the history table. Deleting a block
//! removes it from the main table and appends a history row with `delete_at` set;
//! undeleting restores the newest history row.

use std::collections::BTreeMap;

use fvoci_model::{Block, BlockError, BlockPatch, BlockPatchBatch, BlockType, generate_block_ids};
use serde_json::Value;

use crate::Store;
use crate::db::{Id, OptId, Param, Row, Scope, Sql, now_millis};
use crate::error::StoreError;
use crate::file_info::mark_files_deleted;

/// The columns of a block, in the order [`block_from_row`] reads them.
const BLOCK_COLUMNS: &str = "id, parent_id, created_by, modified_by, \"schema\", type, title, \
                             fields, create_at, update_at, delete_at, board_id";

/// The type string the original gives to a type name it does not recognize
/// (`TypeUnknown`, server/model/blocktype.go:17).
const UNKNOWN_TYPE: &str = "unknown";

/// Which blocks [`Store::get_blocks`] returns. Unset members do not filter.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BlockFilter {
    /// Only blocks of this board.
    pub board_id: Option<String>,
    /// Only blocks with this parent.
    pub parent_id: Option<String>,
    /// Only blocks of this type. The empty type and the type `unknown` do not filter
    /// either.
    pub block_type: Option<BlockType>,
    /// Only one page of the result.
    pub paging: Option<Paging>,
}

impl BlockFilter {
    /// All blocks of a board.
    pub fn board(board_id: &str) -> Self {
        Self {
            board_id: Some(board_id.to_owned()),
            ..Self::default()
        }
    }

    /// Narrows the filter to blocks with the given parent.
    #[must_use]
    pub fn with_parent(mut self, parent_id: &str) -> Self {
        self.parent_id = Some(parent_id.to_owned());
        self
    }

    /// Narrows the filter to blocks of the given type.
    #[must_use]
    pub fn with_type(mut self, block_type: BlockType) -> Self {
        self.block_type = Some(block_type);
        self
    }
}

/// One page of a result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Paging {
    /// The zero-based page number.
    pub page: u32,
    /// The number of records per page. Zero means all records, whatever the page number.
    pub per_page: u32,
}

/// Bounds for [`Store::get_subtree`]. Unset members do not filter.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SubtreeQuery {
    /// Only blocks last updated at or before this time, in milliseconds since the epoch.
    pub before_update_at: Option<i64>,
    /// Only blocks last updated at or after this time.
    pub after_update_at: Option<i64>,
    /// At most this many blocks.
    pub limit: Option<u32>,
}

/// The order of history records.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum HistoryOrder {
    /// Oldest first.
    #[default]
    Ascending,
    /// Newest first.
    Descending,
}

/// Bounds for history queries. Unset members do not filter.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HistoryQuery {
    /// Only records updated strictly before this time, in milliseconds since the epoch.
    pub before_update_at: Option<i64>,
    /// Only records updated strictly after this time.
    pub after_update_at: Option<i64>,
    /// At most this many records.
    pub limit: Option<u32>,
    /// Whether the oldest or the newest record comes first.
    pub order: HistoryOrder,
}

impl HistoryQuery {
    /// The newest record only.
    pub(crate) const NEWEST: Self = Self {
        before_update_at: None,
        after_update_at: None,
        limit: Some(1),
        order: HistoryOrder::Descending,
    };
}

/// Whether a duplicate is an ordinary copy or a template.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DuplicateAs {
    /// An ordinary copy.
    Copy,
    /// A template.
    Template,
}

impl Store {
    /// Returns the blocks matching `filter`, in no particular order.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the query fails.
    pub async fn get_blocks(&self, filter: &BlockFilter) -> Result<Vec<Block>, StoreError> {
        self.with_conn(async |scope| get_blocks(scope, filter).await)
            .await
    }

    /// Returns the blocks with the given identifiers, in no particular order.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NotAllFound`] unless exactly one block is found per
    /// identifier.
    pub async fn get_blocks_by_ids(&self, ids: &[String]) -> Result<Vec<Block>, StoreError> {
        self.with_conn(async |scope| {
            let mut sql = select_blocks(scope, "blocks");
            sql.push(" WHERE id IN (")
                .bind_list(ids.iter().map(|id| Id(id)))
                .push(")");
            let blocks = fetch_blocks(scope, sql).await?;
            if blocks.len() == ids.len() {
                Ok(blocks)
            } else {
                Err(StoreError::NotAllFound {
                    entity: "block",
                    ids: ids.to_vec(),
                })
            }
        })
        .await
    }

    /// Returns a block of a board together with its direct children, oldest first.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the query fails.
    pub async fn get_subtree(
        &self,
        board_id: &str,
        block_id: &str,
        query: &SubtreeQuery,
    ) -> Result<Vec<Block>, StoreError> {
        self.with_conn(async |scope| get_subtree(scope, board_id, block_id, query).await)
            .await
    }

    /// Returns the block with the given identifier.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NotFound`] if there is no such block.
    pub async fn get_block(&self, block_id: &str) -> Result<Block, StoreError> {
        self.with_conn(async |scope| get_block(scope, block_id).await)
            .await
    }

    /// Inserts `block`, or updates the block that already has its identifier, and records
    /// the new state in the history.
    ///
    /// `block` is updated to what was stored: `user_id` as modifier and, for a new block,
    /// as creator, and the current time as modification time. Its `create_at` is left as
    /// given, although a new block is stored with the current time as creation time.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::InvalidBlock`] if the block fails
    /// [`Block::validate`](fvoci_model::Block::validate).
    pub async fn insert_block(&self, block: &mut Block, user_id: &str) -> Result<(), StoreError> {
        self.transaction(async |scope| insert_block(scope, block, user_id).await)
            .await
    }

    /// Inserts or updates every block as [`Store::insert_block`] does, all or none.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::InvalidBlock`] for the first block that fails validation;
    /// nothing is written in that case.
    pub async fn insert_blocks(
        &self,
        blocks: &mut [Block],
        user_id: &str,
    ) -> Result<(), StoreError> {
        self.transaction(async |scope| insert_blocks(scope, blocks, user_id).await)
            .await
    }

    /// Applies `patch` to the block with the given identifier.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NotFound`] if there is no such block and
    /// [`StoreError::InvalidBlock`] if the patched block fails validation.
    pub async fn patch_block(
        &self,
        block_id: &str,
        patch: &BlockPatch,
        user_id: &str,
    ) -> Result<(), StoreError> {
        self.transaction(async |scope| patch_block(scope, block_id, patch, user_id).await)
            .await
    }

    /// Applies each patch of `batch` to the block at the same position, all or none.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::InvalidBoardsAndBlocks`] if the batch has more identifiers
    /// than patches, and otherwise the errors of [`Store::patch_block`].
    pub async fn patch_blocks(
        &self,
        batch: &BlockPatchBatch,
        user_id: &str,
    ) -> Result<(), StoreError> {
        self.transaction(async |scope| {
            for (index, block_id) in batch.block_ids.iter().enumerate() {
                let patch = batch
                    .block_patches
                    .get(index)
                    .ok_or(fvoci_model::BoardsAndBlocksError::BlockIdsAndPatchesMismatch)?;
                patch_block(scope, block_id, patch, user_id).await?;
            }
            Ok(())
        })
        .await
    }

    /// Deletes a block and its direct children, recording the deletions in the history
    /// and marking the files they hold as deleted. Deleting a block that does not exist
    /// succeeds.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if a statement fails.
    pub async fn delete_block(&self, block_id: &str, modified_by: &str) -> Result<(), StoreError> {
        self.transaction(async |scope| delete_block(scope, block_id, modified_by).await)
            .await
    }

    /// Restores a deleted block and its deleted direct children from the history.
    /// Undeleting a block that never existed, or that is not deleted, succeeds.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if a statement fails.
    pub async fn undelete_block(
        &self,
        block_id: &str,
        modified_by: &str,
    ) -> Result<(), StoreError> {
        self.transaction(async |scope| undelete_block(scope, block_id, modified_by).await)
            .await
    }

    /// Returns the number of blocks of each type.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the query fails.
    pub async fn get_block_counts_by_type(&self) -> Result<BTreeMap<String, i64>, StoreError> {
        self.with_conn(async |scope| {
            let sql = scope.sql("SELECT type, COUNT(*) FROM blocks GROUP BY type");
            scope
                .fetch_all(sql)
                .await?
                .iter()
                .map(|row| Ok((row.text(0)?, row.int(1)?)))
                .collect()
        })
        .await
    }

    /// Returns the recorded states of a block.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the query fails.
    pub async fn get_block_history(
        &self,
        block_id: &str,
        query: &HistoryQuery,
    ) -> Result<Vec<Block>, StoreError> {
        self.with_conn(async |scope| get_block_history(scope, "id", block_id, query).await)
            .await
    }

    /// Returns the recorded states of every block of a board.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the query fails.
    pub async fn get_block_history_descendants(
        &self,
        board_id: &str,
        query: &HistoryQuery,
    ) -> Result<Vec<Block>, StoreError> {
        self.with_conn(async |scope| get_block_history(scope, "board_id", board_id, query).await)
            .await
    }

    /// Copies a block of a board and its direct children, except comments, under new
    /// identifiers. Returns the copies, the copy of the block itself first.
    ///
    /// The copy of the block gets its `isTemplate` field set according to `duplicate_as`.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NotFound`] if the board has no such block.
    pub async fn duplicate_block(
        &self,
        board_id: &str,
        block_id: &str,
        user_id: &str,
        duplicate_as: DuplicateAs,
    ) -> Result<Vec<Block>, StoreError> {
        self.transaction(async |scope| {
            let subtree = get_subtree(scope, board_id, block_id, &SubtreeQuery::default()).await?;
            let not_found = || {
                StoreError::not_found(format_args!(
                    "block subtree BoardID={board_id} BlockID={block_id}"
                ))
            };
            if subtree.is_empty() {
                return Err(not_found());
            }

            let (roots, children): (Vec<Block>, Vec<Block>) = subtree
                .into_iter()
                .filter(|block| block.block_type != BlockType::Comment)
                .partition(|block| block.id == block_id);
            let mut root = roots.into_iter().next().ok_or_else(not_found)?;
            root.fields.insert(
                "isTemplate".to_owned(),
                Value::Bool(duplicate_as == DuplicateAs::Template),
            );

            let mut copies = Vec::with_capacity(children.len() + 1);
            copies.push(root);
            copies.extend(children);
            generate_block_ids(&mut copies);
            insert_blocks(scope, &mut copies, user_id).await?;
            Ok(copies)
        })
        .await
    }
}

/// Starts `SELECT <block columns> FROM <table>`.
fn select_blocks(scope: &Scope<'_>, table: &str) -> Sql {
    scope.sql(&format!("SELECT {BLOCK_COLUMNS} FROM {table}"))
}

async fn fetch_blocks(scope: &mut Scope<'_>, sql: Sql) -> Result<Vec<Block>, StoreError> {
    scope
        .fetch_all(sql)
        .await?
        .iter()
        .map(block_from_row)
        .collect()
}

fn block_from_row(row: &Row) -> Result<Block, StoreError> {
    Ok(Block {
        id: row.id(0)?,
        parent_id: row.id(1)?,
        created_by: row.id(2)?,
        modified_by: row.id(3)?,
        schema: row.int(4)?,
        block_type: BlockType::from(row.text(5)?),
        title: row.text(6)?,
        fields: row.json_map(7, "fields")?,
        create_at: row.int(8)?,
        update_at: row.int(9)?,
        delete_at: row.int(10)?,
        board_id: row.id(11)?,
    })
}

pub(crate) async fn get_blocks(
    scope: &mut Scope<'_>,
    filter: &BlockFilter,
) -> Result<Vec<Block>, StoreError> {
    let mut sql = select_blocks(scope, "blocks");
    sql.push(" WHERE 1 = 1");
    if let Some(board_id) = &filter.board_id {
        sql.push_bind(" AND board_id = ", Id(board_id));
    }
    if let Some(parent_id) = &filter.parent_id {
        sql.push_bind(" AND parent_id = ", Id(parent_id));
    }
    // Preserved quirk: the empty type and the type `unknown` do not filter, so the blocks
    // of type `unknown` cannot be asked for (server/services/store/sqlstore/blocks.go:69).
    let block_type = filter
        .block_type
        .as_ref()
        .map(BlockType::as_str)
        .filter(|name| !name.is_empty() && *name != UNKNOWN_TYPE);
    if let Some(block_type) = block_type {
        sql.push_bind(" AND type = ", block_type);
    }
    // A page size of zero does not limit, and the offset of any page is then zero
    // (server/services/store/sqlstore/blocks.go:73-79).
    if let Some(paging) = filter.paging.filter(|paging| paging.per_page > 0) {
        let offset = i64::from(paging.page) * i64::from(paging.per_page);
        sql.push_bind(" LIMIT ", i64::from(paging.per_page))
            .push_bind(" OFFSET ", offset);
    }
    fetch_blocks(scope, sql).await
}

async fn get_subtree(
    scope: &mut Scope<'_>,
    board_id: &str,
    block_id: &str,
    query: &SubtreeQuery,
) -> Result<Vec<Block>, StoreError> {
    let mut sql = select_blocks(scope, "blocks");
    sql.push_bind(" WHERE (id = ", Id(block_id))
        .push_bind(" OR parent_id = ", Id(block_id))
        .push_bind(") AND board_id = ", Id(board_id));
    if let Some(before) = query.before_update_at {
        sql.push_bind(" AND update_at <= ", before);
    }
    if let Some(after) = query.after_update_at {
        sql.push_bind(" AND update_at >= ", after);
    }
    sql.push(" ORDER BY insert_at, update_at");
    if let Some(limit) = query.limit {
        sql.push_bind(" LIMIT ", i64::from(limit));
    }
    fetch_blocks(scope, sql).await
}

async fn find_block(scope: &mut Scope<'_>, block_id: &str) -> Result<Option<Block>, StoreError> {
    let mut sql = select_blocks(scope, "blocks");
    sql.push_bind(" WHERE id = ", Id(block_id));
    Ok(fetch_blocks(scope, sql).await?.into_iter().next())
}

pub(crate) async fn get_block(scope: &mut Scope<'_>, block_id: &str) -> Result<Block, StoreError> {
    find_block(scope, block_id)
        .await?
        .ok_or_else(|| StoreError::not_found(format_args!("block ID={block_id}")))
}

/// Returns the history of the blocks whose `key_column` equals `key`.
async fn get_block_history(
    scope: &mut Scope<'_>,
    key_column: &str,
    key: &str,
    query: &HistoryQuery,
) -> Result<Vec<Block>, StoreError> {
    let mut sql = select_blocks(scope, "blocks_history");
    sql.push(" WHERE ")
        .push(key_column)
        .push_bind(" = ", Id(key));
    if let Some(before) = query.before_update_at {
        sql.push_bind(" AND update_at < ", before);
    }
    if let Some(after) = query.after_update_at {
        sql.push_bind(" AND update_at > ", after);
    }
    sql.push(match query.order {
        HistoryOrder::Ascending => " ORDER BY insert_at, update_at",
        HistoryOrder::Descending => " ORDER BY insert_at DESC, update_at DESC",
    });
    if let Some(limit) = query.limit {
        sql.push_bind(" LIMIT ", i64::from(limit));
    }
    fetch_blocks(scope, sql).await
}

pub(crate) async fn insert_block(
    scope: &mut Scope<'_>,
    block: &mut Block,
    user_id: &str,
) -> Result<(), StoreError> {
    block
        .validate()
        .map_err(|source| StoreError::InvalidBlock {
            id: block.id.clone(),
            source,
        })?;
    let exists = find_block(scope, &block.id).await?.is_some();

    let now = now_millis();
    block.update_at = now;
    user_id.clone_into(&mut block.modified_by);
    let fields = Param::json(&block.fields)?;

    if exists {
        // Preserved quirk: a block that exists under another board is left untouched,
        // because the update is keyed by board too, yet its history row is still written.
        let mut sql = scope.sql("UPDATE blocks SET ");
        sql.push_bind("parent_id = ", OptId(&block.parent_id))
            .push_bind(", modified_by = ", OptId(&block.modified_by))
            .push_bind(", \"schema\" = ", block.schema)
            .push_bind(", type = ", block.block_type.as_str())
            .push_bind(", title = ", &block.title)
            .push_bind(", fields = ", fields.clone())
            .push_bind(", update_at = ", block.update_at)
            .push_bind(", delete_at = ", block.delete_at)
            .push_bind(" WHERE id = ", Id(&block.id))
            .push_bind(" AND board_id = ", Id(&block.board_id));
        scope.execute(sql).await?;
    } else {
        user_id.clone_into(&mut block.created_by);
        let mut sql = scope.sql(
            "INSERT INTO blocks (id, parent_id, created_by, modified_by, \"schema\", type, title, \
             fields, create_at, update_at, delete_at, board_id) VALUES (",
        );
        bind_block_values(&mut sql, block, user_id, &fields, now);
        sql.push(")");
        scope.execute(sql).await?;
    }

    // Preserved quirk: the history row of an update carries the modifier as creator and
    // the modification time as creation time, like the row of an insert.
    let stamp = scope.history_stamp();
    let mut sql = scope.sql(
        "INSERT INTO blocks_history (id, parent_id, created_by, modified_by, \"schema\", type, \
         title, fields, create_at, update_at, delete_at, board_id, insert_at) VALUES (",
    );
    bind_block_values(&mut sql, block, user_id, &fields, now);
    sql.push(", ").bind_history_stamp(stamp).push(")");
    scope.execute(sql).await?;
    Ok(())
}

/// Binds the values of a block row, with `user_id` as creator and
/// `now` as creation time.
fn bind_block_values(sql: &mut Sql, block: &Block, user_id: &str, fields: &Param, now: i64) {
    sql.bind(Id(&block.id))
        .push_bind(", ", OptId(&block.parent_id))
        .push_bind(", ", OptId(user_id))
        .push_bind(", ", OptId(&block.modified_by))
        .push_bind(", ", block.schema)
        .push_bind(", ", block.block_type.as_str())
        .push_bind(", ", &block.title)
        .push_bind(", ", fields.clone())
        .push_bind(", ", now)
        .push_bind(", ", block.update_at)
        .push_bind(", ", block.delete_at)
        .push_bind(", ", Id(&block.board_id));
}

pub(crate) async fn insert_blocks(
    scope: &mut Scope<'_>,
    blocks: &mut [Block],
    user_id: &str,
) -> Result<(), StoreError> {
    for block in blocks.iter() {
        block
            .validate()
            .map_err(|source| StoreError::InvalidBlock {
                id: block.id.clone(),
                source,
            })?;
    }
    for block in blocks {
        insert_block(scope, block, user_id).await?;
    }
    Ok(())
}

pub(crate) async fn patch_block(
    scope: &mut Scope<'_>,
    block_id: &str,
    patch: &BlockPatch,
    user_id: &str,
) -> Result<(), StoreError> {
    let mut block = get_block(scope, block_id).await?;
    patch.apply(&mut block);
    insert_block(scope, &mut block, user_id).await
}

/// Deletes a block and its direct children; see [`Store::delete_block`].
pub(crate) async fn delete_block(
    scope: &mut Scope<'_>,
    block_id: &str,
    modified_by: &str,
) -> Result<(), StoreError> {
    let Some(block) = find_block(scope, block_id).await? else {
        tracing::warn!(block_id, "delete_block: block not found");
        return Ok(());
    };

    let now = now_millis();
    let stamp = scope.history_stamp();
    let mut sql = scope.sql("INSERT INTO blocks_history (board_id, id, parent_id, \"schema\", type, title, fields, modified_by, \
         create_at, update_at, delete_at, created_by, insert_at) VALUES (");
    sql.bind(Id(&block.board_id))
        .push_bind(", ", Id(&block.id))
        .push_bind(", ", OptId(&block.parent_id))
        .push_bind(", ", block.schema)
        .push_bind(", ", block.block_type.as_str())
        .push_bind(", ", &block.title)
        .push_bind(", ", Param::json(&block.fields)?)
        .push_bind(", ", OptId(modified_by))
        .push_bind(", ", block.create_at)
        .push_bind(", ", now)
        .push_bind(", ", now)
        .push_bind(", ", OptId(&block.created_by))
        .push(", ")
        .bind_history_stamp(stamp)
        .push(")");
    scope.execute(sql).await?;

    mark_files_deleted(scope, &block.board_id, &stored_file_names(&block)).await?;

    let mut sql = scope.sql("DELETE FROM blocks WHERE id = ");
    sql.bind(Id(block_id));
    scope.execute(sql).await?;

    delete_block_children(scope, &block.board_id, Some(&block.id), modified_by).await
}

/// Deletes the blocks of a board that have the given parent, or every block of the board
/// if no parent is given, recording the deletions in the history.
pub(crate) async fn delete_block_children(
    scope: &mut Scope<'_>,
    board_id: &str,
    parent_id: Option<&str>,
    modified_by: &str,
) -> Result<(), StoreError> {
    let now = now_millis();
    let children_filter = |sql: &mut Sql| {
        sql.push_bind(" WHERE board_id = ", Id(board_id));
        if let Some(parent_id) = parent_id {
            sql.push_bind(" AND parent_id = ", Id(parent_id));
        }
    };

    let stamp = scope.history_stamp();
    let mut sql = scope.sql("INSERT INTO blocks_history (board_id, id, parent_id, \"schema\", type, title, fields, modified_by, \
         create_at, update_at, delete_at, created_by, insert_at) \
         SELECT board_id, id, parent_id, \"schema\", type, title, fields, ");
    sql.bind(OptId(modified_by))
        .push_bind(", create_at, ", now)
        .push_bind(", ", now)
        .push(", created_by, ")
        .bind_history_stamp(stamp)
        .push(" FROM blocks");
    children_filter(&mut sql);
    scope.execute(sql).await?;

    let mut sql = select_blocks(scope, "blocks");
    children_filter(&mut sql);
    let file_names: Vec<String> = fetch_blocks(scope, sql)
        .await?
        .iter()
        .flat_map(stored_file_names)
        .collect();
    mark_files_deleted(scope, board_id, &file_names).await?;

    let mut sql = scope.sql("DELETE FROM blocks");
    children_filter(&mut sql);
    scope.execute(sql).await?;
    Ok(())
}

/// The stored names of the files a block refers to: an image's `fileId`, an
/// attachment's `attachmentId`.
fn stored_file_names(block: &Block) -> Vec<String> {
    ["fileId", "attachmentId"]
        .into_iter()
        .filter_map(|field| block.fields.get(field)?.as_str())
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .collect()
}

pub(crate) async fn undelete_block(
    scope: &mut Scope<'_>,
    block_id: &str,
    modified_by: &str,
) -> Result<(), StoreError> {
    let newest = get_block_history(scope, "id", block_id, &HistoryQuery::NEWEST).await?;
    let Some(block) = newest.into_iter().next() else {
        tracing::warn!(block_id, "undelete_block: block not found");
        return Ok(());
    };
    if block.delete_at == 0 {
        tracing::warn!(block_id, "undelete_block: block not deleted");
        return Ok(());
    }

    let now = now_millis();
    let fields = Param::json(&block.fields)?;
    let stamp = scope.history_stamp();
    for (table, stamp) in [("blocks_history", Some(stamp)), ("blocks", None)] {
        let mut sql = scope.sql(&format!(
            "INSERT INTO {table} (board_id, id, parent_id, \"schema\", type, title, fields, \
             modified_by, create_at, update_at, delete_at, created_by{}) VALUES (",
            if stamp.is_some() { ", insert_at" } else { "" }
        ));
        sql.bind(Id(&block.board_id))
            .push_bind(", ", Id(&block.id))
            .push_bind(", ", OptId(&block.parent_id))
            .push_bind(", ", block.schema)
            .push_bind(", ", block.block_type.as_str())
            .push_bind(", ", &block.title)
            .push_bind(", ", fields.clone())
            .push_bind(", ", OptId(modified_by))
            .push_bind(", ", block.create_at)
            .push_bind(", ", now)
            .push_bind(", 0, ", OptId(&block.created_by));
        if let Some(stamp) = stamp {
            sql.push(", ").bind_history_stamp(stamp);
        }
        sql.push(")");
        scope.execute(sql).await?;
    }

    undelete_block_children(scope, &block.board_id, Some(&block.id), modified_by).await
}

/// Restores the deleted blocks of a board that have the given parent, or every deleted
/// block of the board if no parent is given, from their newest history rows.
pub(crate) async fn undelete_block_children(
    scope: &mut Scope<'_>,
    board_id: &str,
    parent_id: Option<&str>,
    modified_by: &str,
) -> Result<(), StoreError> {
    if board_id.is_empty() {
        return Err(BlockError::EmptyBoardId.into());
    }
    let now = now_millis();
    let history = "blocks_history";

    // The main table first: the second statement adds history rows, after which the
    // newest history row of each block is no longer a deleted one.
    for (table, stamp) in [
        ("blocks", None),
        ("blocks_history", Some(scope.history_stamp())),
    ] {
        let mut sql = scope.sql(&format!(
            "INSERT INTO {table} (board_id, id, parent_id, \"schema\", type, title, fields, \
             modified_by, create_at, update_at, delete_at, created_by{}) \
             SELECT bh.board_id, bh.id, bh.parent_id, bh.\"schema\", bh.type, bh.title, \
             bh.fields, ",
            if stamp.is_some() { ", insert_at" } else { "" }
        ));
        sql.bind(OptId(modified_by))
            .push_bind(", bh.create_at, ", now)
            .push(", 0, bh.created_by");
        if let Some(stamp) = stamp {
            sql.push(", ").bind_history_stamp(stamp);
        }
        sql.push(&format!(
            " FROM {history} AS bh, (SELECT id, MAX(insert_at) AS max_insert_at FROM {history}"
        ))
        .push_bind(" WHERE board_id = ", Id(board_id));
        if let Some(parent_id) = parent_id {
            sql.push_bind(" AND parent_id = ", Id(parent_id));
        }
        sql.push(
            " GROUP BY id) AS sub \
             WHERE bh.id = sub.id AND bh.insert_at = sub.max_insert_at AND bh.delete_at != 0",
        );
        scope.execute(sql).await?;
    }
    Ok(())
}
