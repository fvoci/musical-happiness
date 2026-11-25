//! Where a comment may be: on a card of its board, a page of its wiki, or a unit of
//! its course, with nothing below it (contract B1; [`BlockType::takes_comments`]).
//! Every block that is written is checked here by [`check`].

use fvoci_model::{Block, BlockError, BlockType};

use crate::db::{Id, Scope};
use crate::error::StoreError;

/// The type of the board's block with the identifier, if the board has one.
pub(crate) async fn type_on_container(
    scope: &mut Scope<'_>,
    block_id: &str,
    container_id: &str,
) -> Result<Option<BlockType>, StoreError> {
    let mut sql = scope.sql("SELECT type FROM blocks WHERE id = ");
    sql.bind(Id(block_id))
        .push_bind(" AND container_id = ", Id(container_id));
    match scope.fetch_optional(sql).await? {
        Some(row) => Ok(Some(crate::blocks::block_type_column(&row, 0, "type")?)),
        None => Ok(None),
    }
}

/// The types of the blocks directly below a block.
async fn types_below(scope: &mut Scope<'_>, block_id: &str) -> Result<Vec<BlockType>, StoreError> {
    let mut sql = scope.sql("SELECT DISTINCT type FROM blocks WHERE parent_id = ");
    sql.bind(Id(block_id));
    scope
        .fetch_all(sql)
        .await?
        .iter()
        .map(|row| crate::blocks::block_type_column(row, 0, "type"))
        .collect()
}

/// Checks that a block that is written keeps comments where they belong: a comment is
/// on a card or a page of its board and has nothing below it. What is checked is what the write
/// changes, so a block stored before the rule is still changed in other ways: a new
/// block's parent, a moved block's new parent, and a type that changes. `parent_type`
/// is the type of the block's parent, `None` if it has none.
pub(crate) async fn check(
    scope: &mut Scope<'_>,
    block: &Block,
    parent_type: Option<&BlockType>,
    existing: Option<&Block>,
) -> Result<(), StoreError> {
    let refuse = |source| {
        Err(StoreError::InvalidBlock {
            id: block.id.clone(),
            source,
        })
    };
    let moves = existing.is_none_or(|existing| existing.parent_id != block.parent_id);
    let old_type = existing.map(|existing| &existing.block_type);
    let is_comment = block.block_type == BlockType::Comment;
    let becomes_comment = is_comment && old_type != Some(&BlockType::Comment);
    let on_its_place = parent_type.copied().is_some_and(BlockType::takes_comments);
    if is_comment && (moves || becomes_comment) && !on_its_place {
        // Where only the type changed, the type is what the request got wrong.
        return refuse(if moves {
            BlockError::CommentNotOnCardOrPage
        } else {
            BlockError::TypeStrandsComments
        });
    }
    if moves && parent_type == Some(&BlockType::Comment) {
        return refuse(BlockError::ParentIsComment);
    }
    let Some(old_type) = old_type.filter(|old_type| **old_type != block.block_type) else {
        return Ok(());
    };
    let below = types_below(scope, &block.id).await?;
    let strands = (is_comment && !below.is_empty())
        || (old_type.takes_comments() && below.contains(&BlockType::Comment));
    if strands {
        return refuse(BlockError::TypeStrandsComments);
    }
    Ok(())
}

/// What a block that comes back from the trash onto a parent of `parent_type` (`None`
/// for none) would break, if anything: the parent may have changed type since the
/// block was deleted, and a block stored before the rule may never have kept it.
pub(crate) fn restore_refusal(
    block_type: BlockType,
    parent_type: Option<BlockType>,
) -> Option<BlockError> {
    if block_type == BlockType::Comment && !parent_type.is_some_and(BlockType::takes_comments) {
        Some(BlockError::CommentNotOnCardOrPage)
    } else if parent_type == Some(BlockType::Comment) {
        Some(BlockError::ParentIsComment)
    } else {
        None
    }
}
