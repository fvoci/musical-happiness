//! The blocks that exist: cards, views, comments and what is on a card.
//!
//! A block is its board's, and below the top level its parent's, which is a block of the
//! same board. Deleting moves a block to the trash and restoring brings it back; both
//! are in [`crate::trash`]. Lists come oldest first, by creation time and then by
//! identifier.

use std::collections::{BTreeMap, HashSet};

use fvoci_model::{
    BODY_VERSION, Block, BlockError, BlockPatch, BlockPatchBatch, BlockType, Body, BodySummary,
    TaskCount, generate_block_ids, validate_card_property_kinds,
};
use serde_json::Value;

use crate::Store;
use crate::boards;
use crate::db::{Id, OptId, Param, Row, Scope, Sql, now_millis};
use crate::error::StoreError;
use crate::file_refs;

/// The name of every block column, in the order rows are read and written.
pub(crate) const BLOCK_COLUMNS: &str = "id, parent_id, created_by, modified_by, \"schema\", type, title, \
                             fields, create_at, update_at, board_id, body";

/// The columns that [`BLOCK_COLUMNS`] does not read: what is worked out from the body
/// when it is stored.
const BODY_SUMMARY_COLUMNS: &str = "body_version, body_text, body_tasks_total, body_tasks_done";

/// The columns of a listed block: [`BLOCK_COLUMNS`] with no body, and what a list says
/// about the body in its place.
const LISTED_BLOCK_COLUMNS: &str = "id, parent_id, created_by, modified_by, \"schema\", type, title, \
                             fields, create_at, update_at, board_id, NULL, \
                             body IS NOT NULL, body_tasks_total, body_tasks_done";

/// The column of [`LISTED_BLOCK_COLUMNS`] that says whether there is a body.
const LISTED_HAS_BODY: usize = 12;
/// The column of [`LISTED_BLOCK_COLUMNS`] with the body's task items.
const LISTED_TASKS_TOTAL: usize = 13;
/// The column of [`LISTED_BLOCK_COLUMNS`] with the body's checked task items.
const LISTED_TASKS_DONE: usize = 14;

/// The order of every list of blocks.
const OLDEST_FIRST: &str = " ORDER BY create_at, id";

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

/// Whether a duplicate is an ordinary copy or a template.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DuplicateAs {
    /// An ordinary copy.
    Copy,
    /// A template.
    Template,
}

impl Store {
    /// Returns the blocks matching `filter`, oldest first.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the query fails.
    pub async fn get_blocks(&self, filter: &BlockFilter) -> Result<Vec<Block>, StoreError> {
        self.with_conn(async |scope| get_blocks(scope, filter).await)
            .await
    }

    /// Returns the blocks that match `filter` as [`Store::get_blocks`] does, but without
    /// their bodies: each comes with what a list says about its body instead, read from
    /// what was worked out when the body was stored.
    ///
    /// # Errors
    ///
    /// Returns the database's error, or [`StoreError::Corrupt`] for a row that cannot be
    /// read.
    pub async fn get_listed_blocks(
        &self,
        filter: &BlockFilter,
    ) -> Result<Vec<(Block, BodySummary)>, StoreError> {
        self.with_conn(async |scope| {
            let sql = filtered_blocks(scope, filter, LISTED_BLOCK_COLUMNS);
            scope
                .fetch_all(sql)
                .await?
                .iter()
                .map(|row| {
                    let tasks = TaskCount {
                        total: count_column(row, LISTED_TASKS_TOTAL, "body_tasks_total")?,
                        done: count_column(row, LISTED_TASKS_DONE, "body_tasks_done")?,
                    };
                    let summary = BodySummary {
                        has_body: row.bool(LISTED_HAS_BODY)?,
                        tasks,
                    };
                    Ok((block_from_row(row)?, summary))
                })
                .collect()
        })
        .await
    }

    /// Returns the blocks with the given identifiers, oldest first.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NotAllFound`] unless exactly one block is found per
    /// identifier.
    pub async fn get_blocks_by_ids(&self, ids: &[String]) -> Result<Vec<Block>, StoreError> {
        self.with_conn(async |scope| {
            let mut sql = select_blocks(scope);
            sql.push(" WHERE id IN (")
                .bind_list(ids.iter().map(|id| Id(id)))
                .push(")")
                .push(OLDEST_FIRST);
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

    /// Returns the block with the given identifier.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NotFound`] if there is no such block. A deleted block is
    /// not found.
    pub async fn get_block(&self, block_id: &str) -> Result<Block, StoreError> {
        self.with_conn(async |scope| get_block(scope, block_id).await)
            .await
    }

    /// Inserts `block`, or updates the block that already has its identifier.
    ///
    /// `block` is updated to what was stored: `user_id` as modifier and the current time
    /// as modification time and, for a new block, `user_id` as creator and the current
    /// time as creation time, whatever the caller put there: a block is created when it
    /// is stored, also one that is imported or copied. A block that names its board as its parent is stored, and
    /// comes back, without a parent: a block directly on its board has none. An update
    /// leaves the stored board, creator and creation time alone.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::InvalidBlock`] if the block fails
    /// [`Block::validate`](fvoci_model::Block::validate), its parent is not a block of
    /// its board, or the update would put it below itself; and [`StoreError::NotFound`]
    /// if its board does not exist or a block with the identifier exists on another
    /// board.
    pub async fn insert_block(&self, block: &mut Block, user_id: &str) -> Result<(), StoreError> {
        self.transaction(async |scope| insert_block(scope, block, user_id).await)
            .await
    }

    /// Inserts or updates every block as [`Store::insert_block`] does, all or none. A
    /// block may name another block of the same call as its parent, wherever in the
    /// list the two are.
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
            // Which boards the blocks are on is read first, so that the boards are
            // locked in one order whatever the order of the blocks.
            // A block that does not exist is reported where it is patched, below.
            let mut board_ids = Vec::with_capacity(batch.block_ids.len());
            for block_id in &batch.block_ids {
                if let Some(block) = find_block(scope, block_id).await? {
                    board_ids.push(block.board_id);
                }
            }
            boards::lock_boards(scope, board_ids.iter().map(String::as_str)).await?;
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

    /// Returns how many blocks of each type exist.
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
            let mut sql = select_blocks(scope);
            sql.push_bind(" WHERE (id = ", Id(block_id))
                .push_bind(" OR parent_id = ", Id(block_id))
                .push_bind(") AND board_id = ", Id(board_id))
                .push(OLDEST_FIRST);
            let family = fetch_blocks(scope, sql).await?;
            let not_found = || {
                StoreError::not_found(format_args!(
                    "block subtree BoardID={board_id} BlockID={block_id}"
                ))
            };

            let (roots, children): (Vec<Block>, Vec<Block>) = family
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
            if let Some(board) = boards::find_board(scope, board_id).await? {
                clear_copied_values(&mut copies, &board.card_properties);
            }
            insert_blocks(scope, &mut copies, user_id).await?;
            Ok(copies)
        })
        .await
    }
}

/// Starts `SELECT <block columns> FROM blocks`.
pub(crate) fn select_blocks(scope: &Scope<'_>) -> Sql {
    scope.sql(&format!("SELECT {BLOCK_COLUMNS} FROM blocks"))
}

pub(crate) async fn fetch_blocks(
    scope: &mut Scope<'_>,
    sql: Sql,
) -> Result<Vec<Block>, StoreError> {
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
        // A block that exists is not deleted.
        delete_at: 0,
        board_id: row.id(10)?,
        body: match row.json(11, "body")? {
            Value::Null => None,
            document => Some(Body::from_stored(document)),
        },
    })
}

pub(crate) async fn get_blocks(
    scope: &mut Scope<'_>,
    filter: &BlockFilter,
) -> Result<Vec<Block>, StoreError> {
    let sql = filtered_blocks(scope, filter, BLOCK_COLUMNS);
    fetch_blocks(scope, sql).await
}

/// A count the store wrote, read back.
fn count_column(row: &Row, index: usize, column: &'static str) -> Result<u32, StoreError> {
    u32::try_from(row.int(index)?).map_err(|error| StoreError::Corrupt {
        column,
        reason: error.to_string(),
    })
}

/// The query for the blocks that match `filter`, reading `columns`.
fn filtered_blocks(scope: &Scope<'_>, filter: &BlockFilter, columns: &str) -> Sql {
    let mut sql = scope.sql(&format!("SELECT {columns} FROM blocks"));
    sql.push(" WHERE 1 = 1");
    if let Some(board_id) = &filter.board_id {
        sql.push_bind(" AND board_id = ", Id(board_id));
    }
    if let Some(parent_id) = &filter.parent_id {
        // A block directly on its board has no parent, and whoever writes one may name
        // the board there. Asking for the blocks "on the board" finds them the same way.
        if filter.board_id.as_deref() == Some(parent_id.as_str()) {
            sql.push(" AND parent_id IS NULL");
        } else {
            sql.push_bind(" AND parent_id = ", Id(parent_id));
        }
    }
    // The empty type and the type `unknown` do not filter, so the blocks of type
    // `unknown` cannot be asked for.
    let block_type = filter
        .block_type
        .as_ref()
        .map(BlockType::as_str)
        .filter(|name| !name.is_empty() && *name != UNKNOWN_TYPE);
    if let Some(block_type) = block_type {
        sql.push_bind(" AND type = ", block_type);
    }
    sql.push(OLDEST_FIRST);
    // A page size of zero does not limit, and the offset of any page is then zero.
    if let Some(paging) = filter.paging.filter(|paging| paging.per_page > 0) {
        let offset = i64::from(paging.page) * i64::from(paging.per_page);
        sql.push_bind(" LIMIT ", i64::from(paging.per_page))
            .push_bind(" OFFSET ", offset);
    }
    sql
}

pub(crate) async fn find_block(
    scope: &mut Scope<'_>,
    block_id: &str,
) -> Result<Option<Block>, StoreError> {
    let mut sql = select_blocks(scope);
    sql.push_bind(" WHERE id = ", Id(block_id));
    Ok(fetch_blocks(scope, sql).await?.into_iter().next())
}

pub(crate) async fn get_block(scope: &mut Scope<'_>, block_id: &str) -> Result<Block, StoreError> {
    find_block(scope, block_id)
        .await?
        .ok_or_else(|| StoreError::not_found(format_args!("block ID={block_id}")))
}

/// Whether the board has a block with the identifier.
async fn is_block_of_board(
    scope: &mut Scope<'_>,
    block_id: &str,
    board_id: &str,
) -> Result<bool, StoreError> {
    let mut sql = scope.sql("SELECT 1 FROM blocks WHERE id = ");
    sql.bind(Id(block_id))
        .push_bind(" AND board_id = ", Id(board_id));
    Ok(scope.fetch_optional(sql).await?.is_some())
}

/// Whether the block `block_id` is `ancestor_id` or below it, at any depth.
async fn is_below(
    scope: &mut Scope<'_>,
    block_id: &str,
    ancestor_id: &str,
) -> Result<bool, StoreError> {
    // `UNION` and not `UNION ALL`: it also ends the walk if parents ever form a cycle.
    let mut sql = scope
        .sql("WITH RECURSIVE up(id, parent_id) AS (SELECT id, parent_id FROM blocks WHERE id = ");
    sql.bind(Id(block_id))
        .push(
            " UNION SELECT b.id, b.parent_id FROM blocks AS b JOIN up ON b.id = up.parent_id) \
             SELECT 1 FROM up WHERE id = ",
        )
        .bind(Id(ancestor_id));
    Ok(scope.fetch_optional(sql).await?.is_some())
}

pub(crate) async fn insert_block(
    scope: &mut Scope<'_>,
    block: &mut Block,
    user_id: &str,
) -> Result<(), StoreError> {
    insert_block_at(scope, block, user_id, None).await
}

/// Stores a block as created or changed at `at`, or now, once its board is held, if
/// that is `None`.
async fn insert_block_at(
    scope: &mut Scope<'_>,
    block: &mut Block,
    user_id: &str,
    at: Option<i64>,
) -> Result<(), StoreError> {
    let invalid = |block: &Block, source| StoreError::InvalidBlock {
        id: block.id.clone(),
        source,
    };
    block.validate().map_err(|source| invalid(block, source))?;
    // A block directly on its board has no parent. Clients of the original name the
    // board there.
    if block.parent_id == block.board_id {
        block.parent_id.clear();
    }
    // One writer per board at a time, from before anything is read: the parent that is
    // found below is still there when the block is stored.
    if !boards::lock_board(scope, &block.board_id).await? {
        return Err(StoreError::not_found(format_args!(
            "board ID={} of block {}",
            block.board_id, block.id
        )));
    }
    // Asked here so that a parent that is none, or another board's, is the request's
    // mistake and not a constraint the database reports.
    if !block.parent_id.is_empty()
        && (block.parent_id == block.id
            || !is_block_of_board(scope, &block.parent_id, &block.board_id).await?)
    {
        return Err(invalid(block, BlockError::ParentNotOnBoard));
    }
    let now = at.unwrap_or_else(now_millis);
    block.update_at = now;
    block.delete_at = 0;
    user_id.clone_into(&mut block.modified_by);

    let existing = find_block(scope, &block.id).await?;
    check_written_kinds(scope, block, existing.as_ref()).await?;
    if let Some(existing) = existing {
        // Every change of a block moves its `updateAt` on, also within one millisecond
        // and when the clock steps back: a writer that read the block before this
        // change is told so (`readAt`).
        block.update_at = now.max(existing.update_at.saturating_add(1));
        // A block that moves must not end up below itself: nothing would lead to it
        // from its board any more.
        if !block.parent_id.is_empty()
            && block.parent_id != existing.parent_id
            && is_below(scope, &block.parent_id, &block.id).await?
        {
            return Err(invalid(block, BlockError::ParentBelowBlock));
        }
        // A block stays on its board: the update is keyed by the board too, and a block
        // of that identifier on another board is not this block.
        let mut sql = scope.sql("UPDATE blocks SET ");
        sql.push_bind("parent_id = ", OptId(&block.parent_id))
            .push_bind(", modified_by = ", OptId(&block.modified_by))
            .push_bind(", \"schema\" = ", block.schema)
            .push_bind(", type = ", block.block_type.as_str())
            .push_bind(", title = ", &block.title)
            .push_bind(", fields = ", Param::json(&block.fields)?)
            .push_bind(", update_at = ", block.update_at);
        push_body(&mut sql, block.body.as_ref(), BodyColumns::Assign)?;
        sql.push_bind(" WHERE id = ", Id(&block.id))
            .push_bind(" AND board_id = ", Id(&block.board_id));
        if scope.execute(sql).await? == 0 {
            return Err(StoreError::not_found(format_args!(
                "block ID={} on board {}",
                block.id, block.board_id
            )));
        }
        file_refs::refresh(scope, block).await
    } else {
        user_id.clone_into(&mut block.created_by);
        block.create_at = now;
        insert_block_row(scope, block).await
    }
}

/// Checks that the values a write gives a card fit the kinds of the board's properties
/// (`docs/s6-design.md` 5.7): a date value for a date property, and none for another.
/// Only the values the write changes are checked; one that a change of the definitions
/// left behind stays until it is written.
async fn check_written_kinds(
    scope: &mut Scope<'_>,
    block: &Block,
    existing: Option<&Block>,
) -> Result<(), StoreError> {
    let Some(Value::Object(values)) = block.fields.get("properties") else {
        return Ok(());
    };
    let before = existing
        .and_then(|existing| existing.fields.get("properties"))
        .and_then(Value::as_object);
    let written: fvoci_model::JsonMap = values
        .iter()
        .filter(|(id, value)| before.and_then(|before| before.get(*id)) != Some(*value))
        .map(|(id, value)| (id.clone(), value.clone()))
        .collect();
    if written.is_empty() {
        return Ok(());
    }
    let Some(board) = boards::find_board(scope, &block.board_id).await? else {
        return Ok(());
    };
    validate_card_property_kinds(&written, &board.card_properties).map_err(|error| {
        StoreError::InvalidBlock {
            id: block.id.clone(),
            source: BlockError::from(error),
        }
    })
}

/// Clears the values of copied cards that no longer fit their board's definitions, as
/// [`fvoci_model::clear_values_of_another_kind`] does.
pub(crate) fn clear_copied_values(blocks: &mut [Block], definitions: &[fvoci_model::JsonMap]) {
    for block in blocks {
        if let Some(Value::Object(values)) = block.fields.get_mut("properties") {
            fvoci_model::clear_values_of_another_kind(values, definitions);
        }
    }
}

/// Inserts a block row with every column as `block` has it. For a new block, and for
/// one that comes back from the trash.
pub(crate) async fn insert_block_row(
    scope: &mut Scope<'_>,
    block: &Block,
) -> Result<(), StoreError> {
    let mut sql = scope.sql(&format!(
        "INSERT INTO blocks ({BLOCK_COLUMNS}, {BODY_SUMMARY_COLUMNS}) VALUES ("
    ));
    sql.bind(Id(&block.id))
        .push_bind(", ", OptId(&block.parent_id))
        .push_bind(", ", OptId(&block.created_by))
        .push_bind(", ", OptId(&block.modified_by))
        .push_bind(", ", block.schema)
        .push_bind(", ", block.block_type.as_str())
        .push_bind(", ", &block.title)
        .push_bind(", ", Param::json(&block.fields)?)
        .push_bind(", ", block.create_at)
        .push_bind(", ", block.update_at)
        .push_bind(", ", Id(&block.board_id));
    push_body(&mut sql, block.body.as_ref(), BodyColumns::List)?;
    sql.push(")");
    scope.execute(sql).await?;
    file_refs::refresh(scope, block).await
}

/// How [`push_body`] writes the body's columns.
#[derive(Debug, Clone, Copy)]
enum BodyColumns {
    /// As `, column = value` for an `UPDATE`.
    Assign,
    /// As `, value` in the order of [`BLOCK_COLUMNS`]' `body` and then
    /// [`BODY_SUMMARY_COLUMNS`], for an `INSERT`.
    List,
}

/// Appends the body and what is worked out from it.
fn push_body(sql: &mut Sql, body: Option<&Body>, how: BodyColumns) -> Result<(), StoreError> {
    let tasks = body.map(Body::tasks).unwrap_or_default();
    let values = [
        ("body", Param::opt_jsonb(body.map(Body::document))?),
        ("body_version", Param::OptInt(body.map(|_| BODY_VERSION))),
        ("body_text", Param::OptText(body.map(Body::text))),
        ("body_tasks_total", Param::Int(i64::from(tasks.total))),
        ("body_tasks_done", Param::Int(i64::from(tasks.done))),
    ];
    for (column, value) in values {
        match how {
            BodyColumns::Assign => sql.push_bind(&format!(", {column} = "), value),
            BodyColumns::List => sql.push_bind(", ", value),
        };
    }
    Ok(())
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
    boards::lock_boards(scope, blocks.iter().map(|block| block.board_id.as_str())).await?;
    // One time for all of them. They are stored parents first, which is not the order
    // they were given in, and a list of blocks is by creation time and then by
    // identifier: with a time for each, where a millisecond happened to end would
    // decide the order they are listed in. With one time it is that of their
    // identifiers, which whoever made them made in the order that is meant.
    let now = now_millis();
    for index in parents_first(blocks) {
        insert_block_at(scope, &mut blocks[index], user_id, Some(now)).await?;
    }
    Ok(())
}

/// The positions of the blocks in an order in which every block comes after its parent,
/// if the parent is among them. Blocks that are in a cycle of parents come last, in the
/// order they were given; they have no order that would work.
pub(crate) fn parents_first(blocks: &[Block]) -> Vec<usize> {
    let position: std::collections::HashMap<&str, usize> = blocks
        .iter()
        .enumerate()
        .map(|(index, block)| (block.id.as_str(), index))
        .collect();
    let mut placed: HashSet<usize> = HashSet::with_capacity(blocks.len());
    let mut order = Vec::with_capacity(blocks.len());
    loop {
        let before = order.len();
        for (index, block) in blocks.iter().enumerate() {
            if placed.contains(&index) {
                continue;
            }
            let waits_for_parent = position
                .get(block.parent_id.as_str())
                .is_some_and(|parent| *parent != index && !placed.contains(parent));
            if !waits_for_parent {
                placed.insert(index);
                order.push(index);
            }
        }
        if order.len() == blocks.len() {
            return order;
        }
        if order.len() == before {
            order.extend((0..blocks.len()).filter(|index| !placed.contains(index)));
            return order;
        }
    }
}

pub(crate) async fn patch_block(
    scope: &mut Scope<'_>,
    block_id: &str,
    patch: &BlockPatch,
    user_id: &str,
) -> Result<(), StoreError> {
    // The block says which board to hold; what is patched is read under the lock, so
    // that of two patches at once the second is applied to what the first left.
    let board_id = get_block(scope, block_id).await?.board_id;
    boards::lock_board(scope, &board_id).await?;
    let mut block = get_block(scope, block_id).await?;
    let stored_update_at = block.update_at;
    patch
        .check_type(&block.block_type)
        .map_err(|source| StoreError::InvalidBlock {
            id: block.id.clone(),
            source,
        })?;
    patch.apply(&mut block);
    // A patch whose block breaks the limits of a block (title, fields) is refused as
    // such (400) before it is refused for being made against an older block (409). The
    // checks of the parent still come after, in `insert_block_at`.
    block
        .validate()
        .map_err(|source| StoreError::InvalidBlock {
            id: block.id.clone(),
            source,
        })?;
    if let Some(read_at) = patch.read_at
        && read_at != stored_update_at
    {
        return Err(StoreError::Stale {
            id: block.id,
            read_at,
            update_at: stored_update_at,
        });
    }
    insert_block(scope, &mut block, user_id).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(id: &str, parent_id: &str) -> Block {
        Block {
            id: id.to_owned(),
            parent_id: parent_id.to_owned(),
            ..Block::default()
        }
    }

    fn ids(blocks: &[Block]) -> Vec<&str> {
        parents_first(blocks)
            .into_iter()
            .map(|index| blocks[index].id.as_str())
            .collect()
    }

    #[test]
    fn the_listed_columns_are_where_the_list_reads_them() {
        let columns: Vec<&str> = LISTED_BLOCK_COLUMNS.split(", ").map(str::trim).collect();
        let block_columns = BLOCK_COLUMNS.split(", ").count();
        assert_eq!(columns[block_columns - 1], "NULL", "the body is not read");
        assert_eq!(columns[LISTED_HAS_BODY], "body IS NOT NULL");
        assert_eq!(columns[LISTED_TASKS_TOTAL], "body_tasks_total");
        assert_eq!(columns[LISTED_TASKS_DONE], "body_tasks_done");
        assert_eq!(columns.len(), LISTED_TASKS_DONE + 1);
    }

    #[test]
    fn parents_come_before_their_children_whatever_the_given_order() {
        let blocks = [
            block("text", "card"),
            block("reply", "comment"),
            block("comment", "card"),
            block("card", ""),
            block("view", "elsewhere"),
        ];
        assert_eq!(
            ids(&blocks),
            ["card", "view", "text", "comment", "reply"],
            "what waits for nothing keeps its place; children follow their parents"
        );
    }

    #[test]
    fn blocks_in_a_cycle_of_parents_come_last_as_given() {
        let blocks = [
            block("a", "b"),
            block("b", "a"),
            block("own", "own"),
            block("top", ""),
        ];
        assert_eq!(ids(&blocks), ["own", "top", "a", "b"]);
    }
}
