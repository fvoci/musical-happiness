//! Ports of `server/integrationtests/boards_and_blocks_test.go`, plus tests of what was
//! verified on the original while porting.
//!
//! Where the original looks boards up through the search route or stores blocks through
//! the application layer of the blocks area, these tests use the store.
//!
//! The identifiers in a request that creates boards and blocks (`"board-id-1"`, `"bd"`,
//! `"b1"`) only tie the request's blocks to its boards: nothing is stored under them, so
//! they need not be identifiers.

use fvoci_model::{
    Block, BlockPatch, BlockType, Board, BoardMember, BoardPatch, BoardType, BoardsAndBlocks,
    DeleteBoardsAndBlocks, PatchBoardsAndBlocks, new_id,
};
use fvoci_store::BlockFilter;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

use crate::boards::{
    TeamSocket, board_with_admin, board_without_members, insert_block, member, members_of,
    new_board,
};
use crate::common::{
    ApiResponse, Client, ROOT_TEAM_ID, Setup, TEST_TEAM_ID, TestServer, UNKNOWN_ID,
};

const PATH: &str = "/boards-and-blocks";

// -- helpers --------------------------------------------------------------------------

fn request_board(id: &str, team_id: &str, board_type: BoardType, title: &str) -> Board {
    Board {
        id: id.to_owned(),
        ..new_board(team_id, board_type, title)
    }
}

fn request_block(id: &str, board_id: &str, title: &str) -> Block {
    Block {
        id: id.to_owned(),
        board_id: board_id.to_owned(),
        title: title.to_owned(),
        block_type: BlockType::Card,
        create_at: 1,
        update_at: 1,
        ..Block::default()
    }
}

/// `th.Server.App().InsertBlock(&model.Block{ID, BoardID, Title}, userID)`, under a new
/// identifier.
async fn stored_block(th: &TestServer, board_id: &str, user_id: &str) -> Block {
    insert_block(
        th,
        Block {
            id: new_id(),
            board_id: board_id.to_owned(),
            title: "initial title".to_owned(),
            ..Block::default()
        },
        user_id,
    )
    .await
}

async fn blocks_of(th: &TestServer, board_id: &str) -> Vec<Block> {
    th.app
        .store()
        .get_blocks(&BlockFilter::board(board_id))
        .await
        .expect("the blocks are read")
}

fn title_patches(count: usize, title: &str) -> (Vec<BoardPatch>, Vec<BlockPatch>) {
    let board_patch = BoardPatch {
        title: Some(title.to_owned()),
        ..BoardPatch::default()
    };
    let block_patch = BlockPatch {
        title: Some(title.to_owned()),
        ..BlockPatch::default()
    };
    (vec![board_patch; count], vec![block_patch; count])
}

fn strings(ids: &[&str]) -> Vec<String> {
    ids.iter().map(|id| (*id).to_owned()).collect()
}

fn expect_bad_request(response: &ApiResponse, message: &str) {
    response.check_error(StatusCode::BAD_REQUEST, message);
}

/// Two open boards of user1, each with one block, as most subtests of
/// `TestPatchBoardsAndBlocks` set up. The first is in the test team, the second in
/// `second_team`, and has user1 as admin only if `second` says so.
struct TwoBoards {
    board1: Board,
    board2: Board,
    block1: Block,
    block2: Block,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SecondBoard {
    WithAdmin,
    WithoutMembers,
}

async fn two_boards(th: &TestServer, second: SecondBoard, second_team: &str) -> TwoBoards {
    let user_id = th.client.me().await.id;
    let board1 = board_with_admin(
        th,
        new_board(TEST_TEAM_ID, BoardType::Open, "initial title"),
        &user_id,
    )
    .await;
    let board2 = new_board(second_team, BoardType::Open, "initial title");
    let board2 = match second {
        SecondBoard::WithAdmin => board_with_admin(th, board2, &user_id).await,
        SecondBoard::WithoutMembers => board_without_members(th, board2, &user_id).await,
    };
    let block1 = stored_block(th, &board1.id, &user_id).await;
    let block2 = stored_block(th, &board2.id, &user_id).await;
    TwoBoards {
        board1,
        board2,
        block1,
        block2,
    }
}

async fn assert_titles_unchanged(th: &TestServer, boards: &TwoBoards) {
    for board in [&boards.board1, &boards.board2] {
        let stored = th.app.board(&board.id).await.expect("the board exists");
        assert_eq!(stored.title, "initial title", "board {}", board.id);
    }
    for block in [&boards.block1, &boards.block2] {
        let stored = th
            .app
            .store()
            .get_block(&block.id)
            .await
            .expect("the block exists");
        assert_eq!(stored.title, "initial title", "block {}", block.id);
    }
}

// -- TestCreateBoardsAndBlocks --------------------------------------------------------

/// `TestCreateBoardsAndBlocks/"a non authenticated user should be rejected"`
#[tokio::test]
async fn create_boards_and_blocks_a_non_authenticated_user_should_be_rejected() {
    let th = Setup::native().start().await;

    let response = th.client.post(PATH, &BoardsAndBlocks::default()).await;

    response.check_unauthorized();
    th.teardown().await;
}

/// `TestCreateBoardsAndBlocks/"invalid boards and blocks"/"no boards"`
#[tokio::test]
async fn create_boards_and_blocks_no_boards() {
    let th = Setup::native().start().await.init_basic().await;
    let request = BoardsAndBlocks {
        boards: vec![],
        blocks: vec![Block {
            create_at: 0,
            update_at: 0,
            ..request_block("block-id", "board-id", "")
        }],
    };

    let response = th.client.post(PATH, &request).await;

    expect_bad_request(&response, "at least one board is required");
    th.teardown().await;
}

/// `TestCreateBoardsAndBlocks/"invalid boards and blocks"/"no blocks"`
#[tokio::test]
async fn create_boards_and_blocks_no_blocks() {
    let th = Setup::native().start().await.init_basic().await;
    let request = BoardsAndBlocks {
        boards: vec![request_board(
            "board-id",
            TEST_TEAM_ID,
            BoardType::Private,
            "",
        )],
        blocks: vec![],
    };

    let response = th.client.post(PATH, &request).await;

    expect_bad_request(&response, "at least one block is required");
    th.teardown().await;
}

/// `TestCreateBoardsAndBlocks/"invalid boards and blocks"/"blocks from nonexistent
/// boards"`
#[tokio::test]
async fn create_boards_and_blocks_blocks_from_nonexistent_boards() {
    let th = Setup::native().start().await.init_basic().await;
    let request = BoardsAndBlocks {
        boards: vec![request_board(
            "board-id",
            TEST_TEAM_ID,
            BoardType::Private,
            "",
        )],
        blocks: vec![request_block("block-id", "nonexistent-board-id", "")],
    };

    let response = th.client.post(PATH, &request).await;

    expect_bad_request(
        &response,
        "invalid boardId nonexistent-board-id (no board of the request has that id)",
    );
    assert_eq!(response.refused_at(), "/blocks/0/boardId");
    th.teardown().await;
}

/// `TestCreateBoardsAndBlocks/"invalid boards and blocks"/"boards with no IDs"`
#[tokio::test]
async fn create_boards_and_blocks_boards_with_no_ids() {
    let th = Setup::native().start().await.init_basic().await;
    let request = BoardsAndBlocks {
        boards: vec![
            request_board("board-id", TEST_TEAM_ID, BoardType::Private, ""),
            request_board("", TEST_TEAM_ID, BoardType::Private, ""),
        ],
        blocks: vec![request_block("block-id", "board-id", "")],
    };

    let response = th.client.post(PATH, &request).await;

    expect_bad_request(
        &response,
        "boards need an ID to be referenced from the blocks",
    );
    th.teardown().await;
}

/// `TestCreateBoardsAndBlocks/"invalid boards and blocks"/"boards from different teams"`
#[tokio::test]
async fn create_boards_and_blocks_boards_from_different_teams() {
    let th = Setup::native().start().await.init_basic().await;
    let request = BoardsAndBlocks {
        boards: vec![
            request_board("board-id-1", TEST_TEAM_ID, BoardType::Private, ""),
            request_board("board-id-2", ROOT_TEAM_ID, BoardType::Private, ""),
        ],
        blocks: vec![request_block("block-id", "board-id-1", "")],
    };

    let response = th.client.post(PATH, &request).await;

    expect_bad_request(&response, "cannot create boards for multiple teams");
    th.teardown().await;
}

/// `TestCreateBoardsAndBlocks/"invalid boards and blocks"/"creating boards and blocks"`
#[tokio::test]
async fn create_boards_and_blocks_creating_boards_and_blocks() {
    let th = Setup::native().start().await.init_basic().await;
    let user1 = th.client.me().await;
    let request = BoardsAndBlocks {
        boards: vec![
            request_board("board-id-1", TEST_TEAM_ID, BoardType::Open, "public board"),
            request_board(
                "board-id-2",
                TEST_TEAM_ID,
                BoardType::Private,
                "private board",
            ),
        ],
        blocks: vec![
            request_block("block-id-1", "board-id-1", "block 1"),
            request_block("block-id-2", "board-id-2", "block 2"),
        ],
    };

    let created: BoardsAndBlocks = th.client.post(PATH, &request).await.created_json();

    assert_eq!(created.boards.len(), 2, "two boards");
    assert_eq!(created.blocks.len(), 2, "two blocks");

    // Each board should have been created with a new ID, and its block should be there
    // too.
    for (term, title, board_type, old_id, block_title) in [
        (
            "public",
            "public board",
            BoardType::Open,
            "board-id-1",
            "block 1",
        ),
        (
            "private",
            "private board",
            BoardType::Private,
            "board-id-2",
            "block 2",
        ),
    ] {
        let found = th
            .app
            .store()
            .search_boards_for_user_in_team(TEST_TEAM_ID, term, &user1.id)
            .await
            .expect("the boards are searched");
        assert_eq!(found.len(), 1, "one board matches {term:?}: {found:?}");
        let board = &found[0];
        assert_eq!(board.title, title, "the title");
        assert_eq!(board.board_type, board_type, "the type");
        assert_ne!(board.id, old_id, "a new identifier");
        assert!(
            created.boards.contains(board),
            "the answer has the board as stored"
        );
        let blocks = blocks_of(&th, &board.id).await;
        assert_eq!(blocks.len(), 1, "one block: {blocks:?}");
        assert_eq!(blocks[0].title, block_title, "the block's title");
        assert_ne!(blocks[0].id, old_id.replace("board", "block"), "a new id");

        // user should be an admin of both newly created boards
        let members = members_of(&th, &board.id).await;
        assert_eq!(
            members,
            vec![BoardMember {
                scheme_admin: true,
                scheme_editor: true,
                ..member(&board.id, &user1.id)
            }],
            "the creator administers the board"
        );
    }

    // Both boards are filed under the creator's default category.
    let categories = th
        .app
        .user_category_boards(&user1.id, TEST_TEAM_ID)
        .await
        .expect("the categories are read");
    let default = categories
        .iter()
        .find(|entry| entry.category.name == "Boards")
        .expect("the default category exists");
    assert_eq!(default.board_metadata.len(), 2, "both boards are filed");

    th.teardown().await;
}

/// A card as a request names it, with the times the route insists on.
fn card_json(board_id: &str) -> Value {
    json!({"id": "b1", "boardId": board_id, "type": "card", "createAt": 1, "updateAt": 1})
}

/// The boards of the test team that a client's user sees.
async fn team_boards(client: &Client) -> Vec<Board> {
    client
        .get(&format!("/w/{TEST_TEAM_ID}/boards"))
        .await
        .ok_json()
}

/// What `POST /boards-and-blocks` refuses beyond what the original's tests cover.
#[tokio::test]
async fn create_boards_and_blocks_refusals() {
    let th = Setup::native().start().await.init_basic().await;
    let board = json!({"id": "bd", "teamId": TEST_TEAM_ID, "type": "P"});

    // A body that is no JSON object is a bad request. (The original answers 500.)
    for body in ["{bad", "", "null", "[]"] {
        let response = th.client.send_raw_body(Method::POST, PATH, body).await;
        response.check_bad_request();
        assert_eq!(response.problem_type(), "/problems/bad-request", "{body}");
    }

    // Every board needs an identifier for the blocks to name, the first one too. (The
    // original does not check the first board at all.)
    for boards in [
        json!([{"teamId": TEST_TEAM_ID, "type": "P", "title": "no id"}]),
        json!([board, {"teamId": TEST_TEAM_ID, "type": "P"}]),
    ] {
        let response = th
            .client
            .post(PATH, &json!({"boards": boards, "blocks": [card_json("")]}))
            .await;
        expect_bad_request(
            &response,
            "boards need an ID to be referenced from the blocks",
        );
    }

    // Blocks are checked one by one: type, creation time, modification time, board.
    // Each refusal names the member in the one spelling the API has.
    for (block, field, message) in [
        (
            json!({"id": "b1", "boardId": "bd", "createAt": 1, "updateAt": 1}),
            "type",
            "missing type for block id b1",
        ),
        (
            json!({"id": "b1", "boardId": "bd", "type": "card", "updateAt": 1}),
            "createAt",
            "invalid createAt for block id b1",
        ),
        (
            json!({"id": "b1", "boardId": "bd", "type": "card", "createAt": 1}),
            "updateAt",
            "invalid updateAt for block id b1",
        ),
    ] {
        let response = th
            .client
            .post(PATH, &json!({"boards": [board], "blocks": [block]}))
            .await;
        expect_bad_request(&response, message);
        assert_eq!(
            response.refused_at(),
            format!("/blocks/0/{field}"),
            "{message}"
        );
    }

    // No team at all, a team that does not exist, and text that is no identifier: the
    // boards' team must exist. (The original's team permission check fails on no team,
    // and it creates boards in a team of any other name.)
    for board in [
        json!({"id": "bd", "type": "P"}),
        json!({"id": "bd", "teamId": UNKNOWN_ID, "type": "P"}),
        json!({"id": "bd", "teamId": "nope", "type": "P"}),
    ] {
        let response = th
            .client
            .post(
                PATH,
                &json!({"boards": [board], "blocks": [card_json("bd")]}),
            )
            .await;
        expect_bad_request(&response, "no such team");
        assert_eq!(
            response.refused_at(),
            "/boards",
            "the member at fault: {board}"
        );
    }

    // A block that fails the store's validation: a bad request that names the block by
    // its new identifier. Nothing is stored.
    let mut long_title = card_json("bd");
    long_title["title"] = json!("x".repeat(70_000));
    let response = th
        .client
        .post(PATH, &json!({"boards": [board], "blocks": [long_title]}))
        .await;
    response.check_bad_request();
    assert!(
        response
            .error_message()
            .ends_with(": block title size limit exceeded"),
        "{}",
        response.error_message()
    );

    assert_eq!(team_boards(&th.client).await, vec![], "nothing was created");
    th.teardown().await;
}

/// What `POST /boards-and-blocks` accepts beyond what the original's tests cover.
#[tokio::test]
async fn create_boards_and_blocks_accepts_a_template() {
    let th = Setup::native().start().await.init_basic().await;
    let user1 = th.client.me().await;

    // A template is created like a board but filed under no category.
    let created: BoardsAndBlocks = th
        .client
        .post(
            PATH,
            &json!({
                "boards": [{"id": "bd", "teamId": TEST_TEAM_ID, "type": "O", "isTemplate": true}],
                "blocks": [card_json("bd")],
            }),
        )
        .await
        .created_json();
    assert!(created.boards[0].is_template, "a template");
    let categories = th
        .app
        .user_category_boards(&user1.id, TEST_TEAM_ID)
        .await
        .expect("the categories are read");
    assert!(
        !categories
            .iter()
            .flat_map(|entry| &entry.board_metadata)
            .any(|metadata| metadata.board_id == created.boards[0].id),
        "the template is in no category"
    );

    th.teardown().await;
}

/// A deliberate difference: the original stores boards of any type and minimum role that
/// come through this route, because only `POST /boards` validates them. The port has no
/// way to store them and refuses them with the messages of `POST /boards`.
#[tokio::test]
async fn create_boards_and_blocks_refuses_unknown_board_types_and_roles() {
    let th = Setup::native().start().await.init_basic().await;
    let block = card_json("bd");

    for (board, message) in [
        (
            json!({"id": "bd", "teamId": TEST_TEAM_ID, "type": "invalid"}),
            "invalid-board-type",
        ),
        (
            json!({"id": "bd", "teamId": TEST_TEAM_ID}),
            "invalid-board-type",
        ),
        (
            json!({"id": "bd", "teamId": TEST_TEAM_ID, "type": "O", "minimumRole": "boss"}),
            "invalid-board-minimum-role",
        ),
    ] {
        let response = th
            .client
            .post(PATH, &json!({"boards": [board], "blocks": [block]}))
            .await;
        expect_bad_request(&response, message);
    }

    assert_eq!(team_boards(&th.client).await, vec![], "nothing was created");
    th.teardown().await;
}

/// Creating boards and blocks tells the creator about each board, block and membership
/// (`server/app/boards_and_blocks.go:28-45`).
#[tokio::test]
async fn create_boards_and_blocks_is_broadcast() {
    let th = Setup::native().start().await.init_basic().await;
    let user1 = th.client.me().await;
    let mut socket = TeamSocket::subscribe(&th, &th.client, TEST_TEAM_ID, &user1.id).await;
    let request = BoardsAndBlocks {
        boards: vec![request_board(
            "board-id-1",
            TEST_TEAM_ID,
            BoardType::Open,
            "public board",
        )],
        blocks: vec![request_block("block-id-1", "board-id-1", "block 1")],
    };

    let created: BoardsAndBlocks = th.client.post(PATH, &request).await.created_json();

    let board_id = &created.boards[0].id;
    // The board and block messages are sent before the creator is a member in the eyes
    // of the hub only if the membership were written later; it is written with the
    // board, so all three arrive.
    socket.expect("UPDATE_BOARD", "/board/id", board_id).await;
    let message = socket
        .expect("UPDATE_BLOCK", "/block/id", &created.blocks[0].id)
        .await;
    assert_eq!(message["block"]["title"], json!("block 1"), "{message}");
    let message = socket
        .expect("UPDATE_MEMBER", "/member/boardId", board_id)
        .await;
    assert_eq!(message["member"]["userId"], json!(user1.id), "{message}");
    th.teardown().await;
}

// -- TestPatchBoardsAndBlocks ---------------------------------------------------------

async fn patch(client: &Client, request: &impl serde::Serialize) -> ApiResponse {
    client.patch(PATH, request).await
}

/// `TestPatchBoardsAndBlocks/"a non authenticated user should be rejected"`
#[tokio::test]
async fn patch_boards_and_blocks_a_non_authenticated_user_should_be_rejected() {
    let th = Setup::native().start().await;

    let response = patch(&th.client, &PatchBoardsAndBlocks::default()).await;

    response.check_unauthorized();
    th.teardown().await;
}

/// `TestPatchBoardsAndBlocks/"invalid patch boards and blocks"/"no board IDs"`
#[tokio::test]
async fn patch_boards_and_blocks_no_board_ids() {
    let th = Setup::native().start().await.init_basic().await;
    let boards = two_boards(&th, SecondBoard::WithAdmin, TEST_TEAM_ID).await;
    let (board_patches, block_patches) = title_patches(2, "new title 1");
    let request = PatchBoardsAndBlocks {
        board_ids: vec![],
        board_patches,
        block_ids: strings(&[&boards.block1.id, &boards.block2.id]),
        block_patches,
    };

    let response = patch(&th.client, &request).await;

    expect_bad_request(&response, "at least one board is required");
    assert_titles_unchanged(&th, &boards).await;
    th.teardown().await;
}

/// `TestPatchBoardsAndBlocks/"invalid patch boards and blocks"/"missmatch board IDs and
/// patches"`
#[tokio::test]
async fn patch_boards_and_blocks_missmatch_board_ids_and_patches() {
    let th = Setup::native().start().await.init_basic().await;
    let boards = two_boards(&th, SecondBoard::WithAdmin, TEST_TEAM_ID).await;
    let (mut board_patches, block_patches) = title_patches(2, "new title 1");
    board_patches.truncate(1);
    let request = PatchBoardsAndBlocks {
        board_ids: strings(&[&boards.board1.id, &boards.board2.id]),
        board_patches,
        block_ids: strings(&[&boards.block1.id, &boards.block2.id]),
        block_patches,
    };

    let response = patch(&th.client, &request).await;

    expect_bad_request(&response, "board ids and patches need to match");
    assert_titles_unchanged(&th, &boards).await;
    th.teardown().await;
}

/// `TestPatchBoardsAndBlocks/"invalid patch boards and blocks"/"no block IDs"`
#[tokio::test]
async fn patch_boards_and_blocks_no_block_ids() {
    let th = Setup::native().start().await.init_basic().await;
    let boards = two_boards(&th, SecondBoard::WithAdmin, TEST_TEAM_ID).await;
    let (board_patches, block_patches) = title_patches(2, "new title 1");
    let request = PatchBoardsAndBlocks {
        board_ids: strings(&[&boards.board1.id, &boards.board2.id]),
        board_patches,
        block_ids: vec![],
        block_patches,
    };

    let response = patch(&th.client, &request).await;

    expect_bad_request(&response, "block ids and patches need to match");
    assert_titles_unchanged(&th, &boards).await;
    th.teardown().await;
}

/// `TestPatchBoardsAndBlocks/"invalid patch boards and blocks"/"missmatch block IDs and
/// patches"`
#[tokio::test]
async fn patch_boards_and_blocks_missmatch_block_ids_and_patches() {
    let th = Setup::native().start().await.init_basic().await;
    let boards = two_boards(&th, SecondBoard::WithAdmin, TEST_TEAM_ID).await;
    let (board_patches, mut block_patches) = title_patches(2, "new title 1");
    block_patches.truncate(1);
    let request = PatchBoardsAndBlocks {
        board_ids: strings(&[&boards.board1.id, &boards.board2.id]),
        board_patches,
        block_ids: strings(&[&boards.block1.id, &boards.block2.id]),
        block_patches,
    };

    let response = patch(&th.client, &request).await;

    expect_bad_request(&response, "block ids and patches need to match");
    assert_titles_unchanged(&th, &boards).await;
    th.teardown().await;
}

/// `TestPatchBoardsAndBlocks/"invalid patch boards and blocks"/"block that doesn't belong
/// to any board"`
#[tokio::test]
async fn patch_boards_and_blocks_block_that_doesnt_belong_to_any_board() {
    let th = Setup::native().start().await.init_basic().await;
    let boards = two_boards(&th, SecondBoard::WithAdmin, TEST_TEAM_ID).await;
    let (mut board_patches, block_patches) = title_patches(2, "new title 1");
    board_patches.truncate(1);
    let request = PatchBoardsAndBlocks {
        board_ids: strings(&[&boards.board1.id]),
        board_patches,
        block_ids: strings(&[&boards.block1.id, &boards.block2.id]),
        block_patches,
    };

    let response = patch(&th.client, &request).await;

    expect_bad_request(
        &response,
        "the request names a block that is not on its boards",
    );
    assert_titles_unchanged(&th, &boards).await;
    th.teardown().await;
}

/// `TestPatchBoardsAndBlocks/"if the user doesn't have permissions for one of the boards,
/// nothing should be updated"`
#[tokio::test]
async fn patch_boards_and_blocks_without_permissions_for_one_board_nothing_is_updated() {
    let th = Setup::native().start().await.init_basic().await;
    let boards = two_boards(&th, SecondBoard::WithoutMembers, TEST_TEAM_ID).await;
    let (board_patches, block_patches) = title_patches(2, "new title 2");
    let request = PatchBoardsAndBlocks {
        board_ids: strings(&[&boards.board1.id, &boards.board2.id]),
        board_patches,
        block_ids: strings(&[&boards.block1.id, &boards.block2.id]),
        block_patches,
    };

    let response = patch(&th.client, &request).await;

    response.check_forbidden();
    assert_eq!(
        response.error_message(),
        "access denied to modifying board properties",
        "why"
    );
    assert_titles_unchanged(&th, &boards).await;
    th.teardown().await;
}

/// `TestPatchBoardsAndBlocks/"boards belonging to different teams should be rejected"`
#[tokio::test]
async fn patch_boards_and_blocks_boards_belonging_to_different_teams_should_be_rejected() {
    let th = Setup::native().start().await.init_basic().await;
    let boards = two_boards(&th, SecondBoard::WithAdmin, ROOT_TEAM_ID).await;
    let (board_patches, block_patches) = title_patches(2, "new title 3");
    let request = PatchBoardsAndBlocks {
        board_ids: strings(&[&boards.board1.id, &boards.board2.id]),
        board_patches,
        block_ids: strings(&[&boards.block1.id, &boards.block2.id]),
        block_patches,
    };

    let response = patch(&th.client, &request).await;

    expect_bad_request(&response, "mismatched team ID");
    assert_titles_unchanged(&th, &boards).await;
    th.teardown().await;
}

/// `TestPatchBoardsAndBlocks/"patches should be rejected if one is invalid"`
#[tokio::test]
async fn patch_boards_and_blocks_patches_should_be_rejected_if_one_is_invalid() {
    let th = Setup::native().start().await.init_basic().await;
    let boards = two_boards(&th, SecondBoard::WithoutMembers, TEST_TEAM_ID).await;
    let request = json!({
        "boardIds": [boards.board1.id, boards.board2.id],
        "boardPatches": [{"title": "new title 4"}, {"type": "invalid"}],
        "blockIds": [boards.block1.id, boards.block2.id],
        "blockPatches": [{"title": "new title 4"}, {"title": "new title 4"}],
    });

    let response = patch(&th.client, &request).await;

    expect_bad_request(&response, "invalid-board-type");
    assert_titles_unchanged(&th, &boards).await;

    // Verified on the original: the boards are checked in order, each for validity and
    // then for permissions, so with the boards swapped the missing permission comes
    // first.
    let swapped = json!({
        "boardIds": [boards.board2.id, boards.board1.id],
        "boardPatches": [{"title": "new title 4"}, {"minimumRole": "boss"}],
        "blockIds": [],
        "blockPatches": [],
    });
    patch(&th.client, &swapped).await.check_forbidden();
    let role = json!({
        "boardIds": [boards.board1.id],
        "boardPatches": [{"minimumRole": "boss"}],
        "blockIds": [],
        "blockPatches": [],
    });
    expect_bad_request(
        &patch(&th.client, &role).await,
        "invalid-board-minimum-role",
    );

    th.teardown().await;
}

/// `TestPatchBoardsAndBlocks/"patches should be rejected if there is a block that doesn't
/// belong to the boards being patched"`
#[tokio::test]
async fn patch_boards_and_blocks_rejects_a_block_of_a_board_that_is_not_patched() {
    let th = Setup::native().start().await.init_basic().await;
    let boards = two_boards(&th, SecondBoard::WithAdmin, TEST_TEAM_ID).await;
    let (mut board_patches, block_patches) = title_patches(2, "new patched title");
    board_patches.truncate(1);
    let request = PatchBoardsAndBlocks {
        board_ids: strings(&[&boards.board1.id]),
        board_patches,
        block_ids: strings(&[&boards.block1.id, &boards.block2.id]),
        block_patches,
    };

    let response = patch(&th.client, &request).await;

    response.check_bad_request();
    assert_titles_unchanged(&th, &boards).await;
    th.teardown().await;
}

/// `TestPatchBoardsAndBlocks/"patches should be applied if they're valid and they're
/// related"`
#[tokio::test]
async fn patch_boards_and_blocks_patches_should_be_applied_if_valid_and_related() {
    let th = Setup::native().start().await.init_basic().await;
    let user1 = th.client.me().await;
    let boards = two_boards(&th, SecondBoard::WithAdmin, TEST_TEAM_ID).await;
    let new_title = "new other title";
    let (board_patches, block_patches) = title_patches(2, new_title);
    let request = PatchBoardsAndBlocks {
        board_ids: strings(&[&boards.board1.id, &boards.board2.id]),
        board_patches,
        block_ids: strings(&[&boards.block1.id, &boards.block2.id]),
        block_patches,
    };
    let mut socket = TeamSocket::subscribe(&th, &th.client, TEST_TEAM_ID, &user1.id).await;

    let patched: BoardsAndBlocks = patch(&th.client, &request).await.ok_json();

    assert_eq!(patched.boards.len(), 2, "two boards");
    assert_eq!(patched.blocks.len(), 2, "two blocks");
    assert_eq!(
        patched
            .boards
            .iter()
            .map(|board| board.id.as_str())
            .collect::<Vec<_>>(),
        vec![boards.board1.id.as_str(), boards.board2.id.as_str()],
        "in the order of the request"
    );

    // ensure that the entities have been updated
    for board in [&boards.board1, &boards.board2] {
        let stored = th.app.board(&board.id).await.expect("the board exists");
        assert_eq!(stored.title, new_title, "board {}", board.id);
        assert!(patched.boards.contains(&stored), "answered as stored");
    }
    for block in [&boards.block1, &boards.block2] {
        let stored = th
            .app
            .store()
            .get_block(&block.id)
            .await
            .expect("the block exists");
        assert_eq!(stored.title, new_title, "block {}", block.id);
        assert_eq!(stored.modified_by, user1.id, "modified by the user");
    }

    // The members are told about every block and board.
    for block in [&boards.block1, &boards.block2] {
        let message = socket.expect("UPDATE_BLOCK", "/block/id", &block.id).await;
        assert_eq!(message["block"]["title"], json!(new_title), "{message}");
    }
    for board in [&boards.board1, &boards.board2] {
        let message = socket.expect("UPDATE_BOARD", "/board/id", &board.id).await;
        assert_eq!(message["board"]["title"], json!(new_title), "{message}");
    }

    th.teardown().await;
}

/// A body of `PATCH /boards-and-blocks` that cannot be decoded is a bad request. (The
/// original answers 500.)
#[tokio::test]
async fn patch_boards_and_blocks_undecodable_bodies_are_bad_requests() {
    let th = Setup::native().start().await.init_basic().await;
    let boards = two_boards(&th, SecondBoard::WithAdmin, TEST_TEAM_ID).await;
    let first_id = &boards.board1.id;

    let null_patch = format!("{{\"boardIds\":[\"{first_id}\"],\"boardPatches\":[null]}}");
    let number_type =
        format!("{{\"boardIds\":[\"{first_id}\"],\"boardPatches\":[{{\"type\":5}}]}}");
    for body in ["{bad", "null", "[]", &null_patch, &number_type] {
        let response = th.client.send_raw_body(Method::PATCH, PATH, body).await;
        response.check_bad_request();
        assert_eq!(response.problem_type(), "/problems/bad-request", "{body}");
    }

    // An empty object is decoded, and then found to list no boards.
    expect_bad_request(
        &patch(&th.client, &json!({})).await,
        "at least one board is required",
    );
    th.teardown().await;
}

/// The order of the checks of `PATCH /boards-and-blocks` and what it answers to
/// requests the original's tests do not cover.
#[tokio::test]
async fn patch_boards_and_blocks_checks() {
    let th = Setup::native().start().await.init_basic().await;
    let user2 = th.client2.me().await;
    let boards = two_boards(&th, SecondBoard::WithAdmin, TEST_TEAM_ID).await;
    let first_id = &boards.board1.id;

    // A board that does not exist, and a private board of somebody else, answer alike.
    let private = th.create_board(TEST_TEAM_ID, BoardType::Private).await;
    for board_id in [UNKNOWN_ID, "nonexistent", private.id.as_str()] {
        let response = patch(
            &th.client2,
            &json!({"boardIds": [board_id], "boardPatches": [{"title": "n"}]}),
        )
        .await;
        response.check_error(StatusCode::NOT_FOUND, "board not found");
    }

    // A block that does not exist.
    for block_id in [UNKNOWN_ID, "nonexistent"] {
        let response = patch(
            &th.client,
            &json!({
                "boardIds": [first_id], "boardPatches": [{"title": "n"}],
                "blockIds": [block_id], "blockPatches": [{"title": "n"}],
            }),
        )
        .await;
        expect_bad_request(
            &response,
            "the request names a block that is not on its boards",
        );
    }

    // An editor patches board properties and blocks, but not the board type.
    th.client
        .add_member(&BoardMember {
            scheme_editor: true,
            ..member(first_id, &user2.id)
        })
        .await
        .check_ok();
    let response = patch(
        &th.client2,
        &json!({
            "boardIds": [first_id], "boardPatches": [{"type": "P"}],
            "blockIds": [boards.block1.id], "blockPatches": [{"title": "n"}],
        }),
    )
    .await;
    response.check_forbidden();
    assert_eq!(
        response.error_message(),
        "access denied to modifying board type",
        "why"
    );
    assert_titles_unchanged(&th, &boards).await;

    // Boards only, without the block members: the answer has an empty list of blocks.
    // (The original answers `"blocks": null`; the model sends empty lists.)
    let response = patch(
        &th.client2,
        &json!({"boardIds": [first_id], "boardPatches": [{"description": "by the editor"}]}),
    )
    .await;
    let value = response.value();
    response.check_ok();
    assert_eq!(value["blocks"], json!([]), "no blocks");
    assert_eq!(
        value["boards"][0]["description"],
        json!("by the editor"),
        "{value}"
    );
    assert_eq!(value["boards"][0]["modifiedBy"], json!(user2.id), "{value}");

    // The same board twice: both patches are applied, and the board is answered twice.
    let response = patch(
        &th.client,
        &json!({
            "boardIds": [first_id, first_id],
            "boardPatches": [{"title": "t1"}, {"icon": "i2"}],
        }),
    )
    .await;
    let value: Value = response.ok_json();
    assert_eq!(
        value["boards"][0]["icon"],
        json!(""),
        "after the first patch"
    );
    assert_eq!(value["boards"][1]["title"], json!("t1"), "after both");
    assert_eq!(value["boards"][1]["icon"], json!("i2"), "after both");

    // A commenter patches nothing.
    th.app
        .update_board_member(&BoardMember {
            scheme_commenter: true,
            ..member(first_id, &user2.id)
        })
        .await
        .expect("user2 becomes a commenter");
    patch(
        &th.client2,
        &json!({"boardIds": [first_id], "boardPatches": [{"title": "n"}]}),
    )
    .await
    .check_forbidden();

    th.teardown().await;
}

// -- TestDeleteBoardsAndBlocks --------------------------------------------------------

async fn delete(client: &Client, request: &impl serde::Serialize) -> ApiResponse {
    client.delete_json(PATH, request).await
}

/// `TestDeleteBoardsAndBlocks/"a non authenticated user should be rejected"`
#[tokio::test]
async fn delete_boards_and_blocks_a_non_authenticated_user_should_be_rejected() {
    let th = Setup::native().start().await;

    let response = delete(&th.client, &DeleteBoardsAndBlocks::default()).await;

    response.check_unauthorized();
    th.teardown().await;
}

/// `TestDeleteBoardsAndBlocks/"invalid delete boards and blocks"/"no boards"`
#[tokio::test]
async fn delete_boards_and_blocks_no_boards() {
    let th = Setup::native().start().await.init_basic().await;
    let user1 = th.client.me().await;
    // a board and a block are required for the permission checks
    let board =
        board_with_admin(&th, new_board(TEST_TEAM_ID, BoardType::Open, ""), &user1.id).await;
    let block = stored_block(&th, &board.id, &user1.id).await;
    let request = DeleteBoardsAndBlocks {
        boards: vec![],
        blocks: vec![block.id],
    };

    let response = delete(&th.client, &request).await;

    // That the request lists no board is found before anything is looked up. (The
    // original checks the block against the listed boards first.)
    expect_bad_request(&response, "at least one board is required");
    expect_bad_request(
        &delete(&th.client, &json!({})).await,
        "at least one board is required",
    );
    assert_eq!(blocks_of(&th, &board.id).await.len(), 1, "nothing deleted");
    th.teardown().await;
}

/// `TestDeleteBoardsAndBlocks/"invalid delete boards and blocks"/"boards from different
/// teams"`
#[tokio::test]
async fn delete_boards_and_blocks_boards_from_different_teams() {
    let th = Setup::native().start().await.init_basic().await;
    let user1 = th.client.me().await;
    let board =
        board_with_admin(&th, new_board(TEST_TEAM_ID, BoardType::Open, ""), &user1.id).await;
    let block = stored_block(&th, &board.id, &user1.id).await;
    let other_teams_board =
        board_with_admin(&th, new_board(ROOT_TEAM_ID, BoardType::Open, ""), &user1.id).await;
    let request = DeleteBoardsAndBlocks {
        boards: strings(&[&board.id, &other_teams_board.id]),
        blocks: vec![block.id],
    };

    let response = delete(&th.client, &request).await;

    expect_bad_request(&response, "all boards should belong to the same team");
    th.app.board(&board.id).await.expect("the board is there");
    assert_eq!(blocks_of(&th, &board.id).await.len(), 1, "with its block");
    th.teardown().await;
}

/// `TestDeleteBoardsAndBlocks/"if the user has no permissions to one of the boards,
/// nothing should be deleted"`
#[tokio::test]
async fn delete_boards_and_blocks_without_permissions_to_one_board_nothing_is_deleted() {
    let th = Setup::native().start().await.init_basic().await;
    let user1 = th.client.me().await;
    // the user is an admin of the first board
    let board1 =
        board_with_admin(&th, new_board(TEST_TEAM_ID, BoardType::Open, ""), &user1.id).await;
    // but not of the second
    let board2 =
        board_without_members(&th, new_board(TEST_TEAM_ID, BoardType::Open, ""), &user1.id).await;
    // (The original names a block that does not exist; here the block is one of the
    // first board, so that the missing permission is all that is wrong.)
    let block = stored_block(&th, &board1.id, &user1.id).await;
    let request = DeleteBoardsAndBlocks {
        boards: strings(&[&board1.id, &board2.id]),
        blocks: vec![block.id],
    };

    let response = delete(&th.client, &request).await;

    response.check_forbidden();
    assert_eq!(
        response.error_message(),
        "access denied to delete board",
        "why"
    );
    th.app.board(&board1.id).await.expect("board 1 is there");
    th.app.board(&board2.id).await.expect("board 2 is there");
    assert_eq!(blocks_of(&th, &board1.id).await.len(), 1, "and the block");
    th.teardown().await;
}

/// `TestDeleteBoardsAndBlocks/"all boards and blocks should be deleted if the request is
/// correct"`
#[tokio::test]
async fn delete_boards_and_blocks_all_boards_and_blocks_should_be_deleted() {
    let th = Setup::native().start().await.init_basic().await;
    let user1 = th.client.me().await;
    // The application layer stores the boards and blocks under the identifiers it is
    // given.
    let board_ids = [new_id(), new_id()];
    let block_ids = [new_id(), new_id()];
    let request = BoardsAndBlocks {
        boards: vec![
            request_board(&board_ids[0], TEST_TEAM_ID, BoardType::Open, "public board"),
            request_board(
                &board_ids[1],
                TEST_TEAM_ID,
                BoardType::Private,
                "private board",
            ),
        ],
        blocks: vec![
            request_block(&block_ids[0], &board_ids[0], "block 1"),
            request_block(&block_ids[1], &board_ids[1], "block 2"),
        ],
    };
    let created = th
        .app
        .create_boards_and_blocks_with_admin(request, &user1.id)
        .await
        .expect("the boards and blocks are created");
    assert_eq!(created.boards.len(), 2, "two boards");
    assert_eq!(created.blocks.len(), 2, "two blocks");

    // ensure that the entities have been successfully created
    for board_id in &board_ids {
        th.app.board(board_id).await.expect("the board exists");
    }
    for block_id in &block_ids {
        th.app
            .store()
            .get_block(block_id)
            .await
            .expect("the block exists");
    }
    let mut socket = TeamSocket::subscribe(&th, &th.client, TEST_TEAM_ID, &user1.id).await;

    // call the API to delete boards and blocks
    let response = delete(
        &th.client,
        &DeleteBoardsAndBlocks {
            boards: board_ids.to_vec(),
            blocks: block_ids.to_vec(),
        },
    )
    .await;
    response.check_no_content();

    // ensure that the entities have been successfully deleted
    for board_id in &board_ids {
        let error = th.app.board(board_id).await.expect_err("the board is gone");
        assert!(error.is_not_found(), "{error:?}");
    }
    for block_id in &block_ids {
        let error = th
            .app
            .store()
            .get_block(block_id)
            .await
            .expect_err("the block is gone");
        assert!(error.is_not_found(), "{error:?}");
    }

    // Those who were members are told about every deleted block and board, although
    // the memberships went with the boards.
    for block_id in &block_ids {
        let message = socket.expect("UPDATE_BLOCK", "/block/id", block_id).await;
        assert!(
            message["block"]["deleteAt"].as_i64() > Some(0),
            "a deletion: {message}"
        );
    }
    for board_id in &board_ids {
        let message = socket.expect("UPDATE_BOARD", "/board/id", board_id).await;
        assert!(
            message["board"]["deleteAt"].as_i64() > Some(0),
            "a deletion: {message}"
        );
    }

    th.teardown().await;
}

/// The order of the checks of `DELETE /boards-and-blocks` and what it answers to
/// requests the original's tests do not cover.
#[tokio::test]
async fn delete_boards_and_blocks_checks() {
    let th = Setup::native().start().await.init_basic().await;
    let boards = two_boards(&th, SecondBoard::WithAdmin, TEST_TEAM_ID).await;
    let first_id = &boards.board1.id;

    // A body that is no JSON object is a bad request.
    for body in ["{bad", "[]", "null"] {
        let response = th.client.send_raw_body(Method::DELETE, PATH, body).await;
        response.check_bad_request();
        assert_eq!(response.problem_type(), "/problems/bad-request", "{body}");
    }

    for nothing in [UNKNOWN_ID, "nonexistent"] {
        let response = delete(&th.client, &json!({"boards": [nothing]})).await;
        response.check_error(StatusCode::NOT_FOUND, "board not found");

        expect_bad_request(
            &delete(&th.client, &json!({"boards": [], "blocks": [nothing]})).await,
            "at least one board is required",
        );
        // A block that does not exist answers as one of a board that is not listed.
        expect_bad_request(
            &delete(
                &th.client,
                &json!({"boards": [first_id], "blocks": [nothing]}),
            )
            .await,
            "the request names a block that is not on its boards",
        );
    }

    // A block of a board that is not listed.
    expect_bad_request(
        &delete(
            &th.client,
            &json!({"boards": [first_id], "blocks": [boards.block2.id]}),
        )
        .await,
        "the request names a block that is not on its boards",
    );

    // Somebody who is not a member.
    delete(
        &th.client2,
        &json!({"boards": [first_id], "blocks": [boards.block1.id]}),
    )
    .await
    .check_forbidden();
    assert_titles_unchanged(&th, &boards).await;

    // Boards only: the board goes, with its blocks.
    delete(&th.client, &json!({"boards": [first_id]}))
        .await
        .check_no_content();
    let error = th.app.board(first_id).await.expect_err("the board is gone");
    assert!(error.is_not_found(), "{error:?}");
    assert_eq!(blocks_of(&th, first_id).await, vec![], "and its block");
    th.app
        .board(&boards.board2.id)
        .await
        .expect("the other board is untouched");

    th.teardown().await;
}
