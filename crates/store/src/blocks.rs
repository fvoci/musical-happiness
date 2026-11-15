//! The blocks that exist: cards, views, comments and what is on a card.
//!
//! A block is its board's, and below the top level its parent's, which is a block of the
//! same board. Deleting moves a block to the trash and restoring brings it back; both
//! are in [`crate::trash`]. Lists come oldest first, by creation time and then by
//! identifier.

use std::collections::{BTreeMap, HashSet};

use fvoci_model::{
    BODY_VERSION, Block, BlockError, BlockPatch, BlockPatchBatch, BlockType, Body, BodySummary,
    TaskCount, generate_block_ids,
};
use serde_json::Value;

use crate::Store;
use crate::block_derived::{self, Before};
use crate::boards;
use crate::body_versions::{self, BodyWrite};
use crate::card_values::{self, NamedCards};
use crate::comment_places;
use crate::comment_versions::{self, EDITED_AT_COLUMN};
use crate::db::{Id, OptId, Param, Row, Scope, Sql, now_millis};
use crate::error::StoreError;
use crate::file_refs::NewImages;
use crate::page_places;

/// The name of every block column, in the order rows are read and written.
pub(crate) const BLOCK_COLUMNS: &str = "id, parent_id, created_by, modified_by, \"schema\", type, title, \
                             fields, create_at, update_at, board_id, body";

/// The columns a block is read with: [`BLOCK_COLUMNS`], and when its body was last
/// changed if it is a comment with versions.
pub(crate) fn block_read_columns() -> String {
    format!("{BLOCK_COLUMNS}, {EDITED_AT_COLUMN}")
}

/// The column of a block read with [`block_read_columns`] that says when its body was
/// last changed.
const EDITED_AT: usize = 12;

/// The columns that [`BLOCK_COLUMNS`] does not read: what is worked out from the body
/// when it is stored.
const BODY_SUMMARY_COLUMNS: &str = "body_version, body_text, body_tasks_total, body_tasks_done";

/// The columns of a listed block: [`BLOCK_COLUMNS`] with no body, and what a list says
/// about the body in its place.
const LISTED_BLOCK_COLUMNS: &str = "id, parent_id, created_by, modified_by, \"schema\", type, title, \
                             fields, create_at, update_at, board_id, NULL, \
                             body IS NOT NULL, body_tasks_total, body_tasks_done";

/// When the last comment on the block `blocks.id` was posted, zero for none: its
/// comments (blocks of the type `comment` whose parent it is; deleted ones are in the
/// trash) and their `create_at`, which writing a comment again does not change. Read
/// after [`LISTED_BLOCK_COLUMNS`], at [`LISTED_LAST_COMMENT_AT`], and alone by
/// [`Store::get_last_comment_at`]. `blocks_parent_type_create_at_idx` answers it.
const LAST_COMMENT_AT_COLUMN: &str = "COALESCE((SELECT MAX(c.create_at) FROM blocks c \
                                      WHERE c.parent_id = blocks.id AND c.type = 'comment'), 0)";

/// The column of [`LISTED_BLOCK_COLUMNS`] that says whether there is a body.
const LISTED_HAS_BODY: usize = 12;
/// The column of [`LISTED_BLOCK_COLUMNS`] with the body's task items.
const LISTED_TASKS_TOTAL: usize = 13;
/// The column of [`LISTED_BLOCK_COLUMNS`] with the body's checked task items.
const LISTED_TASKS_DONE: usize = 14;
/// The column of [`LISTED_BLOCK_COLUMNS`] with when the block's last comment was posted.
const LISTED_LAST_COMMENT_AT: usize = 15;
/// The columns a list reads for previews, after the last comment's time: the beginning
/// of the plain text of the body, worked out when it was stored, and the body. The text
/// is 201 characters (code points, each of one or two UTF-16 code units): the 200 units
/// a preview shows are among them, and a character as it is seen that the cut splits
/// runs past 200 units, so the preview leaves it out whole.
const PREVIEW_COLUMNS: &str = "SUBSTR(body_text, 1, 201), body";
/// The column of the plain text of the body, read for previews.
const LISTED_BODY_TEXT: usize = 16;
/// The column of the body, read for previews.
const LISTED_BODY: usize = 17;

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

/// A block as a list gives it: without its body, with what the list says about it.
#[derive(Debug, Clone, PartialEq)]
pub struct ListedBlock {
    /// The block, without its body.
    pub block: Block,
    /// What the list says about the body.
    pub summary: BodySummary,
    /// When the block's last comment was posted; zero if it has none.
    pub last_comment_at: i64,
    /// What a preview shows of the body, if the list was asked for previews.
    pub preview: Option<BodyPreview>,
}

/// What a list asked for previews reads of a block's body.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BodyPreview {
    /// The beginning of the body's plain text, as it is searched (a line per paragraph):
    /// its first 201 characters.
    pub text: String,
    /// The stored name of the file of the body's first image, if it has one.
    pub first_image: Option<String>,
}

/// Whether a list of blocks reads what previews show of their bodies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Previews {
    /// It does not.
    Without,
    /// It reads the plain text and the first image of each body.
    With,
}

impl Store {
    /// Writes the fields of a block as they are, without any check: for tests that need
    /// a block as an older server stored it.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the statement fails.
    #[cfg(feature = "test-util")]
    pub async fn set_block_fields_unchecked(
        &self,
        block_id: &str,
        fields: &fvoci_model::JsonMap,
    ) -> Result<(), StoreError> {
        self.with_conn(async |scope| {
            let mut sql = scope.sql("UPDATE blocks SET fields = ");
            sql.bind(Param::json(fields)?)
                .push_bind(" WHERE id = ", Id(block_id));
            scope.execute(sql).await?;
            Ok(())
        })
        .await
    }

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
    /// what was worked out when the body was stored, and when its last comment was posted.
    ///
    /// # Errors
    ///
    /// Returns the database's error, or [`StoreError::Corrupt`] for a row that cannot be
    /// read.
    pub async fn get_listed_blocks(
        &self,
        filter: &BlockFilter,
        previews: Previews,
    ) -> Result<Vec<ListedBlock>, StoreError> {
        self.with_conn(async |scope| {
            let mut columns = format!("{LISTED_BLOCK_COLUMNS}, {LAST_COMMENT_AT_COLUMN}");
            if previews == Previews::With {
                columns = format!("{columns}, {PREVIEW_COLUMNS}");
            }
            let sql = filtered_blocks(scope, filter, &columns);
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
                    Ok(ListedBlock {
                        block: block_from_row(row)?,
                        summary,
                        last_comment_at: row.int(LISTED_LAST_COMMENT_AT)?,
                        preview: match previews {
                            Previews::Without => None,
                            Previews::With => Some(preview_from_row(row)?),
                        },
                    })
                })
                .collect()
        })
        .await
    }

    /// Returns when the last comment on a block was posted (its `create_at`); zero if
    /// it has none. Deleted comments are in the trash and do not count.
    ///
    /// # Errors
    ///
    /// Returns the database's error.
    pub async fn get_last_comment_at(&self, block_id: &str) -> Result<i64, StoreError> {
        self.with_conn(async |scope| {
            let mut sql = scope.sql(&format!(
                "SELECT {LAST_COMMENT_AT_COLUMN} FROM blocks WHERE id = "
            ));
            sql.bind(Id(block_id));
            // No such block has no comments.
            let row = scope.fetch_optional(sql).await?;
            row.map_or(Ok(0), |row| row.int(0))
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
        self.transaction(async |scope| {
            insert_blocks(scope, blocks, user_id, NewImages::Recorded).await
        })
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
        self.transaction(async |scope| {
            patch_block(scope, block_id, patch, user_id, BodyWrite::Whole).await
        })
        .await
    }

    /// Gives blocks of a board the bodies in `bodies` (by block identifier; `None` for
    /// none), all or none, as the rest of a write that made them: no one's change, so the
    /// blocks keep who changed them last and when, and a comment gets no version. An
    /// import points the images of the bodies it stored at the files it stored after them
    /// so. The files the blocks show are put down again.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NotFound`] if the board, or one of the blocks on it, is not
    /// there.
    pub async fn replace_bodies_as_written(
        &self,
        board_id: &str,
        bodies: Vec<(String, Option<Body>)>,
    ) -> Result<(), StoreError> {
        self.transaction(async |scope| {
            if !boards::lock_board(scope, board_id).await? {
                return Err(StoreError::not_found(format_args!("board ID={board_id}")));
            }
            for (block_id, body) in bodies {
                let mut sql = scope.sql("UPDATE blocks SET update_at = update_at");
                push_body(&mut sql, body.as_ref(), BodyColumns::Assign)?;
                sql.push_bind(" WHERE id = ", Id(&block_id))
                    .push_bind(" AND board_id = ", Id(board_id));
                if scope.execute(sql).await? == 0 {
                    return Err(StoreError::not_found(format_args!(
                        "block ID={block_id} on board {board_id}"
                    )));
                }
                let block = get_block(scope, &block_id).await?;
                block_derived::refresh(scope, &block, Before::Unknown, NewImages::Recorded).await?;
            }
            Ok(())
        })
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
                patch_block(scope, block_id, patch, user_id, BodyWrite::Whole).await?;
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
            // What the source holds is settled before the copies are named, so that a
            // value that named its own card names no card; a copy on the same board then
            // names the cards its source named.
            let definitions = boards::find_board(scope, board_id)
                .await?
                .map(|board| board.card_properties)
                .unwrap_or_default();
            card_values::clear_copied_values(&mut copies, &definitions);
            generate_block_ids(&mut copies);
            card_values::keep_cards_of_board(scope, &mut copies, board_id, &definitions).await?;
            insert_blocks(scope, &mut copies, user_id, NewImages::Unchecked).await?;
            Ok(copies)
        })
        .await
    }
}

/// Starts `SELECT <block columns> FROM blocks`.
pub(crate) fn select_blocks(scope: &Scope<'_>) -> Sql {
    scope.sql(&format!("SELECT {} FROM blocks", block_read_columns()))
}

pub(crate) async fn fetch_blocks(
    scope: &mut Scope<'_>,
    sql: Sql,
) -> Result<Vec<Block>, StoreError> {
    scope
        .fetch_all(sql)
        .await?
        .iter()
        .map(|row| {
            let mut block = block_from_row(row)?;
            block.edited_at = row.int(EDITED_AT)?;
            Ok(block)
        })
        .collect()
}

/// The block of a row read with [`BLOCK_COLUMNS`] or [`LISTED_BLOCK_COLUMNS`]: without
/// [`Block::edited_at`].
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
        edited_at: 0,
    })
}

/// What a preview shows of the body of a listed row read with [`PREVIEW_COLUMNS`].
fn preview_from_row(row: &Row) -> Result<BodyPreview, StoreError> {
    let first_image = match row.json(LISTED_BODY, "body")? {
        Value::Null => None,
        document => Body::from_stored(document)
            .first_file_id()
            .map(str::to_owned),
    };
    Ok(BodyPreview {
        text: row.text(LISTED_BODY_TEXT)?,
        first_image,
    })
}

pub(crate) async fn get_blocks(
    scope: &mut Scope<'_>,
    filter: &BlockFilter,
) -> Result<Vec<Block>, StoreError> {
    let sql = filtered_blocks(scope, filter, &block_read_columns());
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
    let whole = BodyWrite::Whole;
    let named = insert_block_at(scope, block, user_id, None, NewImages::Recorded, whole).await?;
    card_values::check_named_cards(scope, &named).await
}

/// Stores a block as created or changed at `at`, or now, once its board is held, if
/// that is `None`. The images it shows newly are asked as `new_images` says, and the
/// versions of a card's body are kept as `body_write` says. Returns the cards its
/// relation values newly name, which the caller asks for once the write is stored
/// ([`card_values::check_named_cards`]).
async fn insert_block_at(
    scope: &mut Scope<'_>,
    block: &mut Block,
    user_id: &str,
    at: Option<i64>,
    new_images: NewImages,
    body_write: BodyWrite,
) -> Result<Vec<NamedCards>, StoreError> {
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
    let Some(kind) = boards::lock_board_of_kind(scope, &block.board_id).await? else {
        return Err(StoreError::not_found(format_args!(
            "board ID={} of block {}",
            block.board_id, block.id
        )));
    };
    // Asked here so that a parent that is none, or another board's, is the request's
    // mistake and not a constraint the database reports.
    let parent_type = if block.parent_id.is_empty() {
        None
    } else if block.parent_id == block.id {
        return Err(invalid(block, BlockError::ParentNotOnBoard));
    } else {
        let found = comment_places::type_on_board(scope, &block.parent_id, &block.board_id).await?;
        Some(found.ok_or_else(|| invalid(block, BlockError::ParentNotOnBoard))?)
    };
    let now = at.unwrap_or_else(now_millis);
    block.update_at = now;
    block.delete_at = 0;
    user_id.clone_into(&mut block.modified_by);

    let existing = find_block(scope, &block.id).await?;
    page_places::check(block, kind, parent_type.as_ref())?;
    comment_places::check(scope, block, parent_type.as_ref(), existing.as_ref()).await?;
    let named = card_values::check_written(scope, block, existing.as_ref()).await?;
    if let Some(existing) = existing {
        if block.block_type != existing.block_type {
            scope.invalidate_visibility();
        }
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
        block.edited_at =
            if comment_versions::record_change(scope, &existing, block, user_id).await? {
                block.update_at
            } else {
                existing.edited_at
            };
        // Before the files are put down: what the body before showed is still shown
        // by its version.
        if body_write == BodyWrite::Whole {
            body_versions::record_change(scope, &existing, block, user_id).await?;
        }
        block_derived::refresh(scope, block, Before::Stored(&existing), new_images).await?;
    } else {
        user_id.clone_into(&mut block.created_by);
        block.create_at = now;
        block.edited_at = 0;
        insert_block_row(scope, block, new_images).await?;
    }
    Ok(named)
}

/// Inserts a block row with every column as `block` has it. For a new block, and for
/// one that comes back from the trash; its images are asked as `new_images` says.
pub(crate) async fn insert_block_row(
    scope: &mut Scope<'_>,
    block: &Block,
    new_images: NewImages,
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
    block_derived::refresh(scope, block, Before::Absent, new_images).await
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
    new_images: NewImages,
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
    // What relation values name is asked for once all are stored: two new cards may
    // name each other.
    let mut named = Vec::new();
    for index in parents_first(blocks) {
        let block = &mut blocks[index];
        named.extend(
            insert_block_at(
                scope,
                block,
                user_id,
                Some(now),
                new_images,
                BodyWrite::Whole,
            )
            .await?,
        );
    }
    card_values::check_named_cards(scope, &named).await
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

/// Applies a patch to a block; the versions of a card's body are kept as `body_write`
/// says.
pub(crate) async fn patch_block(
    scope: &mut Scope<'_>,
    block_id: &str,
    patch: &BlockPatch,
    user_id: &str,
    body_write: BodyWrite,
) -> Result<(), StoreError> {
    // The block says which board to hold; what is patched is read under the lock, so
    // that of two patches at once the second is applied to what the first left.
    let board_id = get_block(scope, block_id).await?.board_id;
    boards::lock_board(scope, &board_id).await?;
    let mut block = get_block(scope, block_id).await?;
    let stored_update_at = block.update_at;
    // What the block was stored with before it was refused is not the patch's fault.
    block.drop_malformed_fields();
    patch
        .check_type()
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
    let named = insert_block_at(
        scope,
        &mut block,
        user_id,
        None,
        NewImages::Recorded,
        body_write,
    )
    .await?;
    card_values::check_named_cards(scope, &named).await
}

#[cfg(test)]
mod tests;
