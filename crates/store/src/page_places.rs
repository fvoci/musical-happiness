//! Where a block may be by its type: a page on a wiki, at its top or below a page of
//! it, and a wiki has no block but its pages and the comments on them; an entry on a
//! diary, below nothing, and a diary has no block but its entries. Every block that is
//! written is checked here by [`check`].

use fvoci_model::{Block, BlockError, BlockType, ContainerKind};

use crate::error::StoreError;

/// Checks that a block that is written is of a type its container's kind has, that a
/// page is below nothing but a page and an entry below nothing, and that what a wiki
/// has besides its pages is comments on them. The block's container is held, so the
/// parent that was found stays as it is until the block is stored. `kind` is the kind
/// of the block's container, and `parent_type` the type of the block's parent on that
/// container, `None` if it has none.
pub(crate) fn check(
    block: &Block,
    kind: ContainerKind,
    parent_type: Option<&BlockType>,
) -> Result<(), StoreError> {
    let below_page = parent_type.is_none_or(|parent| *parent == BlockType::Page);
    let refused = match (kind, &block.block_type) {
        (ContainerKind::Diary, BlockType::Entry) if parent_type.is_none() => return Ok(()),
        (ContainerKind::Diary, BlockType::Entry) => BlockError::EntryBelowBlock,
        (ContainerKind::Diary, _) => BlockError::NotEntryOnDiary,
        (ContainerKind::Board | ContainerKind::Wiki, BlockType::Entry) => BlockError::EntryOffDiary,
        (ContainerKind::Board, BlockType::Page) => BlockError::PageOffWiki,
        // Where a comment goes, on a page of the wiki, is `comment_places`' to say, as for
        // a comment of a board: its parent is what a request gets wrong.
        (ContainerKind::Board, _) | (ContainerKind::Wiki, BlockType::Comment) => return Ok(()),
        (ContainerKind::Wiki, BlockType::Page) if below_page => return Ok(()),
        (ContainerKind::Wiki, BlockType::Page) => BlockError::PageNotBelowPage,
        (ContainerKind::Wiki, _) => BlockError::NotPageOnWiki,
    };
    Err(StoreError::InvalidBlock {
        id: block.id.clone(),
        source: refused,
    })
}
