//! Where a page may be: on a wiki, at its top or below a page of it; and a wiki has no
//! block but its pages and the comments on them. Every block that is written is checked
//! here by [`check`].

use fvoci_model::{Block, BlockError, BlockType, BoardKind};

use crate::error::StoreError;

/// Checks that a block that is written is of a type its board's kind has, that a page
/// is below nothing but a page, and that what a wiki has besides its pages is comments
/// on them. The block's board is held, so the parent that
/// was found stays as it is until the block is stored. `kind` is the kind of the
/// block's board, and `parent_type` the type of the block's parent on that board,
/// `None` if it has none.
pub(crate) fn check(
    block: &Block,
    kind: BoardKind,
    parent_type: Option<&BlockType>,
) -> Result<(), StoreError> {
    let below_page = parent_type.is_none_or(|parent| *parent == BlockType::Page);
    let refused = match (kind, &block.block_type) {
        (BoardKind::Board, BlockType::Page) => BlockError::PageOffWiki,
        (BoardKind::Board, _) => return Ok(()),
        (BoardKind::Wiki, BlockType::Page) if below_page => return Ok(()),
        (BoardKind::Wiki, BlockType::Page) => BlockError::PageNotBelowPage,
        (BoardKind::Wiki, BlockType::Comment) if parent_type == Some(&BlockType::Page) => {
            return Ok(());
        }
        (BoardKind::Wiki, _) => BlockError::NotPageOnWiki,
    };
    Err(StoreError::InvalidBlock {
        id: block.id.clone(),
        source: refused,
    })
}
