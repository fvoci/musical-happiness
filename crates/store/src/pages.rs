//! The pages of a wiki: making one at its place among the pages of its parent, and the
//! tree of them all.
//!
//! A page is a block ([`BlockType::Page`]); what keeps it on a wiki and below a page is
//! checked where every block is stored (`page_places`).

use fvoci_model::{
    Block, BlockError, BlockType, NewPage, PAGE_ICON_FIELD, PAGE_ORDER_FIELD, PageNode, new_id,
    order_between, sort_pages,
};
use serde_json::Value;

use crate::db::{Id, Scope};
use crate::{Store, StoreError, blocks, boards};

impl Store {
    /// Makes a page of the wiki as `new` says, at the place it asks for among the pages
    /// of its parent, and returns its block. The wiki is held while the place is worked
    /// out, so two pages made at once get two places.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NotFound`] if there is no such wiki, and
    /// [`StoreError::InvalidBlock`] if the board is no wiki, the parent is no page of
    /// it, the page to go after is no page of the same parent, or the title or the
    /// icon may not be stored.
    pub async fn create_page(
        &self,
        wiki_id: &str,
        new: &NewPage,
        user_id: &str,
    ) -> Result<Block, StoreError> {
        self.transaction(async |scope| {
            boards::hold_board(scope, wiki_id).await?;
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
            let order = order_among_siblings(scope, &page, new.after.as_deref()).await?;
            page.fields
                .insert(PAGE_ORDER_FIELD.to_owned(), Value::from(order));
            if !new.icon.is_empty() {
                page.fields
                    .insert(PAGE_ICON_FIELD.to_owned(), Value::from(new.icon.clone()));
            }
            blocks::insert_block(scope, &mut page, user_id).await?;
            Ok(page)
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
            let mut sql = scope.sql(
                "SELECT id, parent_id, title, fields, create_at, update_at FROM blocks \
                 WHERE board_id = ",
            );
            sql.bind(Id(wiki_id))
                .push_bind(" AND type = ", BlockType::Page.as_str());
            let rows = scope.fetch_all(sql).await?;
            let mut pages = Vec::with_capacity(rows.len());
            for row in &rows {
                pages.push(PageNode::of(&Block {
                    id: row.id(0)?,
                    parent_id: row.id(1)?,
                    title: row.text(2)?,
                    fields: row.json_map(3, "fields")?,
                    create_at: row.int(4)?,
                    update_at: row.int(5)?,
                    ..Block::default()
                }));
            }
            sort_pages(&mut pages);
            Ok(pages)
        })
        .await
    }
}

/// The order that puts the new page `page` where `after` says among the pages of its
/// parent: right after the page of that identifier, first for the empty identifier, and
/// last for `None`.
async fn order_among_siblings(
    scope: &mut Scope<'_>,
    page: &Block,
    after: Option<&str>,
) -> Result<String, StoreError> {
    let mut sql = scope.sql("SELECT id, fields FROM blocks WHERE board_id = ");
    sql.bind(Id(&page.board_id))
        .push_bind(" AND type = ", BlockType::Page.as_str());
    if page.parent_id.is_empty() {
        sql.push(" AND parent_id IS NULL");
    } else {
        sql.push_bind(" AND parent_id = ", Id(&page.parent_id));
    }
    // By order and then by identifier, as the tree lists them.
    let mut siblings: Vec<(String, String)> = Vec::new();
    for row in &scope.fetch_all(sql).await? {
        let fields = row.json_map(1, "fields")?;
        let order = fields.get(PAGE_ORDER_FIELD).and_then(Value::as_str);
        siblings.push((order.unwrap_or_default().to_owned(), row.id(0)?));
    }
    siblings.sort();
    let (before, behind) = match after {
        None => (siblings.last(), None),
        Some("") => (None, siblings.first()),
        Some(after) => {
            let Some(at) = siblings.iter().position(|(_, id)| id == after) else {
                return Err(StoreError::InvalidBlock {
                    id: page.id.clone(),
                    source: BlockError::AfterNotSibling,
                });
            };
            (siblings.get(at), siblings.get(at + 1))
        }
    };
    let (before, behind) = (
        before.map(|(order, _)| order.as_str()),
        behind.map(|(order, _)| order.as_str()),
    );
    order_between(before, behind).map_err(|error| StoreError::Corrupt {
        column: "fields",
        reason: format!(
            "the orders of the pages below {:?} on {}: {error}",
            page.parent_id, page.board_id
        ),
    })
}
