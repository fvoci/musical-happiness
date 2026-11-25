//! Blocks as a reference shows them: what an identifier names, what names a block, and
//! what there is to name, each without its body.
//!
//! Nothing here asks who reads. A caller gives the boards it has asked about, or asks
//! of each block's board what it is given: the rows of [`crate::body_links`] say what
//! a body says, of blocks its writer was never asked about.

use std::collections::HashSet;

use fvoci_model::{Block, BlockType};

use crate::Store;
use crate::blocks::Paging;
use crate::db::{Id, Row, Scope};
use crate::error::StoreError;

/// The columns of a block as a reference shows it ([`shown_block`]): without its body.
const SHOWN_COLUMNS: &str = "id, container_id, parent_id, type, title, fields, updated_at, number";

/// How many blocks a search reads at a time while it looks for titles.
const SEARCHED_AT_ONCE: u16 = 500;

/// How many identifiers one statement is asked about: well under the parameters a
/// statement of either database takes.
const ASKED_AT_ONCE: usize = 500;

/// The block of a row read with [`SHOWN_COLUMNS`]: what it is, where it is and what it
/// is called, and no body.
fn shown_block(row: &Row) -> Result<Block, StoreError> {
    Ok(Block {
        id: row.id(0)?,
        container_id: row.id(1)?,
        parent_id: row.id(2)?,
        block_type: BlockType::from(row.text(3)?),
        title: row.text(4)?,
        fields: row.json_map(5, "fields")?,
        updated_at: row.int(6)?,
        number: crate::card_numbers::number_from_row(row, 7)?,
        ..Block::default()
    })
}

/// Starts the query for the cards, pages and units on the containers of `container_ids`,
/// of which there is at least one, without their bodies. More `AND` conditions may follow.
fn select_named_on(scope: &Scope<'_>, container_ids: &[&str]) -> crate::db::Sql {
    let mut sql = scope.sql(&format!(
        "SELECT {SHOWN_COLUMNS} FROM blocks WHERE container_id IN ("
    ));
    sql.bind_list(container_ids.iter().map(|id| Id(id)))
        .push_bind(") AND type IN (", BlockType::Card.as_str())
        .push_bind(", ", BlockType::Page.as_str())
        .push_bind(", ", BlockType::Unit.as_str())
        .push(")");
    sql
}

/// Whether the title has every word of `words` in it, which are in lower case, whatever
/// the case of its letters.
///
/// Compared here rather than in SQL, as the titles of boards and the names of users
/// are: both databases fold every letter the same way, and `%` and `_` are characters
/// like any other.
fn title_has(title: &str, words: &[String]) -> bool {
    let title = title.to_lowercase();
    words.iter().all(|word| title.contains(word.as_str()))
}

impl Store {
    /// The blocks the identifiers name that are on a board now, of whatever type, each
    /// without its body and in no order: what a reference is resolved from. An
    /// identifier that names none (never one, in the trash, of another organization)
    /// has no block here, and nothing says which. Asked some at a time, as a statement
    /// binds only so many.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if a query fails.
    pub async fn named_blocks(&self, ids: &[&str]) -> Result<Vec<Block>, StoreError> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        self.with_conn(async |scope| {
            let mut found = Vec::new();
            for some in ids.chunks(ASKED_AT_ONCE) {
                let mut sql =
                    scope.sql(&format!("SELECT {SHOWN_COLUMNS} FROM blocks WHERE id IN ("));
                sql.bind_list(some.iter().map(|id| Id(id))).push(")");
                for row in scope.fetch_all(sql).await? {
                    found.push(shown_block(&row)?);
                }
            }
            Ok(found)
        })
        .await
    }

    /// Which of the identifiers name a block of the type that is on the board now (in
    /// the trash a block is not among them): a card of a board, or a page of a wiki,
    /// what a body may name to whoever reads the board or the wiki alone, by its link.
    /// Asked some at a time, as bodies may name more than one statement binds.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if a query fails.
    pub async fn blocks_on_container_among(
        &self,
        container_id: &str,
        block_type: BlockType,
        ids: &[&str],
    ) -> Result<HashSet<String>, StoreError> {
        if ids.is_empty() {
            return Ok(HashSet::new());
        }
        self.with_conn(async |scope| {
            let mut cards = HashSet::new();
            for some in ids.chunks(ASKED_AT_ONCE) {
                let mut sql = scope.sql("SELECT id FROM blocks WHERE container_id = ");
                sql.bind(Id(container_id))
                    .push_bind(" AND type = ", block_type.as_str())
                    .push(" AND id IN (")
                    .bind_list(some.iter().map(|id| Id(id)))
                    .push(")");
                for row in scope.fetch_all(sql).await? {
                    cards.insert(row.id(0)?);
                }
            }
            Ok(cards)
        })
        .await
    }

    /// One page of the cards and pages on the boards of `container_ids` whose body names
    /// the block `target_id`, or which have a comment whose body does: each once,
    /// without its body, the oldest first. The block itself is not among them, whatever
    /// its own body or its comments name.
    ///
    /// What names from a board that is not in `container_ids`, and what is in the trash,
    /// is not counted and takes no place in a page.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the query fails.
    pub async fn blocks_naming(
        &self,
        target_id: &str,
        container_ids: &[&str],
        paging: Paging,
    ) -> Result<Vec<Block>, StoreError> {
        if container_ids.is_empty() {
            return Ok(Vec::new());
        }
        self.with_conn(async |scope| {
            let mut sql = select_named_on(scope, container_ids);
            // What a comment names, the card or the page it is on names. The rows of
            // the boards asked about are the only ones looked at; what is found is held
            // to the boards its block is on now all the same.
            sql.push_bind(" AND id <> ", Id(target_id))
                .push_bind(
                    " AND id IN (SELECT CASE WHEN s.type = ",
                    BlockType::Comment.as_str(),
                )
                .push(
                    " THEN s.parent_id ELSE s.id END \
                     FROM body_links l JOIN blocks s ON s.id = l.source_id WHERE l.target_id = ",
                )
                .bind(Id(target_id))
                .push(" AND l.container_id IN (")
                .bind_list(container_ids.iter().map(|id| Id(id)))
                .push(")) ORDER BY created_at, id");
            if paging.per_page > 0 {
                let offset = paging.offset();
                sql.push_bind(" LIMIT ", i64::from(paging.per_page))
                    .push_bind(" OFFSET ", offset);
            }
            scope
                .fetch_all(sql)
                .await?
                .iter()
                .map(shown_block)
                .collect()
        })
        .await
    }

    /// The cards and pages on the boards of `container_ids` whose title has every word of
    /// `term` in it, as spaces divide it and whatever the case of the letters: at most
    /// `limit`, the one changed last first, without their bodies. An empty term finds
    /// every one. A block whose `isTemplate` is `true` ([`Block::is_template`]: for a
    /// card, a template of its board's cards) is never found, and nothing in the trash
    /// is.
    ///
    /// The blocks are read some at a time, each time those after the last one read in
    /// that order: a block that is changed meanwhile moves to before where the reading
    /// is, so none is found twice.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if a query fails.
    pub async fn titled_blocks(
        &self,
        container_ids: &[&str],
        term: &str,
        limit: usize,
    ) -> Result<Vec<Block>, StoreError> {
        if container_ids.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }
        let words: Vec<String> = term.split_whitespace().map(str::to_lowercase).collect();
        self.with_conn(async |scope| {
            let mut found = Vec::new();
            let mut last: Option<(i64, String)> = None;
            loop {
                let mut sql = select_named_on(scope, container_ids);
                if let Some((updated_at, id)) = &last {
                    sql.push_bind(" AND (updated_at < ", *updated_at)
                        .push_bind(" OR (updated_at = ", *updated_at)
                        .push_bind(" AND id > ", Id(id))
                        .push("))");
                }
                sql.push(" ORDER BY updated_at DESC, id")
                    .push_bind(" LIMIT ", i64::from(SEARCHED_AT_ONCE));
                let rows = scope.fetch_all(sql).await?;
                for row in &rows {
                    let block = shown_block(row)?;
                    last = Some((block.updated_at, block.id.clone()));
                    if !block.is_template() && title_has(&block.title, &words) {
                        found.push(block);
                        if found.len() == limit {
                            return Ok(found);
                        }
                    }
                }
                if rows.len() < usize::from(SEARCHED_AT_ONCE) {
                    return Ok(found);
                }
            }
        })
        .await
    }
}
