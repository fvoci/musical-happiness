//! Where the blocks of an archive may be, checked as the store checks a write before
//! anything of the archive is stored: of a board, the types a write of blocks makes; of
//! a wiki, its pages, a tree within the organization's limits, and the comments on
//! them (`fvoci_store`'s `page_places` and `comment_places`, and `PageTree`).

use std::collections::HashMap;

use fvoci_model::{
    Block, BlockError, BlockType, BoardKind, BoardsAndBlocks, PageLimits, PageNode, PageTree,
};
use fvoci_store::StoreError;

use crate::error::AppError;

/// Checks the blocks of each board of `boards_and_blocks` against the kind of their
/// board, as they are read from an archive (each with its board, by its identifier in
/// the archive). A block that is on no board of them is left to the caller, which drops
/// what nothing leads to.
///
/// # Errors
///
/// Returns a bad request, with the store's message, for a block of a board of a type
/// a write of blocks does not make (a page among them); and for a wiki, for a block that
/// is no page or comment, a page below anything but a page of it, a comment on anything
/// but a page of it, a block whose parent the archive does not have, a block sharing its
/// identifier with another, pages that are each other's parents, and pages deeper or
/// more than `limits` let a wiki have.
pub(super) fn check(
    boards_and_blocks: &BoardsAndBlocks,
    limits: PageLimits,
) -> Result<(), AppError> {
    for board in &boards_and_blocks.boards {
        let blocks = boards_and_blocks
            .blocks
            .iter()
            .filter(|block| block.board_id == board.id);
        match board.kind {
            BoardKind::Board => check_board(blocks)?,
            BoardKind::Wiki => check_wiki(&board.id, blocks, limits)?,
        }
    }
    // What is on none of them is dropped later, unless something leads to it; of a type
    // no write makes, it is refused all the same.
    let boards: Vec<&str> = boards_and_blocks
        .boards
        .iter()
        .map(|board| board.id.as_str())
        .collect();
    check_board(
        boards_and_blocks
            .blocks
            .iter()
            .filter(|block| !boards.contains(&block.board_id.as_str())),
    )
}

/// The refusal of `block` as the store refuses it.
fn refused(block: &Block, source: BlockError) -> AppError {
    StoreError::InvalidBlock {
        id: block.id.clone(),
        source,
    }
    .into()
}

/// A board's blocks are of the types a write of blocks makes: no page.
fn check_board<'b>(blocks: impl Iterator<Item = &'b Block>) -> Result<(), AppError> {
    for block in blocks {
        if block.block_type == BlockType::Page {
            return Err(refused(block, BlockError::PageOffWiki));
        }
        if !block.block_type.is_current() {
            return Err(refused(
                block,
                BlockError::type_not_current(&block.block_type),
            ));
        }
    }
    Ok(())
}

/// A wiki's blocks are its pages, a tree within `limits`, and the comments on them.
fn check_wiki<'b>(
    wiki_id: &str,
    blocks: impl Iterator<Item = &'b Block>,
    limits: PageLimits,
) -> Result<(), AppError> {
    let blocks: Vec<&Block> = blocks.collect();
    let mut types: HashMap<&str, &BlockType> = HashMap::with_capacity(blocks.len());
    for block in &blocks {
        if types.insert(&block.id, &block.block_type).is_some() {
            return Err(AppError::bad_request(format_args!(
                "the archive's wiki {wiki_id} has two blocks of the identifier {}",
                block.id
            )));
        }
    }
    // A block directly on its wiki may name the wiki as its parent, as one of a board may.
    let parent_of = |block: &Block| -> String {
        if block.parent_id == wiki_id {
            String::new()
        } else {
            block.parent_id.clone()
        }
    };
    let mut pages = Vec::new();
    for block in &blocks {
        let parent = parent_of(block);
        let parent_type = if parent.is_empty() {
            None
        } else {
            Some(
                *types
                    .get(parent.as_str())
                    .ok_or_else(|| refused(block, BlockError::ParentNotOnBoard))?,
            )
        };
        match (&block.block_type, parent_type) {
            (BlockType::Page, None | Some(BlockType::Page)) => pages.push(PageNode {
                id: block.id.clone(),
                parent_id: parent,
                ..PageNode::default()
            }),
            (BlockType::Page, Some(_)) => {
                return Err(refused(block, BlockError::PageNotBelowPage));
            }
            (BlockType::Comment, Some(BlockType::Page)) => {}
            (BlockType::Comment, Some(BlockType::Comment)) => {
                return Err(refused(block, BlockError::ParentIsComment));
            }
            (BlockType::Comment, _) => {
                return Err(refused(block, BlockError::CommentNotOnCardOrPage));
            }
            _ => return Err(refused(block, BlockError::NotPageOnWiki)),
        }
    }
    check_tree(&blocks, pages, limits)
}

/// The pages are a tree, no deeper and no more than `limits` let a wiki be.
fn check_tree(blocks: &[&Block], pages: Vec<PageNode>, limits: PageLimits) -> Result<(), AppError> {
    let count = pages.len();
    let ids: Vec<String> = pages.iter().map(|page| page.id.clone()).collect();
    let tree: PageTree = pages.into_iter().collect();
    let block_of = |id: &str| blocks.iter().find(|block| block.id == id).copied();
    let mut deepest: Option<(&str, usize)> = None;
    for id in &ids {
        let depth = tree.depth(id);
        // A page that its own parents lead back to is counted as deep as the tree is large.
        if depth > count {
            let block = block_of(id).expect("a page of the tree is a block of the archive");
            return Err(refused(block, BlockError::ParentBelowBlock));
        }
        if deepest.is_none_or(|(_, most)| depth > most) {
            deepest = Some((id, depth));
        }
    }
    let too_many = usize::try_from(limits.pages).is_ok_and(|most| count > most);
    let too_deep =
        deepest.filter(|(_, depth)| usize::try_from(limits.depth).is_ok_and(|most| *depth > most));
    match (too_deep, ids.first()) {
        (Some((id, _)), _) => {
            let block = block_of(id).expect("a page of the tree is a block of the archive");
            Err(refused(
                block,
                BlockError::PageTooDeep {
                    limit: limits.depth,
                },
            ))
        }
        (None, Some(first)) if too_many => {
            let block = block_of(first).expect("a page of the tree is a block of the archive");
            Err(refused(
                block,
                BlockError::TooManyPages {
                    limit: limits.pages,
                },
            ))
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use fvoci_model::Board;

    use super::*;
    use crate::error::Reason;

    fn wiki_of(blocks: &[(&str, &str, BlockType)]) -> BoardsAndBlocks {
        let board = Board {
            id: "w".to_owned(),
            kind: BoardKind::Wiki,
            ..Board::default()
        };
        let blocks = blocks
            .iter()
            .map(|(id, parent, block_type)| Block {
                id: (*id).to_owned(),
                board_id: "w".to_owned(),
                parent_id: (*parent).to_owned(),
                block_type: block_type.clone(),
                ..Block::default()
            })
            .collect();
        BoardsAndBlocks {
            boards: vec![board],
            blocks,
        }
    }

    const LIMITS: PageLimits = PageLimits { depth: 2, pages: 3 };

    /// Blocks of the wiki `w`: each its identifier, its parent and its type.
    type Blocks<'a> = &'a [(&'a str, &'a str, BlockType)];

    fn reason(blocks: Blocks<'_>) -> Option<String> {
        check(&wiki_of(blocks), LIMITS)
            .err()
            .map(|error| error.to_string())
    }

    #[test]
    fn a_tree_of_pages_and_their_comments_within_the_limits_is_taken() {
        let blocks = [
            ("a", "w", BlockType::Page),
            ("b", "a", BlockType::Page),
            ("c", "", BlockType::Page),
            ("x", "b", BlockType::Comment),
        ];
        assert!(check(&wiki_of(&blocks), LIMITS).is_ok());
    }

    #[test]
    fn what_the_store_refuses_of_a_wiki_is_refused() {
        let cases: [(Blocks<'_>, &str); 7] = [
            (&[("a", "", BlockType::Card)], "pages and the comments"),
            (
                &[("a", "", BlockType::Page), ("b", "x", BlockType::Page)],
                "parent",
            ),
            (
                &[("a", "", BlockType::Comment)],
                "the parent of a comment is a card",
            ),
            (
                &[
                    ("a", "", BlockType::Page),
                    ("x", "a", BlockType::Comment),
                    ("y", "x", BlockType::Comment),
                ],
                "a comment is the parent of no block",
            ),
            (
                &[("a", "b", BlockType::Page), ("b", "a", BlockType::Page)],
                "below",
            ),
            (
                &[("a", "", BlockType::Page), ("a", "", BlockType::Page)],
                "two blocks",
            ),
            (
                &[
                    ("a", "", BlockType::Page),
                    ("x", "a", BlockType::Comment),
                    ("b", "x", BlockType::Page),
                ],
                "the parent of a page is a page",
            ),
        ];
        for (blocks, said) in cases {
            let refusal = reason(blocks).unwrap_or_default();
            assert!(refusal.contains(said), "{blocks:?}: {refusal}");
        }
    }

    #[test]
    fn a_tree_past_the_limits_is_refused_with_them() {
        let deep = [
            ("a", "", BlockType::Page),
            ("b", "a", BlockType::Page),
            ("c", "b", BlockType::Page),
        ];
        let error = check(&wiki_of(&deep), LIMITS).expect_err("too deep");
        assert_eq!(error.reason(), Some(Reason::PageTooDeep));
        let wide = [
            ("a", "", BlockType::Page),
            ("b", "", BlockType::Page),
            ("c", "", BlockType::Page),
            ("d", "", BlockType::Page),
        ];
        let error = check(&wiki_of(&wide), LIMITS).expect_err("too many");
        assert_eq!(error.reason(), Some(Reason::TooManyPages));
    }

    #[test]
    fn a_page_of_a_board_is_refused() {
        let board = BoardsAndBlocks {
            boards: vec![Board {
                id: "b".to_owned(),
                ..Board::default()
            }],
            blocks: vec![Block {
                id: "p".to_owned(),
                board_id: "b".to_owned(),
                block_type: BlockType::Page,
                ..Block::default()
            }],
        };
        let refusal = check(&board, LIMITS).expect_err("no page on a board");
        assert!(refusal.to_string().contains("not of a board"), "{refusal}");
    }
}
