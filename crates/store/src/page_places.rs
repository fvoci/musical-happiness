//! Where a page may be: on a wiki, at its top or below a page of it; and a wiki has no
//! block that is no page. Every block that is written is checked here by [`check`].

use fvoci_model::{Block, BlockError, BlockType, BoardKind};

use crate::error::StoreError;

/// Checks that a block that is written is of a type its board's kind has, and that a
/// page is below nothing but a page. The block's board is held, so the parent that
/// was found stays as it is until the block is stored. `kind` is the kind of the
/// block's board, and `parent_type` the type of the block's parent on that board,
/// `None` if it has none.
pub(crate) fn check(
    block: &Block,
    kind: BoardKind,
    parent_type: Option<&BlockType>,
) -> Result<(), StoreError> {
    let is_page = block.block_type == BlockType::Page;
    let refused = match (kind, is_page) {
        (BoardKind::Board, false) => return Ok(()),
        (BoardKind::Board, true) => BlockError::PageOffWiki,
        (BoardKind::Wiki, false) => BlockError::NotPageOnWiki,
        (BoardKind::Wiki, true) if parent_type.is_none_or(|parent| *parent == BlockType::Page) => {
            return Ok(());
        }
        (BoardKind::Wiki, true) => BlockError::PageNotBelowPage,
    };
    Err(StoreError::InvalidBlock {
        id: block.id.clone(),
        source: refused,
    })
}
