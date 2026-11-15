//! The pages of a wiki: making one at its place among the pages of its parent, moving
//! one with the pages below it, and the tree of them all.
//!
//! A page is a block ([`BlockType::Page`]); what keeps it on a wiki and below a page is
//! checked where every block is stored (`page_places`). Where a page may go, how deep
//! and how many the pages are, and the place it takes are decided over the whole tree
//! read under the wiki's lock ([`PageTree`]).

use fvoci_model::{
    Block, BlockError, BlockType, NewPage, PAGE_ICON_FIELD, PAGE_ORDER_FIELD, PageLimits, PageMove,
    PageNode, PageTree, Place, new_id, sort_pages,
};
use serde_json::Value;

use crate::db::{Id, Scope};
use crate::{Store, StoreError, blocks, boards, comment_places, page_places};

/// What a write of the tree changed: the page it was about, and the siblings whose
/// orders were made anew to leave it room.
#[derive(Debug, Clone, PartialEq)]
pub struct PageWrite {
    /// The page made or moved, as it is stored.
    pub page: Block,
    /// The pages numbered anew, as they are stored, in the order they are listed in.
    pub renumbered: Vec<Block>,
}

impl Store {
    /// Makes a page of the wiki as `new` says, at the place it asks for among the pages
    /// of its parent, and returns it with the siblings numbered anew for it. The wiki is
    /// held while the place is worked out, so two pages made at once get two places.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NotFound`] if there is no such wiki, and
    /// [`StoreError::InvalidBlock`] if the board is no wiki, the parent is no page of
    /// it, the page would be deeper or the wiki larger than `limits` let them, the page
    /// to go after is no page of the same parent, or the title or the icon may not be
    /// stored.
    pub async fn create_page(
        &self,
        wiki_id: &str,
        new: &NewPage,
        limits: PageLimits,
        user_id: &str,
    ) -> Result<PageWrite, StoreError> {
        self.transaction(async |scope| {
            let Some(kind) = boards::lock_board_of_kind(scope, wiki_id).await? else {
                return Err(StoreError::not_found(format_args!("board ID={wiki_id}")));
            };
            let mut page = Block {
                id: new_id(),
                board_id: wiki_id.to_owned(),
                block_type: BlockType::Page,
                title: new.title.clone(),
                ..Block::default()
            };
            // A page at the top has no parent; a client may name the wiki there.
            if new.parent_id != wiki_id {
                page.parent_id.clone_from(&new.parent_id);
            }
            if !new.icon.is_empty() {
                page.fields
                    .insert(PAGE_ICON_FIELD.to_owned(), Value::from(new.icon.clone()));
            }
            // What the page says of itself and where it goes below are refused before
            // its place among its siblings is: of a request that is wrong in both, the
            // parent is named, whose pages the place was to be among.
            let invalid = |source| StoreError::InvalidBlock {
                id: page.id.clone(),
                source,
            };
            page.validate().map_err(invalid)?;
            let parent_type = if page.parent_id.is_empty() {
                None
            } else {
                let found = comment_places::type_on_board(scope, &page.parent_id, wiki_id).await?;
                Some(found.ok_or_else(|| invalid(BlockError::ParentNotOnBoard))?)
            };
            page_places::check(&page, kind, parent_type.as_ref())?;
            let tree = page_tree(scope, wiki_id).await?;
            tree.check_room(&page.parent_id, 1, 1, limits)
                .map_err(invalid)?;
            let place = tree
                .place(&page.parent_id, new.after.as_deref(), "")
                .map_err(invalid)?;
            let renumbered = write_place(scope, &mut page, place, user_id).await?;
            blocks::insert_block(scope, &mut page, user_id).await?;
            Ok(PageWrite { page, renumbered })
        })
        .await
    }

    /// Moves a page of the wiki, with every page below it, below the parent `to` says
    /// (a page of the same wiki, or the top) at the place it asks for, and returns it
    /// with the siblings numbered anew for it. The wiki is held from before the tree is
    /// read, so of two moves at once the second is decided on what the first left.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NotFound`] if the wiki has no such page, and
    /// [`StoreError::InvalidBlock`] if the parent is no page of the wiki, is the page
    /// itself or a page below it, or would leave a page deeper than `limits` let it be,
    /// or if the page to go after is no other page of that parent.
    pub async fn move_page(
        &self,
        wiki_id: &str,
        page_id: &str,
        to: &PageMove,
        limits: PageLimits,
        user_id: &str,
    ) -> Result<PageWrite, StoreError> {
        self.transaction(async |scope| {
            let not_found = || StoreError::not_found(format_args!("page ID={page_id}"));
            boards::lock_board_of_kind(scope, wiki_id)
                .await?
                .ok_or_else(not_found)?;
            let mut page = blocks::find_block(scope, page_id)
                .await?
                .filter(|block| block.board_id == wiki_id && block.block_type == BlockType::Page)
                .ok_or_else(not_found)?;
            let invalid = |source| StoreError::InvalidBlock {
                id: page_id.to_owned(),
                source,
            };
            // A page at the top has no parent; a client may name the wiki there.
            let parent_id = if to.parent_id == wiki_id {
                ""
            } else {
                to.parent_id.as_str()
            };
            let tree = page_tree(scope, wiki_id).await?;
            tree.check_move(page_id, parent_id, limits)
                .map_err(invalid)?;
            let place = tree
                .place(parent_id, to.after.as_deref(), page_id)
                .map_err(invalid)?;
            parent_id.clone_into(&mut page.parent_id);
            let renumbered = write_place(scope, &mut page, place, user_id).await?;
            blocks::insert_block(scope, &mut page, user_id).await?;
            Ok(PageWrite { page, renumbered })
        })
        .await
    }

    /// The pages of the wiki without their bodies, in the order of
    /// [`fvoci_model::sort_pages`]: each names its parent, which is how they are a
    /// tree. A board that is no wiki has none.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the query fails.
    pub async fn get_page_tree(&self, wiki_id: &str) -> Result<Vec<PageNode>, StoreError> {
        self.with_conn(async |scope| {
            let mut pages = page_nodes(scope, wiki_id).await?;
            sort_pages(&mut pages);
            Ok(pages)
        })
        .await
    }
}

/// The pages of the wiki without their bodies, in no order.
pub(crate) async fn page_nodes(
    scope: &mut Scope<'_>,
    wiki_id: &str,
) -> Result<Vec<PageNode>, StoreError> {
    let mut sql = scope.sql(
        "SELECT id, parent_id, title, fields, create_at, update_at FROM blocks \
         WHERE board_id = ",
    );
    sql.bind(Id(wiki_id))
        .push_bind(" AND type = ", BlockType::Page.as_str());
    let rows = scope.fetch_all(sql).await?;
    let mut pages = Vec::with_capacity(rows.len());
    for row in &rows {
        pages.push(PageNode::from(Block {
            id: row.id(0)?,
            parent_id: row.id(1)?,
            title: row.text(2)?,
            fields: row.json_map(3, "fields")?,
            create_at: row.int(4)?,
            update_at: row.int(5)?,
            ..Block::default()
        }));
    }
    Ok(pages)
}

/// The tree of the wiki's pages, which the wiki is held for.
pub(crate) async fn page_tree(
    scope: &mut Scope<'_>,
    wiki_id: &str,
) -> Result<PageTree, StoreError> {
    Ok(page_nodes(scope, wiki_id).await?.into_iter().collect())
}

/// Gives `page` the order of `place`, and stores the orders of the siblings it numbers
/// anew, each as a change of its block by `user_id`. Returns those siblings as stored.
pub(crate) async fn write_place(
    scope: &mut Scope<'_>,
    page: &mut Block,
    place: Place,
    user_id: &str,
) -> Result<Vec<Block>, StoreError> {
    page.fields
        .insert(PAGE_ORDER_FIELD.to_owned(), Value::from(place.order));
    let mut renumbered = Vec::with_capacity(place.renumbered.len());
    for (sibling_id, order) in place.renumbered {
        let mut sibling = blocks::get_block(scope, &sibling_id).await?;
        sibling
            .fields
            .insert(PAGE_ORDER_FIELD.to_owned(), Value::from(order));
        blocks::insert_block(scope, &mut sibling, user_id).await?;
        renumbered.push(sibling);
    }
    Ok(renumbered)
}
