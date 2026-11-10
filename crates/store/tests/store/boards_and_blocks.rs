//! Port of `server/services/store/storetests/boards_and_blocks.go`.
//!
//! One test per test function of the original; its subtests are the commented sections,
//! in the original order, on one store. The original skips the "nothing should be saved"
//! subtests on SQLite, where it has no transactions; the port is atomic on both
//! databases and runs them everywhere.
//!
//! The original names its boards, blocks, team and user with fixed strings. Here every
//! identifier is generated, and the team and the users a board or block refers to are
//! stored first.
//!
//! The tests after [`duplicate_board`] have no counterpart in the original.

use fvoci_model::{
    BLOCK_TITLE_MAX_CHARS, Block, BlockError, BlockPatch, BlockType, Board, BoardMember,
    BoardPatch, BoardType, BoardsAndBlocks, BoardsAndBlocksError, DeleteBoardsAndBlocks,
    PatchBoardsAndBlocks, is_id, new_id,
};
use fvoci_store::{BlockFilter, DuplicateAs, PublicBoards, Store, StoreError};

use crate::common;

/// New identifiers, as many as the pattern they are bound to names.
fn new_ids<const N: usize>() -> [String; N] {
    std::array::from_fn(|_| new_id())
}

/// The identifiers, sorted.
fn sorted(ids: &[&str]) -> Vec<String> {
    let mut ids: Vec<String> = ids.iter().map(|&id| id.to_owned()).collect();
    ids.sort();
    ids
}

fn new_board(id: &str, team_id: &str, board_type: BoardType) -> Board {
    Board {
        id: id.to_owned(),
        team_id: team_id.to_owned(),
        board_type,
        ..Board::default()
    }
}

fn new_card(id: &str, board_id: &str) -> Block {
    Block {
        id: id.to_owned(),
        board_id: board_id.to_owned(),
        block_type: BlockType::Card,
        ..Block::default()
    }
}

fn sorted_board_ids(boards: &[Board]) -> Vec<String> {
    let mut ids: Vec<String> = boards.iter().map(|board| board.id.clone()).collect();
    ids.sort();
    ids
}

fn is_title_size_limit_exceeded(error: &StoreError) -> bool {
    matches!(
        error,
        StoreError::InvalidBlock {
            source: BlockError::TitleSizeLimitExceeded,
            ..
        }
    )
}

/// Inserts a new open board of the team on behalf of the user and returns its
/// identifier.
async fn insert_new_board(store: &Store, team_id: &str, user_id: &str) -> String {
    let mut board = new_board(&new_id(), team_id, BoardType::Open);
    store
        .insert_board(&mut board, user_id)
        .await
        .expect("the board is inserted");
    board.id
}

/// Inserts a new block of the given board on behalf of the user and returns its
/// identifier.
async fn insert_new_block(store: &Store, board_id: &str, user_id: &str) -> String {
    let mut block = Block {
        id: new_id(),
        board_id: board_id.to_owned(),
        ..Block::default()
    };
    store
        .insert_block(&mut block, user_id)
        .await
        .expect("the block is inserted");
    block.id
}

/// Two boards with two blocks each, as the subtests of the original's
/// `testDeleteBoardsAndBlocks` set up.
struct TwoBoards {
    board_ids: [String; 2],
    block_ids: [String; 4],
}

impl TwoBoards {
    /// Inserts the boards and the blocks. `last_block_board_id` overrides the board of
    /// the fourth block; it must name a board that exists.
    async fn insert(
        store: &Store,
        team_id: &str,
        user_id: &str,
        last_block_board_id: Option<&str>,
    ) -> Self {
        let board1 = insert_new_board(store, team_id, user_id).await;
        let block1 = insert_new_block(store, &board1, user_id).await;
        let block2 = insert_new_block(store, &board1, user_id).await;
        let board2 = insert_new_board(store, team_id, user_id).await;
        let block3 = insert_new_block(store, &board2, user_id).await;
        let block4 = insert_new_block(store, last_block_board_id.unwrap_or(&board2), user_id).await;
        Self {
            board_ids: [board1, board2],
            block_ids: [block1, block2, block3, block4],
        }
    }

    /// "all the entities should still exist"
    async fn assert_all_exist(&self, store: &Store) {
        for board_id in &self.board_ids {
            let result = store.get_board(board_id).await;
            assert!(result.is_ok(), "board {board_id}: {result:?}");
        }
        for block_id in &self.block_ids {
            let result = store.get_block(block_id).await;
            assert!(result.is_ok(), "block {block_id}: {result:?}");
        }
    }
}

#[tokio::test]
#[expect(
    clippy::too_many_lines,
    reason = "one test per test function of the original, whose subtests share a store"
)]
async fn create_boards_and_blocks() {
    let test = common::setup().await;
    let team_id = common::team(&test).await;
    let user_id = common::user(&test, "user").await;

    let stored = test
        .get_boards_for_user_and_team(&user_id, &team_id, PublicBoards::Include)
        .await
        .expect("the query succeeds");
    assert!(stored.is_empty(), "got {stored:?}");

    // "create boards and blocks"
    {
        let [board1, board2, board3] = new_ids();
        let [block1, block2] = new_ids();
        let mut new_bab = BoardsAndBlocks {
            boards: vec![
                new_board(&board1, &team_id, BoardType::Open),
                new_board(&board2, &team_id, BoardType::Private),
                new_board(&board3, &team_id, BoardType::Open),
            ],
            blocks: vec![new_card(&block1, &board1), new_card(&block2, &board2)],
        };

        test.create_boards_and_blocks(&mut new_bab, &user_id)
            .await
            .expect("the boards and blocks are created");
        assert_eq!(new_bab.boards.len(), 3, "got {:?}", new_bab.boards);
        assert_eq!(new_bab.blocks.len(), 2, "got {:?}", new_bab.blocks);

        assert_eq!(
            sorted_board_ids(&new_bab.boards),
            sorted(&[&board1, &board2, &board3]),
            "the boards keep their identifiers"
        );
        assert_eq!(
            common::sorted_ids(&new_bab.blocks),
            sorted(&[&block1, &block2]),
            "the blocks keep their identifiers"
        );
    }

    // "create boards and blocks with admin"
    {
        let [board4, board5, board6] = new_ids();
        let [block3, block4] = new_ids();
        let mut new_bab = BoardsAndBlocks {
            boards: vec![
                new_board(&board4, &team_id, BoardType::Open),
                new_board(&board5, &team_id, BoardType::Private),
                new_board(&board6, &team_id, BoardType::Open),
            ],
            blocks: vec![new_card(&block3, &board4), new_card(&block4, &board5)],
        };

        let members = test
            .create_boards_and_blocks_with_admin(&mut new_bab, &user_id)
            .await
            .expect("the boards, blocks and admins are created");
        assert_eq!(new_bab.boards.len(), 3, "got {:?}", new_bab.boards);
        assert_eq!(new_bab.blocks.len(), 2, "got {:?}", new_bab.blocks);
        assert_eq!(members.len(), 3, "got {members:?}");

        assert_eq!(
            sorted_board_ids(&new_bab.boards),
            sorted(&[&board4, &board5, &board6]),
            "the boards keep their identifiers"
        );
        assert_eq!(
            common::sorted_ids(&new_bab.blocks),
            sorted(&[&block3, &block4]),
            "the blocks keep their identifiers"
        );

        let mut member_board_ids = Vec::new();
        for member in &members {
            assert_eq!(user_id, member.user_id, "the creator is the member");
            member_board_ids.push(member.board_id.clone());
        }
        member_board_ids.sort_unstable();
        assert_eq!(
            member_board_ids,
            sorted(&[&board4, &board5, &board6]),
            "one membership per board"
        );
    }

    // The two subtests that follow name the same boards and blocks: neither stores any.
    let [board7, board8, board9] = new_ids();
    let [block5, block6] = new_ids();

    // "on failure, nothing should be saved"
    {
        // one of the blocks is invalid as it doesn't have BoardID
        let mut new_bab = BoardsAndBlocks {
            boards: vec![
                new_board(&board7, &team_id, BoardType::Open),
                new_board(&board8, &team_id, BoardType::Private),
                new_board(&board9, &team_id, BoardType::Open),
            ],
            blocks: vec![new_card(&block5, &board7), new_card(&block6, "")],
        };

        let error = test
            .create_boards_and_blocks(&mut new_bab, &user_id)
            .await
            .expect_err("a block without a board is rejected");
        assert!(
            matches!(
                error,
                StoreError::InvalidBlock {
                    source: BlockError::EmptyBoardId,
                    ..
                }
            ),
            "got {error:?}"
        );

        let error = test
            .create_boards_and_blocks_with_admin(&mut new_bab, &user_id)
            .await
            .expect_err("a block without a board is rejected");
        assert!(
            matches!(error, StoreError::InvalidBlock { .. }),
            "got {error:?}"
        );

        // Added: the original only checks the errors, and on SQLite it does save the
        // boards and the first block. Here nothing is saved on either database.
        for board_id in [&board7, &board8, &board9] {
            let error = test
                .get_board(board_id)
                .await
                .expect_err("the board was not saved");
            assert!(error.is_not_found(), "board {board_id}: {error:?}");
        }
        let error = test
            .get_block(&block5)
            .await
            .expect_err("the block was not saved");
        assert!(error.is_not_found(), "got {error:?}");
        let members = test
            .get_members_for_user(&user_id)
            .await
            .expect("the memberships are read");
        assert_eq!(members.len(), 3, "only the three of the previous subtest");
    }

    // "should apply block size limits"
    {
        // one of the blocks is invalid as it has a title too large
        let mut new_bab = BoardsAndBlocks {
            boards: vec![
                new_board(&board7, &team_id, BoardType::Open),
                new_board(&board8, &team_id, BoardType::Private),
                new_board(&board9, &team_id, BoardType::Open),
            ],
            blocks: vec![
                new_card(&block5, &board7),
                Block {
                    title: "A".repeat(BLOCK_TITLE_MAX_CHARS + 1),
                    ..new_card(&block6, &board8)
                },
            ],
        };

        let error = test
            .create_boards_and_blocks(&mut new_bab, &user_id)
            .await
            .expect_err("a block with an oversized title is rejected");
        assert!(is_title_size_limit_exceeded(&error), "got {error:?}");

        let error = test
            .create_boards_and_blocks_with_admin(&mut new_bab, &user_id)
            .await
            .expect_err("a block with an oversized title is rejected");
        assert!(is_title_size_limit_exceeded(&error), "got {error:?}");
    }

    test.teardown().await;
}

#[tokio::test]
#[expect(
    clippy::too_many_lines,
    reason = "one test per test function of the original, whose subtests share a store"
)]
async fn patch_boards_and_blocks() {
    let test = common::setup().await;
    let team_id = common::team(&test).await;
    let user_id = common::user(&test, "user").await;

    // The subtests share their boards and blocks, as the original's do by naming them
    // alike: a later subtest stores the first board and the first block again.
    let [board1_id, board2_id, board3_id] = new_ids();
    let [block1_id, block2_id] = new_ids();

    // "on failure, nothing should be saved"
    {
        let initial_title = "initial title";
        let new_title = "new title";

        let mut board = Board {
            title: initial_title.to_owned(),
            ..new_board(&board1_id, &team_id, BoardType::Open)
        };
        test.insert_board(&mut board, &user_id)
            .await
            .expect("the board is inserted");

        let mut block = Block {
            id: block1_id.clone(),
            board_id: board1_id.clone(),
            title: initial_title.to_owned(),
            ..Block::default()
        };
        test.insert_block(&mut block, &user_id)
            .await
            .expect("the block is inserted");

        // apply the patches; the second block does not exist yet
        let title_patch = BlockPatch {
            title: Some(new_title.to_owned()),
            ..BlockPatch::default()
        };
        let pbab = PatchBoardsAndBlocks {
            board_ids: vec![board1_id.clone()],
            board_patches: vec![BoardPatch {
                title: Some(new_title.to_owned()),
                ..BoardPatch::default()
            }],
            block_ids: vec![block1_id.clone(), block2_id.clone()],
            block_patches: vec![title_patch.clone(), title_patch],
        };

        let error = test
            .patch_boards_and_blocks(&pbab, &user_id)
            .await
            .expect_err("the second block does not exist");
        assert!(error.is_not_found(), "got {error:?}");

        // check that things have not changed
        let r_board = test.get_board(&board1_id).await.expect("the board exists");
        assert_eq!(initial_title, r_board.title, "the board patch is undone");

        let r_block = test.get_block(&block1_id).await.expect("the block exists");
        assert_eq!(initial_title, r_block.title, "the block patch is undone");
    }

    // "should apply block size limits"
    {
        let initial_title = "initial title";
        let new_title = "A".repeat(BLOCK_TITLE_MAX_CHARS + 1);

        let mut board = Board {
            title: initial_title.to_owned(),
            ..new_board(&board1_id, &team_id, BoardType::Open)
        };
        test.insert_board(&mut board, &user_id)
            .await
            .expect("the board is inserted");

        let mut block = Block {
            id: block1_id.clone(),
            board_id: board1_id.clone(),
            title: initial_title.to_owned(),
            ..Block::default()
        };
        test.insert_block(&mut block, &user_id)
            .await
            .expect("the block is inserted");

        // apply the patches
        let pbab = PatchBoardsAndBlocks {
            block_ids: vec![block1_id.clone()],
            block_patches: vec![BlockPatch {
                title: Some(new_title),
                ..BlockPatch::default()
            }],
            ..PatchBoardsAndBlocks::default()
        };

        let error = test
            .patch_boards_and_blocks(&pbab, &user_id)
            .await
            .expect_err("the title is too long");
        assert!(is_title_size_limit_exceeded(&error), "got {error:?}");

        // check that things have not changed
        let r_block = test.get_block(&block1_id).await.expect("the block exists");
        assert_eq!(initial_title, r_block.title, "the block keeps its title");
    }

    // "patch boards and blocks"
    {
        let mut new_bab = BoardsAndBlocks {
            boards: vec![
                Board {
                    description: "initial description".to_owned(),
                    ..new_board(&board1_id, &team_id, BoardType::Open)
                },
                new_board(&board2_id, &team_id, BoardType::Private),
                Board {
                    title: "initial title".to_owned(),
                    ..new_board(&board3_id, &team_id, BoardType::Open)
                },
            ],
            blocks: vec![
                Block {
                    title: "initial title".to_owned(),
                    ..new_card(&block1_id, &board1_id)
                },
                Block {
                    schema: 1,
                    ..new_card(&block2_id, &board2_id)
                },
            ],
        };

        test.create_boards_and_blocks(&mut new_bab, &user_id)
            .await
            .expect("the boards and blocks are created");
        assert_eq!(new_bab.boards.len(), 3, "got {:?}", new_bab.boards);
        assert_eq!(new_bab.blocks.len(), 2, "got {:?}", new_bab.blocks);

        // apply the patches
        let new_title = "new title";
        let new_description = "new description";
        let new_schema: i64 = 2;

        let pbab = PatchBoardsAndBlocks {
            board_ids: vec![board3_id.clone(), board1_id.clone()],
            board_patches: vec![
                BoardPatch {
                    title: Some(new_title.to_owned()),
                    description: Some(new_description.to_owned()),
                    ..BoardPatch::default()
                },
                BoardPatch {
                    description: Some(new_description.to_owned()),
                    ..BoardPatch::default()
                },
            ],
            block_ids: vec![block1_id.clone(), block2_id.clone()],
            block_patches: vec![
                BlockPatch {
                    title: Some(new_title.to_owned()),
                    ..BlockPatch::default()
                },
                BlockPatch {
                    schema: Some(new_schema),
                    ..BlockPatch::default()
                },
            ],
        };

        let bab = test
            .patch_boards_and_blocks(&pbab, &user_id)
            .await
            .expect("the patches are applied");
        assert_eq!(bab.boards.len(), 2, "got {:?}", bab.boards);
        assert_eq!(bab.blocks.len(), 2, "got {:?}", bab.blocks);

        // check that things have changed
        let board1 = test.get_board(&board1_id).await.expect("the board exists");
        assert_eq!(new_description, board1.description, "board 1 is patched");

        let board3 = test.get_board(&board3_id).await.expect("the board exists");
        assert_eq!(new_title, board3.title, "board 3 is patched");
        assert_eq!(new_description, board3.description, "board 3 is patched");

        let block1 = test.get_block(&block1_id).await.expect("the block exists");
        assert_eq!(new_title, block1.title, "block 1 is patched");

        let block2 = test.get_block(&block2_id).await.expect("the block exists");
        assert_eq!(new_schema, block2.schema, "block 2 is patched");

        // Added: what comes back is what was stored, in the order of the request.
        assert_eq!(bab.boards, [board3, board1], "the patched boards");
        assert_eq!(bab.blocks, [block1, block2], "the patched blocks");
    }

    test.teardown().await;
}

#[tokio::test]
async fn delete_boards_and_blocks() {
    let test = common::setup().await;
    let team_id = common::team(&test).await;
    let user_id = common::user(&test, "user").await;

    // "should not delete anything if a block doesn't belong to any of the boards"
    {
        // The original puts the fourth block on a board that does not exist. A block's
        // board exists here, so it is a third board, which the request leaves out.
        let different_board_id = insert_new_board(&test, &team_id, &user_id).await;
        let entities =
            TwoBoards::insert(&test, &team_id, &user_id, Some(&different_board_id)).await;

        let dbab = DeleteBoardsAndBlocks {
            boards: entities.board_ids.to_vec(),
            blocks: entities.block_ids.to_vec(),
        };

        let expected_error_msg = format!(
            "block {} doesn't belong to any of the boards in the delete request",
            entities.block_ids[3]
        );
        let error = test
            .delete_boards_and_blocks(&dbab, &user_id)
            .await
            .expect_err("the fourth block belongs to another board");
        assert_eq!(error.to_string(), expected_error_msg, "got {error:?}");

        // all the entities should still exist
        entities.assert_all_exist(&test).await;
    }

    // "should not delete anything if a board doesn't exist"
    {
        let entities = TwoBoards::insert(&test, &team_id, &user_id, None).await;

        let mut boards = entities.board_ids.to_vec();
        boards.push(new_id());
        let dbab = DeleteBoardsAndBlocks {
            boards,
            blocks: entities.block_ids.to_vec(),
        };

        let error = test
            .delete_boards_and_blocks(&dbab, &user_id)
            .await
            .expect_err("the third board does not exist");
        assert!(error.is_not_found(), "got {error:?}");

        // all the entities should still exist
        entities.assert_all_exist(&test).await;
    }

    // "should not delete anything if a block doesn't exist"
    {
        let entities = TwoBoards::insert(&test, &team_id, &user_id, None).await;

        let mut blocks = entities.block_ids.to_vec();
        blocks.push(new_id());
        let dbab = DeleteBoardsAndBlocks {
            boards: entities.board_ids.to_vec(),
            blocks,
        };

        let error = test
            .delete_boards_and_blocks(&dbab, &user_id)
            .await
            .expect_err("the fifth block does not exist");
        assert!(error.is_not_found(), "got {error:?}");

        // all the entities should still exist
        entities.assert_all_exist(&test).await;
    }

    // "should work properly if all the entities are related"
    {
        let entities = TwoBoards::insert(&test, &team_id, &user_id, None).await;

        let dbab = DeleteBoardsAndBlocks {
            boards: entities.board_ids.to_vec(),
            blocks: entities.block_ids.to_vec(),
        };

        test.delete_boards_and_blocks(&dbab, &user_id)
            .await
            .expect("the boards and blocks are deleted");

        for board_id in &entities.board_ids {
            let error = test
                .get_board(board_id)
                .await
                .expect_err("the board is gone");
            assert!(error.is_not_found(), "board {board_id}: {error:?}");
        }
        for block_id in &entities.block_ids {
            let error = test
                .get_block(block_id)
                .await
                .expect_err("the block is gone");
            assert!(error.is_not_found(), "block {block_id}: {error:?}");
        }
    }

    test.teardown().await;
}

#[tokio::test]
async fn duplicate_board() {
    let test = common::setup().await;
    let team_id = common::team(&test).await;
    let user_id = common::user(&test, "user").await;

    // The original gives the first board a channel and checks that the copies have
    // none; a board has no channel here.
    let [board1, board2, board3] = new_ids();
    let [block1, block1a, block2] = new_ids();
    let mut new_bab = BoardsAndBlocks {
        boards: vec![
            new_board(&board1, &team_id, BoardType::Open),
            new_board(&board2, &team_id, BoardType::Private),
            new_board(&board3, &team_id, BoardType::Open),
        ],
        blocks: vec![
            new_card(&block1, &board1),
            Block {
                block_type: BlockType::Comment,
                ..new_card(&block1a, &board1)
            },
            new_card(&block2, &board2),
        ],
    };

    test.create_boards_and_blocks(&mut new_bab, &user_id)
        .await
        .expect("the boards and blocks are created");
    assert_eq!(new_bab.boards.len(), 3, "got {:?}", new_bab.boards);
    assert_eq!(new_bab.blocks.len(), 3, "got {:?}", new_bab.blocks);

    // "duplicate existing board as no template"
    {
        let (bab, members) = test
            .duplicate_board(&board1, &user_id, Some(&team_id), DuplicateAs::Copy)
            .await
            .expect("the board is duplicated");
        assert_eq!(members.len(), 1, "got {members:?}");
        assert_eq!(bab.boards.len(), 1, "got {:?}", bab.boards);
        assert_eq!(bab.blocks.len(), 1, "got {:?}", bab.blocks);
        assert!(!bab.boards[0].is_template, "got {:?}", bab.boards[0]);
    }

    // "duplicate existing board as template"
    {
        let (bab, members) = test
            .duplicate_board(&board1, &user_id, Some(&team_id), DuplicateAs::Template)
            .await
            .expect("the board is duplicated");
        assert_eq!(members.len(), 1, "got {members:?}");
        assert_eq!(bab.boards.len(), 1, "got {:?}", bab.boards);
        assert_eq!(bab.blocks.len(), 1, "got {:?}", bab.blocks);
        assert!(bab.boards[0].is_template, "got {:?}", bab.boards[0]);
    }

    // "duplicate not existing board"
    {
        let error = test
            .duplicate_board(&new_id(), &user_id, Some(&team_id), DuplicateAs::Copy)
            .await
            .expect_err("there is no such board");
        assert!(error.is_not_found(), "got {error:?}");
    }

    test.teardown().await;
}

// Characterization tests: behavior of the original that its test suite does not cover.

/// `server/services/store/sqlstore/boards_and_blocks.go:136-183`.
#[tokio::test]
#[expect(
    clippy::too_many_lines,
    reason = "the four kinds of duplication build on one another"
)]
async fn duplicate_board_copies_under_new_ids_as_a_private_board_of_the_user() {
    let test = common::setup().await;
    let team_id = common::team(&test).await;
    let other_team_id = common::team(&test).await;
    let author = common::user(&test, "author").await;
    let copier = common::user(&test, "copier").await;

    let source_id = new_id();
    let [card_id, text_id, comment_id] = new_ids();
    let mut source = BoardsAndBlocks {
        boards: vec![Board {
            title: "Roadmap".to_owned(),
            description: "Plans".to_owned(),
            icon: "🗺".to_owned(),
            ..new_board(&source_id, &team_id, BoardType::Open)
        }],
        blocks: vec![
            Block {
                parent_id: source_id.clone(),
                title: "Card".to_owned(),
                ..new_card(&card_id, &source_id)
            },
            Block {
                parent_id: card_id.clone(),
                block_type: BlockType::Text,
                title: "Text".to_owned(),
                ..new_card(&text_id, &source_id)
            },
            Block {
                parent_id: card_id.clone(),
                block_type: BlockType::Comment,
                ..new_card(&comment_id, &source_id)
            },
        ],
    };
    test.create_boards_and_blocks(&mut source, &author)
        .await
        .expect("the board and its blocks are created");

    // board -> board, same team
    let (copy, members) = test
        .duplicate_board(&source_id, &copier, None, DuplicateAs::Copy)
        .await
        .expect("the board is duplicated");
    let board = &copy.boards[0];
    assert_ne!(board.id, source_id, "the copy has an identifier of its own");
    assert!(is_id(&board.id), "got {}", board.id);
    assert_eq!(board.title, "Roadmap copy", "the title says it is a copy");
    assert_eq!(board.description, "Plans", "the description is copied");
    assert_eq!(board.icon, "🗺", "the icon is copied");
    assert_eq!(board.board_type, BoardType::Private, "a copy is private");
    assert_eq!(board.team_id, team_id, "the copy stays in the team");
    assert_eq!(board.created_by, copier, "the copier made the copy");
    assert_eq!(board.modified_by, copier, "the copier made the copy");
    assert!(!board.is_template, "a copy of a board is a board");
    assert!(
        !board.is_builtin,
        "a user's copy is not a built-in template"
    );
    assert_eq!(
        members,
        [BoardMember {
            board_id: board.id.clone(),
            user_id: copier.clone(),
            scheme_admin: true,
            scheme_editor: true,
            ..BoardMember::default()
        }],
        "the copier administers the copy"
    );

    // The blocks keep their shape under new identifiers; the comment is left behind.
    assert_eq!(copy.blocks.len(), 2, "got {:?}", copy.blocks);
    let card = copy
        .blocks
        .iter()
        .find(|block| block.block_type == BlockType::Card)
        .expect("the card is copied");
    let paragraph = copy
        .blocks
        .iter()
        .find(|block| block.block_type == BlockType::Text)
        .expect("the text is copied");
    assert_ne!(card.id, card_id, "the card has an identifier of its own");
    assert!(is_id(&card.id), "got {}", card.id);
    assert_eq!(card.board_id, board.id, "the card is on the copy");
    assert_eq!(
        card.parent_id, "",
        "a block that named its board as its parent is copied without a parent"
    );
    assert_eq!(paragraph.parent_id, card.id, "the text follows its card");
    assert_eq!(paragraph.board_id, board.id, "the text is on the copy");
    assert_eq!(card.created_by, copier, "the copier made the card");

    // What is returned is what is stored.
    let stored_board = test.get_board(&board.id).await.expect("the copy exists");
    assert_eq!(&stored_board, board, "the stored copy");
    let stored_blocks = test
        .get_blocks(&BlockFilter::board(&board.id))
        .await
        .expect("the blocks are read");
    assert_eq!(
        common::sorted_ids(&stored_blocks),
        common::sorted_ids(&copy.blocks),
        "the stored blocks of the copy"
    );
    // The source is untouched.
    let source_board = test.get_board(&source_id).await.expect("the source exists");
    assert_eq!(source_board, source.boards[0], "the source board");

    // board -> template, other team
    let (template, _) = test
        .duplicate_board(
            &source_id,
            &copier,
            Some(&other_team_id),
            DuplicateAs::Template,
        )
        .await
        .expect("the board is duplicated");
    let template = &template.boards[0];
    assert_eq!(template.title, "New board template", "a new template");
    assert_eq!(template.team_id, other_team_id, "in the team asked for");
    assert!(template.is_template, "got {template:?}");

    // template -> template
    let (template_copy, _) = test
        .duplicate_board(&template.id, &copier, None, DuplicateAs::Template)
        .await
        .expect("the template is duplicated");
    assert_eq!(
        template_copy.boards[0].title, "New board template copy",
        "a copy of a template"
    );
    assert_eq!(
        template_copy.boards[0].team_id, other_team_id,
        "in the team of the template"
    );

    // template -> board
    let (from_template, _) = test
        .duplicate_board(&template.id, &copier, None, DuplicateAs::Copy)
        .await
        .expect("the template is duplicated");
    assert_eq!(
        from_template.boards[0].title, "New board template",
        "a board made from a template keeps the title"
    );
    assert!(
        !from_template.boards[0].is_template,
        "got {:?}",
        from_template.boards[0]
    );

    test.teardown().await;
}

/// A board without blocks, or with comments only, is copied as a board without blocks.
/// The original refuses it (`server/services/store/sqlstore/boards_and_blocks.go:177`:
/// the identifiers are regenerated by a function that insists on at least one block).
#[tokio::test]
async fn duplicate_board_copies_a_board_without_blocks() {
    let test = common::setup().await;
    let team_id = common::team(&test).await;
    let user_id = common::user(&test, "user").await;

    let empty_id = insert_new_board(&test, &team_id, &user_id).await;
    let only_comments_id = insert_new_board(&test, &team_id, &user_id).await;
    let mut comment = Block {
        block_type: BlockType::Comment,
        ..new_card(&new_id(), &only_comments_id)
    };
    test.insert_block(&mut comment, &user_id)
        .await
        .expect("the comment is inserted");

    for (board_id, what) in [(&empty_id, "empty"), (&only_comments_id, "comments only")] {
        let (copy, members) = test
            .duplicate_board(board_id, &user_id, None, DuplicateAs::Copy)
            .await
            .expect("a board without blocks to copy is copied");
        assert_eq!(copy.boards.len(), 1, "{what}: one board");
        assert_ne!(&copy.boards[0].id, board_id, "{what}: a new identifier");
        assert!(copy.blocks.is_empty(), "{what}: no blocks");
        assert_eq!(members.len(), 1, "{what}: the copying user");
        let stored = test
            .get_blocks(&BlockFilter::board(&copy.boards[0].id))
            .await
            .expect("the copy's blocks are read");
        assert!(stored.is_empty(), "{what}: nothing was stored");
    }

    test.teardown().await;
}

/// `server/services/store/sqlstore/boards_and_blocks.go:76-98`: the original indexes the
/// patches by the position of the identifier and crashes when there are too few.
#[tokio::test]
async fn patch_boards_and_blocks_rejects_fewer_patches_than_ids() {
    let test = common::setup().await;
    let team_id = common::team(&test).await;
    let user_id = common::user(&test, "user").await;

    let board_id = insert_new_board(&test, &team_id, &user_id).await;
    let block_id = insert_new_block(&test, &board_id, &user_id).await;

    let boards_short = PatchBoardsAndBlocks {
        board_ids: vec![board_id.clone()],
        ..PatchBoardsAndBlocks::default()
    };
    let error = test
        .patch_boards_and_blocks(&boards_short, &user_id)
        .await
        .expect_err("there is no patch for the board");
    assert!(
        matches!(
            error,
            StoreError::InvalidBoardsAndBlocks(BoardsAndBlocksError::BoardIdsAndPatchesMismatch)
        ),
        "got {error:?}"
    );

    let blocks_short = PatchBoardsAndBlocks {
        board_ids: vec![board_id.clone()],
        board_patches: vec![BoardPatch {
            title: Some("patched".to_owned()),
            ..BoardPatch::default()
        }],
        block_ids: vec![block_id],
        block_patches: Vec::new(),
    };
    let error = test
        .patch_boards_and_blocks(&blocks_short, &user_id)
        .await
        .expect_err("there is no patch for the block");
    assert!(
        matches!(
            error,
            StoreError::InvalidBoardsAndBlocks(BoardsAndBlocksError::BlockIdsAndPatchesMismatch)
        ),
        "got {error:?}"
    );
    let board = test.get_board(&board_id).await.expect("the board exists");
    assert_eq!(board.title, "", "the board patch was rolled back");

    test.teardown().await;
}

/// `server/services/store/sqlstore/boards_and_blocks.go:110-123`: deleting a block
/// removes its direct children, and a child listed after it is then not found.
#[tokio::test]
async fn delete_boards_and_blocks_fails_when_a_block_is_listed_after_its_parent() {
    let test = common::setup().await;
    let team_id = common::team(&test).await;
    let user_id = common::user(&test, "user").await;

    let board_id = insert_new_board(&test, &team_id, &user_id).await;
    let card_id = insert_new_block(&test, &board_id, &user_id).await;
    let mut child = Block {
        id: new_id(),
        board_id: board_id.clone(),
        parent_id: card_id.clone(),
        ..Block::default()
    };
    test.insert_block(&mut child, &user_id)
        .await
        .expect("the child is inserted");

    let parent_first = DeleteBoardsAndBlocks {
        boards: vec![board_id.clone()],
        blocks: vec![card_id.clone(), child.id.clone()],
    };
    let error = test
        .delete_boards_and_blocks(&parent_first, &user_id)
        .await
        .expect_err("the child is gone by the time its turn comes");
    assert!(error.is_not_found(), "got {error:?}");
    test.get_block(&card_id).await.expect("nothing was deleted");

    let child_first = DeleteBoardsAndBlocks {
        boards: vec![board_id.clone()],
        blocks: vec![child.id, card_id],
    };
    test.delete_boards_and_blocks(&child_first, &user_id)
        .await
        .expect("children before parents works");
    test.get_board(&board_id)
        .await
        .expect_err("the board is gone");

    test.teardown().await;
}
