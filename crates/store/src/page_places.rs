//! Where a page may be: on a wiki, at its top or below a page of it; and a wiki has no
//! block that is no page. Every block that is written is checked here by [`check`].

use fvoci_model::{Block, BlockError, BlockType, BoardKind};

use crate::board_kinds;
use crate::db::Scope;
use crate::error::StoreError;

/// Checks that a block that is written is of a type its board's kind has, and that a
/// page is below nothing but a page. The block's board is held, so its kind and the
/// parent that was found stay as they are until the block is stored. `parent_type` is
/// the type of the block's parent on its board, `None` if it has none.
pub(crate) async fn check(
    scope: &mut Scope<'_>,
    block: &Block,
    parent_type: Option<&BlockType>,
) -> Result<(), StoreError> {
    let is_page = block.block_type == BlockType::Page;
    let on_wiki = board_kinds::kind_of(scope, &block.board_id).await? == Some(BoardKind::Wiki);
    let refused = match (on_wiki, is_page) {
        (false, false) => return Ok(()),
        (false, true) => BlockError::PageOffWiki,
        (true, false) => BlockError::NotPageOnWiki,
        (true, true) if parent_type.is_none_or(|parent| *parent == BlockType::Page) => {
            return Ok(());
        }
        (true, true) => BlockError::PageNotBelowPage,
    };
    Err(StoreError::InvalidBlock {
        id: block.id.clone(),
        source: refused,
    })
}
