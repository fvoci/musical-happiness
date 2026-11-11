//! Port of `server/model/boards_and_blocks_test.go`. Each test maps to one subtest of the
//! original. `TestIsValidDeleteBoardsAndBlocks` is commented out there; its three cases
//! are covered at the end against the behavior the code has.

use fvoci_model::{
    Block, BlockPatch, BlockType, Board, BoardPatch, BoardType, BoardsAndBlocks,
    BoardsAndBlocksError, DeleteBoardsAndBlocks, PatchBoardsAndBlocks,
    generate_boards_and_blocks_ids, renew_boards_and_blocks_ids,
};

fn board(id: &str, board_type: BoardType, title: &str) -> Board {
    Board {
        id: id.to_owned(),
        board_type,
        title: title.to_owned(),
        ..Board::default()
    }
}

fn block(id: &str, board_id: &str, block_type: BlockType) -> Block {
    Block {
        id: id.to_owned(),
        board_id: board_id.to_owned(),
        block_type,
        ..Block::default()
    }
}

fn ids(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

fn title_patch(title: &str) -> BlockPatch {
    BlockPatch {
        title: Some(title.to_owned()),
        ..BlockPatch::default()
    }
}

/// `TestIsValidBoardsAndBlocks` / "no boards"
#[test]
fn boards_and_blocks_validate_no_boards() {
    let bab = BoardsAndBlocks {
        boards: vec![],
        blocks: vec![
            block("block-id-1", "board-id-1", BlockType::Card),
            block("block-id-2", "board-id-2", BlockType::Card),
        ],
    };
    assert_eq!(bab.validate(), Err(BoardsAndBlocksError::NoBoards));
}

/// `TestIsValidBoardsAndBlocks` / "no blocks"
#[test]
fn boards_and_blocks_validate_no_blocks() {
    let bab = BoardsAndBlocks {
        boards: vec![
            board("board-id-1", BoardType::Open, ""),
            board("board-id-2", BoardType::Private, ""),
        ],
        blocks: vec![],
    };
    assert_eq!(bab.validate(), Err(BoardsAndBlocksError::NoBlocks));
}

/// `TestIsValidBoardsAndBlocks` / "block that doesn't belong to the boards"
#[test]
fn boards_and_blocks_validate_block_that_does_not_belong_to_the_boards() {
    let bab = BoardsAndBlocks {
        boards: vec![
            board("board-id-1", BoardType::Open, ""),
            board("board-id-2", BoardType::Private, ""),
        ],
        blocks: vec![
            block("block-id-1", "board-id-1", BlockType::Card),
            block("block-id-3", "board-id-3", BlockType::Card),
            block("block-id-2", "board-id-2", BlockType::Card),
        ],
    };
    assert_eq!(
        bab.validate(),
        Err(BoardsAndBlocksError::BlockWithoutBoard {
            block_id: "block-id-3".to_owned()
        })
    );
}

/// `TestIsValidBoardsAndBlocks` / "valid boards and blocks"
#[test]
fn boards_and_blocks_validate_valid() {
    let bab = BoardsAndBlocks {
        boards: vec![
            board("board-id-1", BoardType::Open, ""),
            board("board-id-2", BoardType::Private, ""),
        ],
        blocks: vec![
            block("block-id-1", "board-id-1", BlockType::Card),
            block("block-id-3", "board-id-2", BlockType::Card),
            block("block-id-2", "board-id-2", BlockType::Card),
        ],
    };
    assert_eq!(bab.validate(), Ok(()));
}

/// `TestGenerateBoardsAndBlocksIDs` / "invalid boards and blocks"
#[test]
fn generate_boards_and_blocks_ids_invalid_boards_and_blocks() {
    let bab = BoardsAndBlocks {
        boards: vec![],
        blocks: vec![
            block("block-id-1", "board-id-1", BlockType::Card),
            block("block-id-2", "board-id-2", BlockType::Card),
        ],
    };
    assert_eq!(
        generate_boards_and_blocks_ids(bab),
        Err(BoardsAndBlocksError::NoBoards)
    );
}

/// Boards that are stored already get new identifiers also when they have no blocks,
/// and a block of a board that is not among them is left out.
#[test]
fn renew_boards_and_blocks_ids_takes_boards_without_blocks() {
    let renewed = renew_boards_and_blocks_ids(BoardsAndBlocks {
        boards: vec![board("board-id-1", BoardType::Open, "title")],
        blocks: vec![],
    });
    assert_eq!(renewed.boards.len(), 1, "the board is kept");
    assert_ne!(renewed.boards[0].id, "board-id-1", "under a new identifier");
    assert!(renewed.blocks.is_empty(), "without blocks");

    let renewed = renew_boards_and_blocks_ids(BoardsAndBlocks {
        boards: vec![board("board-id-1", BoardType::Open, "title")],
        blocks: vec![
            block("block-id-1", "board-id-1", BlockType::Card),
            block("block-id-2", "board-id-2", BlockType::Card),
        ],
    });
    assert_eq!(renewed.blocks.len(), 1, "the block of no board is left out");
    assert_eq!(renewed.blocks[0].board_id, renewed.boards[0].id, "attached");
}

/// `TestGenerateBoardsAndBlocksIDs` / "correctly generates IDs for all the boards and
/// links the blocks to them, with new IDs too"
#[test]
fn generate_boards_and_blocks_ids_links_blocks_to_new_board_ids() {
    let bab = BoardsAndBlocks {
        boards: vec![
            board("board-id-1", BoardType::Open, "board1"),
            board("board-id-2", BoardType::Private, "board2"),
            board("board-id-3", BoardType::Open, "board3"),
        ],
        blocks: vec![
            block("block-id-1", "board-id-1", BlockType::Card),
            block("block-id-2", "board-id-2", BlockType::View),
            block("block-id-3", "board-id-2", BlockType::Comment),
        ],
    };

    let result = generate_boards_and_blocks_ids(bab).expect("the input is valid");

    let board_by_title = |title: &str| -> &Board {
        result
            .boards
            .iter()
            .find(|board| board.title == title)
            .unwrap_or_else(|| panic!("no board titled {title}"))
    };
    let block_by_type = |block_type: BlockType| -> &Block {
        result
            .blocks
            .iter()
            .find(|block| block.block_type == block_type)
            .unwrap_or_else(|| panic!("no block of type {block_type}"))
    };

    // Every board and block has a fresh identifier, and the blocks point at the new
    // identifiers of their boards.
    let board1 = board_by_title("board1");
    assert!(!board1.id.is_empty(), "board1 has no id");
    assert_ne!(board1.id, "board-id-1");
    let board2 = board_by_title("board2");
    assert!(!board2.id.is_empty(), "board2 has no id");
    assert_ne!(board2.id, "board-id-2");
    let board3 = board_by_title("board3");
    assert!(!board3.id.is_empty(), "board3 has no id");
    assert_ne!(board3.id, "board-id-3");

    let block1 = block_by_type(BlockType::Card);
    assert!(!block1.id.is_empty(), "block1 has no id");
    assert_ne!(block1.id, "block-id-1");
    assert_eq!(block1.board_id, board1.id);

    let block2 = block_by_type(BlockType::View);
    assert!(!block2.id.is_empty(), "block2 has no id");
    assert_ne!(block2.id, "block-id-2");
    assert_eq!(block2.board_id, board2.id);

    let block3 = block_by_type(BlockType::Comment);
    assert!(!block3.id.is_empty(), "block3 has no id");
    assert_ne!(block3.id, "block-id-3");
    assert_eq!(block3.board_id, board2.id);
}

/// `TestIsValidPatchBoardsAndBlocks` / "no board ids"
#[test]
fn patch_boards_and_blocks_validate_no_board_ids() {
    let pbab = PatchBoardsAndBlocks {
        board_ids: vec![],
        board_patches: vec![],
        block_ids: ids(&["block-id-1"]),
        block_patches: vec![
            title_patch("new title"),
            BlockPatch {
                schema: Some(1),
                ..BlockPatch::default()
            },
        ],
    };
    assert_eq!(pbab.validate(), Err(BoardsAndBlocksError::NoBoards));
}

/// `TestIsValidPatchBoardsAndBlocks` / "missmatch board IDs and patches"
#[test]
fn patch_boards_and_blocks_validate_mismatched_board_ids_and_patches() {
    let pbab = PatchBoardsAndBlocks {
        board_ids: ids(&["board-id-1", "board-id-2"]),
        board_patches: vec![BoardPatch {
            title: Some("new title".to_owned()),
            ..BoardPatch::default()
        }],
        block_ids: ids(&["block-id-1"]),
        block_patches: vec![title_patch("new title")],
    };
    assert_eq!(
        pbab.validate(),
        Err(BoardsAndBlocksError::BoardIdsAndPatchesMismatch)
    );
}

/// `TestIsValidPatchBoardsAndBlocks` / "missmatch block IDs and patches"
#[test]
fn patch_boards_and_blocks_validate_mismatched_block_ids_and_patches() {
    let pbab = PatchBoardsAndBlocks {
        board_ids: ids(&["board-id-1", "board-id-2"]),
        board_patches: vec![
            BoardPatch {
                title: Some("new title".to_owned()),
                ..BoardPatch::default()
            },
            BoardPatch {
                description: Some("new description".to_owned()),
                ..BoardPatch::default()
            },
        ],
        block_ids: ids(&["block-id-1"]),
        block_patches: vec![
            title_patch("new title"),
            BlockPatch {
                schema: Some(1),
                ..BlockPatch::default()
            },
        ],
    };
    assert_eq!(
        pbab.validate(),
        Err(BoardsAndBlocksError::BlockIdsAndPatchesMismatch)
    );
}

/// `TestIsValidPatchBoardsAndBlocks` / "valid"
#[test]
fn patch_boards_and_blocks_validate_valid() {
    let pbab = PatchBoardsAndBlocks {
        board_ids: ids(&["board-id-1", "board-id-2"]),
        board_patches: vec![
            BoardPatch {
                title: Some("new title".to_owned()),
                ..BoardPatch::default()
            },
            BoardPatch {
                description: Some("new description".to_owned()),
                ..BoardPatch::default()
            },
        ],
        block_ids: ids(&["block-id-1"]),
        block_patches: vec![title_patch("new title")],
    };
    assert_eq!(pbab.validate(), Ok(()));
}

/// The original `TestIsValidDeleteBoardsAndBlocks` is disabled with a "TODO fix this". Its
/// "no block ids" case expects an error the code never returns, so the three cases below
/// pin what the code does instead.
#[test]
fn delete_boards_and_blocks_validate_requires_only_boards() {
    let no_boards = DeleteBoardsAndBlocks {
        boards: vec![],
        blocks: ids(&["block-id-1"]),
    };
    assert_eq!(no_boards.validate(), Err(BoardsAndBlocksError::NoBoards));

    let no_blocks = DeleteBoardsAndBlocks {
        boards: ids(&["board-id-1", "board-id-2"]),
        blocks: vec![],
    };
    assert_eq!(no_blocks.validate(), Ok(()));

    let both = DeleteBoardsAndBlocks {
        boards: ids(&["board-id-1", "board-id-2"]),
        blocks: ids(&["block-id-1"]),
    };
    assert_eq!(both.validate(), Ok(()));
}

/// Not in the original suite: the delete request built from created boards and blocks
/// lists their identifiers in order.
#[test]
fn delete_boards_and_blocks_from_boards_and_blocks() {
    let bab = BoardsAndBlocks {
        boards: vec![board("board-id-1", BoardType::Open, "")],
        blocks: vec![
            block("block-id-1", "board-id-1", BlockType::Card),
            block("block-id-2", "board-id-1", BlockType::View),
        ],
    };
    assert_eq!(
        DeleteBoardsAndBlocks::from(&bab),
        DeleteBoardsAndBlocks {
            boards: ids(&["board-id-1"]),
            blocks: ids(&["block-id-1", "block-id-2"]),
        }
    );
}
