//! The trash of a wiki's pages: listing the pages deleted with what went with them, and
//! bringing one back with the pages below it.
//!
//! A page is deleted as any block is ([`crate::Store::delete_block`]): it goes to the
//! trash with every page below it, as one deletion. It comes back the same way, all of
//! the deletion together, held to the wiki's limits; where the page it was below is
//! gone (in the trash, or gone for good), it comes back at the top of the wiki, last.

use fvoci_model::{Block, BlockType, DeletedPage, PageLimits, PageNode, PageTree};

use crate::blocks::Paging;
use crate::db::{Dialect, Id, Scope, now_millis};
use crate::pages::{page_tree, write_place};
use crate::trash::{block_from_json, block_not_in_trash, push_paging, take_blocks_out_of_trash};
use crate::{Store, StoreError, boards};

/// What bringing a page back changed: the pages that came back, a parent before what is
/// below it, the page asked for first; and the pages numbered anew to leave it room.
#[derive(Debug, Clone, PartialEq)]
pub struct PageRestore {
    /// The pages brought back, as they are stored.
    pub pages: Vec<Block>,
    /// The pages numbered anew, as they are stored.
    pub renumbered: Vec<Block>,
}

impl Store {
    /// The pages of the wiki in the trash at the top of what was deleted with them, the
    /// last deleted first and then by identifier: one page of them if `paging` says so
    /// (a page size of zero is every one). A page that went with the page it was below
    /// comes back with that page, and is not listed.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the query fails.
    pub async fn get_deleted_pages(
        &self,
        wiki_id: &str,
        paging: Option<Paging>,
    ) -> Result<Vec<DeletedPage>, StoreError> {
        self.with_conn(async |scope| {
            // The type is in the stored block.
            let is_page = match scope.dialect() {
                Dialect::Sqlite => "json_extract(top.block, '$.type') = 'page'",
                Dialect::Postgres => "top.block->>'type' = 'page'",
            };
            let mut sql = scope.sql(
                "SELECT top.block, top.deleted_by, top.deleted_at, \
                 (SELECT COUNT(*) FROM deleted_blocks went \
                 WHERE went.deletion_id = top.deletion_id) \
                 FROM deleted_blocks top WHERE top.board_id = ",
            );
            sql.bind(Id(wiki_id)).push(&format!(
                " AND {is_page} AND NOT EXISTS (SELECT 1 FROM deleted_blocks above \
                 WHERE above.deletion_id = top.deletion_id AND above.id = top.parent_id) \
                 ORDER BY top.deleted_at DESC, top.id"
            ));
            push_paging(&mut sql, paging);
            let mut pages = Vec::new();
            for row in &scope.fetch_all(sql).await? {
                let block = block_from_json(row, 0)?;
                let went = u32::try_from(row.int(3)?).unwrap_or(u32::MAX);
                let node = PageNode::from(block);
                pages.push(DeletedPage {
                    id: node.id,
                    title: node.title,
                    icon: node.icon,
                    parent_id: node.parent_id,
                    deleted_at: row.int(2)?,
                    deleted_by: row.id(1)?,
                    below: went.saturating_sub(1),
                });
            }
            Ok(pages)
        })
        .await
    }

    /// Brings a page of the wiki back from the trash with every page that was deleted
    /// along with it, held to `limits`: below the page it was below, or, that page gone,
    /// last at the top of the wiki. Marks them as modified by `user_id` now.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NotFound`] if the trash of the wiki has no such page, and
    /// [`StoreError::InvalidBlock`] if the pages would be deeper, or the wiki larger,
    /// than `limits` let them.
    pub async fn undelete_page(
        &self,
        wiki_id: &str,
        page_id: &str,
        limits: PageLimits,
        user_id: &str,
    ) -> Result<PageRestore, StoreError> {
        self.transaction(async |scope| {
            // The wiki first, then the trash read under its lock: a page restored
            // meanwhile is not in it any more.
            boards::lock_board_of_kind(scope, wiki_id)
                .await?
                .ok_or_else(block_not_in_trash)?;
            let family = deletion_of(scope, wiki_id, page_id).await?;
            undelete(scope, wiki_id, page_id, family, limits, user_id).await
        })
        .await
    }
}

/// The deletion of the wiki the page went with, if it is in the wiki's trash: its
/// identifier, and the blocks of it.
async fn deletion_of(
    scope: &mut Scope<'_>,
    wiki_id: &str,
    page_id: &str,
) -> Result<(String, Vec<Block>), StoreError> {
    let mut sql = scope.sql("SELECT deletion_id, block FROM deleted_blocks WHERE id = ");
    sql.bind(Id(page_id))
        .push_bind(" AND board_id = ", Id(wiki_id));
    let row = scope
        .fetch_optional(sql)
        .await?
        .ok_or_else(block_not_in_trash)?;
    if block_from_json(&row, 1)?.block_type != BlockType::Page {
        return Err(block_not_in_trash());
    }
    let deletion_id = row.id(0)?;
    let mut sql = scope.sql("SELECT block FROM deleted_blocks WHERE deletion_id = ");
    sql.bind(Id(&deletion_id));
    let family = scope
        .fetch_all(sql)
        .await?
        .iter()
        .map(|row| block_from_json(row, 0))
        .collect::<Result<_, _>>()?;
    Ok((deletion_id, family))
}

/// Brings the deletion back, the wiki held: its pages at the top of it below their
/// parent if it is there, last at the top otherwise.
async fn undelete(
    scope: &mut Scope<'_>,
    wiki_id: &str,
    page_id: &str,
    (deletion_id, family): (String, Vec<Block>),
    limits: PageLimits,
    user_id: &str,
) -> Result<PageRestore, StoreError> {
    let invalid = |source| StoreError::InvalidBlock {
        id: page_id.to_owned(),
        source,
    };
    let tree = page_tree(scope, wiki_id).await?;
    let coming: PageTree = family
        .iter()
        .filter(|block| block.block_type == BlockType::Page)
        .cloned()
        .map(PageNode::from)
        .collect();
    let mut tops: Vec<Block> = family
        .iter()
        .filter(|block| coming.parent_of(&block.parent_id).is_none())
        .cloned()
        .collect();
    let mut renumbered = Vec::new();
    for top in &mut tops {
        let parent_gone = !top.parent_id.is_empty() && !tree.contains(&top.parent_id);
        if parent_gone {
            top.parent_id.clear();
        }
        let adding = coming.below(&top.id).len();
        tree.check_room(&top.parent_id, coming.height(&top.id), adding, limits)
            .map_err(invalid)?;
        if parent_gone {
            // Last at the top of the wiki.
            let place = tree.place("", None, "").map_err(invalid)?;
            renumbered.extend(write_place(scope, top, place).await?);
        }
    }
    let restored = take_blocks_out_of_trash(scope, &deletion_id, user_id, now_millis(), |block| {
        if let Some(top) = tops.iter().find(|top| top.id == block.id) {
            block.parent_id.clone_from(&top.parent_id);
            block.fields.clone_from(&top.fields);
        }
    })
    .await?;
    let mut pages = restored;
    // The page asked for first, then parents before what is below them.
    pages.sort_by_key(|block| (block.id != page_id, coming.depth(&block.id)));
    Ok(PageRestore { pages, renumbered })
}
