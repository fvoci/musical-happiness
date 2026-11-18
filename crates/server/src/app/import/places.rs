//! Where the blocks of an archive may be, checked as the store checks a write before
//! anything of the archive is stored: a block's parent on its own board, a comment on a
//! card or a page with nothing below it (`fvoci_store`'s `comment_places`), of a board
//! the types a write of blocks makes, of a wiki its pages and the comments on them, the
//! pages a tree within the organization's limits (`page_places`, [`PageTree`]). Each
//! check takes time in proportion to the blocks, whatever their shape.

use std::collections::{HashMap, HashSet};

use fvoci_model::{
    Block, BlockError, BlockType, ContainerKind, ContainersAndBlocks, PageLimits, PageNode,
    PageTree,
};
use fvoci_store::StoreError;

use crate::app::pages::refused_without_member;
use crate::error::AppError;

/// Checks the blocks of each board of `containers_and_blocks` against the kind of their
/// board and against each other, as they are read from an archive (each with its board
/// and its parent by their identifiers in the archive). A block of a board whose parent
/// the archive does not have is left to the caller, which drops what nothing leads to;
/// of a wiki, it is refused. A block that is on no board of them is held to a board's
/// types.
///
/// # Errors
///
/// Returns a bad request, with the store's message, for a block of a type its board's
/// kind does not take (a page of a board, anything but a page or a comment of a wiki, a
/// type no write makes), a parent on another board, a comment on anything but a card or
/// a page, a block below a comment, a page below anything but a page; and for a wiki, a
/// parent the archive does not have, two blocks of one identifier, pages that are each
/// other's parents, and pages more or deeper than `limits` let a wiki have.
pub(super) fn check(
    containers_and_blocks: &ContainersAndBlocks,
    limits: PageLimits,
) -> Result<(), AppError> {
    let ContainersAndBlocks { containers, blocks } = containers_and_blocks;
    // The first block of an identifier is the one named, as the copy names it.
    let mut placed: HashMap<&str, &Block> = HashMap::with_capacity(blocks.len());
    let mut on_container: HashMap<&str, Vec<&Block>> = HashMap::with_capacity(containers.len());
    for block in blocks {
        placed.entry(&block.id).or_insert(block);
        on_container
            .entry(&block.container_id)
            .or_default()
            .push(block);
    }
    let container_ids: HashSet<&str> = containers
        .iter()
        .map(|container| container.id.as_str())
        .collect();
    for container in containers {
        let mine = on_container
            .remove(container.id.as_str())
            .unwrap_or_default();
        let place = Place {
            container_id: &container.id,
            kind: container.kind,
            placed: &placed,
            containers: &container_ids,
        };
        for block in &mine {
            place.check(block)?;
        }
        if container.kind == ContainerKind::Wiki {
            check_tree(&container.id, &mine, limits)?;
        }
    }
    // What is on none of the boards is dropped later, unless something leads to it; of a
    // type no write makes, it is refused all the same.
    for block in on_container.into_values().flatten() {
        check_type(block, ContainerKind::Board)?;
    }
    Ok(())
}

/// The refusal of `block` as the store refuses it.
fn refused(block: &Block, source: BlockError) -> AppError {
    StoreError::InvalidBlock {
        id: block.id.clone(),
        source,
    }
    .into()
}

/// The board the blocks are checked on, and the rest of the archive they are checked
/// against.
struct Place<'a> {
    container_id: &'a str,
    kind: ContainerKind,
    /// Every block of the archive by its identifier: the first of an identifier.
    placed: &'a HashMap<&'a str, &'a Block>,
    /// The boards of the archive.
    containers: &'a HashSet<&'a str>,
}

impl Place<'_> {
    /// Checks one block of the board: its type, and its parent as a write would find it.
    fn check(&self, block: &Block) -> Result<(), AppError> {
        check_type(block, self.kind)?;
        // A block directly on its board has no parent; the board is none, and is refused
        // as another board of the archive is.
        let parent_id = block.parent_id.as_str();
        let parent_type = if parent_id.is_empty() {
            None
        } else if let Some(parent) = self.placed.get(parent_id) {
            if parent.container_id != self.container_id {
                return Err(refused(block, BlockError::ParentNotOnContainer));
            }
            Some(&parent.block_type)
        } else if self.containers.contains(parent_id) || self.kind == ContainerKind::Wiki {
            // Another board of the archive, or what a wiki's tree does not have.
            return Err(refused(block, BlockError::ParentNotOnContainer));
        } else {
            // What nothing leads to is dropped: no write is asked of it.
            return Ok(());
        };
        // In the store's order: where a page may be, then where a comment may be.
        let refusal = match (&block.block_type, parent_type) {
            (BlockType::Page, Some(parent)) if *parent != BlockType::Page => {
                Some(BlockError::PageNotBelowPage)
            }
            (BlockType::Comment, parent) if !parent.is_some_and(BlockType::takes_comments) => {
                Some(BlockError::CommentNotOnCardOrPage)
            }
            (_, Some(BlockType::Comment)) => Some(BlockError::ParentIsComment),
            _ => None,
        };
        refusal.map_or(Ok(()), |source| Err(refused(block, source)))
    }
}

/// Checks that a block is of a type a board of `kind` takes: of a board, what a write of
/// blocks makes and no page; of a wiki, a page or a comment.
fn check_type(block: &Block, kind: ContainerKind) -> Result<(), AppError> {
    let refusal = match (kind, &block.block_type) {
        (ContainerKind::Board, BlockType::Page) => Some(BlockError::PageOffWiki),
        (ContainerKind::Board, block_type) if !block_type.is_current() => {
            Some(BlockError::type_not_current(block_type))
        }
        (ContainerKind::Wiki, BlockType::Page | BlockType::Comment) | (ContainerKind::Board, _) => {
            None
        }
        (ContainerKind::Wiki, _) => Some(BlockError::NotPageOnWiki),
    };
    refusal.map_or(Ok(()), |source| Err(refused(block, source)))
}

/// Checks that the pages of the wiki are a tree within `limits`: each identifier once,
/// each page reached from the top, no more and no deeper pages than a wiki may have. The
/// parents are those [`Place::check`] took: a page of the wiki, or none.
fn check_tree(wiki_id: &str, blocks: &[&Block], limits: PageLimits) -> Result<(), AppError> {
    let pages: Vec<&Block> = blocks
        .iter()
        .copied()
        .filter(|block| block.block_type == BlockType::Page)
        .collect();
    // Of the whole wiki: as a copy of it is refused, naming no member of a request.
    let refused_whole = |source| {
        refused_without_member(StoreError::InvalidBlock {
            id: wiki_id.to_owned(),
            source,
        })
    };
    // Counted before anything is walked: an archive of many pages is refused at once.
    if usize::try_from(limits.pages).is_ok_and(|most| pages.len() > most) {
        return Err(refused_whole(BlockError::TooManyPages {
            limit: limits.pages,
        }));
    }
    let mut ids = HashSet::with_capacity(blocks.len());
    if let Some(twice) = blocks.iter().find(|block| !ids.insert(block.id.as_str())) {
        return Err(AppError::bad_request(format_args!(
            "the archive's wiki {wiki_id} has two blocks of the identifier {}",
            twice.id
        )));
    }
    let tree: PageTree = pages
        .iter()
        .map(|page| PageNode {
            id: page.id.clone(),
            parent_id: page.parent_id.clone(),
            ..PageNode::default()
        })
        .collect();
    // A page that the top does not lead to is below itself, or below one that is.
    let mut reached: HashSet<&str> = HashSet::with_capacity(tree.len());
    for top in tree.siblings("", "") {
        reached.extend(tree.below(&top.id));
    }
    if let Some(looped) = pages
        .iter()
        .find(|page| !reached.contains(page.id.as_str()))
    {
        return Err(refused(looped, BlockError::ParentBelowBlock));
    }
    tree.check_whole(limits).map_err(refused_whole)
}

#[cfg(test)]
mod tests {
    use fvoci_model::Container;

    use super::*;
    use crate::error::Reason;

    /// Blocks of one board: each its identifier, its parent and its type.
    type Blocks<'a> = &'a [(&'a str, &'a str, BlockType)];

    fn of(container_id: &str, kind: ContainerKind, blocks: Blocks<'_>) -> ContainersAndBlocks {
        let container = Container {
            id: container_id.to_owned(),
            kind,
            ..Container::default()
        };
        let blocks = blocks
            .iter()
            .map(|(id, parent, block_type)| Block {
                id: (*id).to_owned(),
                container_id: container_id.to_owned(),
                parent_id: (*parent).to_owned(),
                block_type: block_type.clone(),
                ..Block::default()
            })
            .collect();
        ContainersAndBlocks {
            containers: vec![container],
            blocks,
        }
    }

    const LIMITS: PageLimits = PageLimits { depth: 2, pages: 3 };

    fn refusal(kind: ContainerKind, blocks: Blocks<'_>) -> String {
        check(&of("w", kind, blocks), LIMITS)
            .err()
            .map(|error| error.to_string())
            .unwrap_or_default()
    }

    #[test]
    fn a_tree_of_pages_and_their_comments_within_the_limits_is_taken() {
        let blocks = [
            ("a", "", BlockType::Page),
            ("b", "a", BlockType::Page),
            ("c", "", BlockType::Page),
            ("x", "b", BlockType::Comment),
        ];
        assert!(check(&of("w", ContainerKind::Wiki, &blocks), LIMITS).is_ok());
    }

    #[test]
    fn what_the_store_refuses_of_a_wiki_is_refused_with_its_message() {
        let page = || BlockType::Page;
        let comment = || BlockType::Comment;
        let cases: [(Blocks<'_>, BlockError); 8] = [
            (&[("a", "", BlockType::Card)], BlockError::NotPageOnWiki),
            (&[("a", "w", page())], BlockError::ParentNotOnContainer),
            (
                &[("a", "", page()), ("b", "x", page())],
                BlockError::ParentNotOnContainer,
            ),
            (&[("a", "", comment())], BlockError::CommentNotOnCardOrPage),
            (
                &[
                    ("a", "", page()),
                    ("x", "a", comment()),
                    ("y", "x", comment()),
                ],
                BlockError::CommentNotOnCardOrPage,
            ),
            (
                &[("a", "b", page()), ("b", "a", page())],
                BlockError::ParentBelowBlock,
            ),
            (&[("a", "a", page())], BlockError::ParentBelowBlock),
            (
                &[("a", "", page()), ("x", "a", comment()), ("b", "x", page())],
                BlockError::PageNotBelowPage,
            ),
        ];
        for (blocks, said) in cases {
            let refused = refusal(ContainerKind::Wiki, blocks);
            assert!(refused.contains(&said.to_string()), "{blocks:?}: {refused}");
        }
        let twice = refusal(ContainerKind::Wiki, &[("a", "", page()), ("a", "", page())]);
        assert!(twice.contains("two blocks"), "{twice}");
    }

    #[test]
    fn what_the_store_refuses_of_a_board_is_refused_and_what_nothing_leads_to_is_left() {
        let cases: [(Blocks<'_>, BlockError); 5] = [
            (&[("p", "", BlockType::Page)], BlockError::PageOffWiki),
            (
                &[("c", "w", BlockType::Card)],
                BlockError::ParentNotOnContainer,
            ),
            (
                &[("x", "", BlockType::Comment)],
                BlockError::CommentNotOnCardOrPage,
            ),
            (
                &[
                    ("c", "", BlockType::Card),
                    ("x", "c", BlockType::Comment),
                    ("v", "x", BlockType::View),
                ],
                BlockError::ParentIsComment,
            ),
            (
                &[("c", "other", BlockType::Card)],
                BlockError::ParentNotOnContainer,
            ),
        ];
        for (blocks, said) in cases {
            let mut archive = of("w", ContainerKind::Board, blocks);
            archive.containers.push(Container {
                id: "other".to_owned(),
                ..Container::default()
            });
            let refused = check(&archive, LIMITS).err().map(|error| error.to_string());
            let refused = refused.unwrap_or_default();
            assert!(refused.contains(&said.to_string()), "{blocks:?}: {refused}");
        }
        let card = BlockType::Card;
        let dropped_later = [("c", "gone", card.clone()), ("x", "c", BlockType::Comment)];
        assert!(check(&of("w", ContainerKind::Board, &dropped_later), LIMITS).is_ok());
        // A parent on another board of the same file.
        let mut two = of("w", ContainerKind::Board, &[("c", "", card.clone())]);
        let other = of("o", ContainerKind::Board, &[("d", "c", card)]);
        two.containers.extend(other.containers);
        two.blocks.extend(other.blocks);
        let refused = check(&two, LIMITS).expect_err("a parent of another board");
        assert!(
            refused
                .to_string()
                .contains(&BlockError::ParentNotOnContainer.to_string())
        );
    }

    #[test]
    fn a_tree_past_the_limits_is_refused_with_them() {
        let page = || BlockType::Page;
        let deep = [("a", "", page()), ("b", "a", page()), ("c", "b", page())];
        let error = check(&of("w", ContainerKind::Wiki, &deep), LIMITS).expect_err("too deep");
        assert_eq!(error.reason(), Some(Reason::PageTooDeep));
        let wide = [
            ("a", "", page()),
            ("b", "", page()),
            ("c", "", page()),
            ("d", "", page()),
        ];
        let error = check(&of("w", ContainerKind::Wiki, &wide), LIMITS).expect_err("too many");
        assert_eq!(error.reason(), Some(Reason::TooManyPages));
    }

    #[test]
    fn a_chain_of_many_pages_is_refused_by_their_count_at_once() {
        let ids: Vec<String> = (0..200_000).map(|at| format!("p{at}")).collect();
        let blocks: Vec<(&str, &str, BlockType)> = ids
            .iter()
            .enumerate()
            .map(|(at, id)| {
                let parent = if at == 0 { "" } else { ids[at - 1].as_str() };
                (id.as_str(), parent, BlockType::Page)
            })
            .collect();
        let archive = of("w", ContainerKind::Wiki, &blocks);
        let limits = PageLimits {
            depth: 10,
            pages: 5_000,
        };
        let error = check(&archive, limits).expect_err("too many");
        assert_eq!(error.reason(), Some(Reason::TooManyPages));
    }
}
