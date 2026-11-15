//! Duplicating a page of a wiki with every page below it, within the wiki.
//!
//! The copies are new pages: new identifiers, made by whoever duplicates them, now. Each
//! keeps its title (the copy of the page asked for takes the title it is given), its
//! icon, its place among the pages copied with it and its body; the versions of the
//! bodies and the comments are not copied. The images of the bodies still show the
//! original's files: the caller gives the copies files of their own afterwards, as a
//! card's duplicate does.

use std::collections::HashSet;

use fvoci_model::{Block, PageLimits, generate_block_ids};

use crate::blocks::{fetch_blocks, insert_blocks, select_blocks};
use crate::db::{Id, Scope};
use crate::file_refs::NewImages;
use crate::pages::{page_tree, write_place};
use crate::{Store, StoreError, boards};

/// What duplicating a page made: the copies, the copy of the page asked for first and
/// a parent before what is below it; and the pages numbered anew to leave it room.
#[derive(Debug, Clone, PartialEq)]
pub struct PageCopies {
    /// The copies, as they are stored.
    pub pages: Vec<Block>,
    /// The siblings numbered anew, as they are stored.
    pub renumbered: Vec<Block>,
}

impl Store {
    /// Copies a page of the wiki with every page below it, the copy right after the page
    /// among its siblings, titled `title` if given (the page's own title otherwise),
    /// held to `limits`. The wiki is held from before the tree is read.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NotFound`] if the wiki has no such page, and
    /// [`StoreError::InvalidBlock`] if the copies would be deeper, or the wiki larger,
    /// than `limits` let them, or the title may not be stored.
    pub async fn duplicate_page(
        &self,
        wiki_id: &str,
        page_id: &str,
        title: Option<&str>,
        limits: PageLimits,
        user_id: &str,
    ) -> Result<PageCopies, StoreError> {
        let not_found = || StoreError::not_found(format_args!("page ID={page_id} of {wiki_id}"));
        let invalid = |source| StoreError::InvalidBlock {
            id: page_id.to_owned(),
            source,
        };
        self.transaction(async |scope| {
            boards::lock_board_of_kind(scope, wiki_id)
                .await?
                .ok_or_else(not_found)?;
            let tree = page_tree(scope, wiki_id).await?;
            let parent_id = tree.parent_of(page_id).ok_or_else(not_found)?.to_owned();
            let family: Vec<String> = tree.below(page_id).into_iter().map(str::to_owned).collect();
            tree.check_room(&parent_id, tree.height(page_id), family.len(), limits)
                .map_err(invalid)?;
            let mut copies = family_of(scope, wiki_id, page_id, &family).await?;
            if let Some(title) = title {
                title.clone_into(&mut copies[0].title);
            }
            copies[0].validate().map_err(invalid)?;
            let place = tree.place(&parent_id, Some(page_id), "").map_err(invalid)?;
            let renumbered = write_place(scope, &mut copies[0], place).await?;
            // New identifiers, and each copy below the copy of its parent.
            generate_block_ids(&mut copies);
            insert_blocks(scope, &mut copies, user_id, NewImages::Unchecked).await?;
            Ok(PageCopies {
                pages: copies,
                renumbered,
            })
        })
        .await
    }
}

/// The page and the pages below it as they are stored, the page first and then in the
/// order of `family` (a parent before what is below it), each made anew: no times, no
/// one who made or changed it.
async fn family_of(
    scope: &mut Scope<'_>,
    wiki_id: &str,
    page_id: &str,
    family: &[String],
) -> Result<Vec<Block>, StoreError> {
    let mut sql = select_blocks(scope);
    sql.push_bind(" WHERE board_id = ", Id(wiki_id))
        .push(" AND type = 'page' AND id IN (")
        .bind_list(family.iter().map(|id| Id(id)))
        .push(")");
    let mut found = fetch_blocks(scope, sql).await?;
    let wanted: HashSet<&str> = family.iter().map(String::as_str).collect();
    found.retain(|block| wanted.contains(block.id.as_str()));
    found.sort_by_key(|block| family.iter().position(|id| *id == block.id));
    if found.first().is_none_or(|block| block.id != page_id) {
        return Err(StoreError::not_found(format_args!("page ID={page_id}")));
    }
    for block in &mut found {
        block.created_by.clear();
        block.modified_by.clear();
        block.create_at = 0;
        block.update_at = 0;
    }
    Ok(found)
}
