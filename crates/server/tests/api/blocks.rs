//! Ports of `server/integrationtests/blocks_test.go` and `server/app/blocks_test.go`,
//! plus tests of what `server/api/blocks.go` decides and the original leaves untested:
//! who may do what, the refusals and their messages, and who is told about a change.
//!
//! The subtests of the original share one server and build on each other; here every
//! subtest is a test of its own that first repeats the steps before it.
//!
//! Boards are created through the store by [`create_board_in_store`], because the board
//! routes belong to another area.
//!
//! Not ported from `server/app/blocks_test.go`:
//! - `TestPatchBlocks/"cloud limit error scenario"`, `TestInsertBlocks/"create view
//!   within limits"`, `/"create view exceeding limits"` and `/"creating multiple views,
//!   reaching limit in the process"`: the card and view limits of the hosted edition,
//!   which a standalone server never applies (the original skips these cases too).
//! - `TestUndeleteBlock/"error scenario"`: a failure injected into the store's undelete
//!   through a mock; the real store's undelete has no failure to provoke.

use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use futures_util::{SinkExt as _, StreamExt as _};
use fvoci_model::{
    BLOCK_FIELDS_MAX_CHARS, BLOCK_TITLE_MAX_CHARS, Block, BlockPatch, BlockPatchBatch, BlockType,
    Board, BoardMember, BoardType, CARD_FIELDS_MAX_CHARS, Category, JsonMap, new_id,
};
use fvoci_server::app::ChangeNotification;
use fvoci_server::error::ErrorKind;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

use crate::common::{Client, Setup, TEST_TEAM_ID, TestServer, UNKNOWN_ID};

/// How long to wait for a websocket message that should arrive.
const PATIENCE: Duration = Duration::from_secs(5);
/// How long to wait before deciding that a websocket message does not arrive.
const QUIET: Duration = Duration::from_millis(300);
/// How long background notifications may take to finish.
const BACKGROUND: Duration = Duration::from_secs(10);

/// The refusal for a user who may not manage the cards of a board.
const NO_CHANGES: &str = "access denied to make board changes";
/// The refusal for a user who may not comment on a board.
const NO_COMMENTS: &str = "access denied to post card comments";

// -- helpers --------------------------------------------------------------------------

/// What a user may do on a board they are added to in a test.
#[derive(Debug, Clone, Copy)]
enum Role {
    Editor,
    Commenter,
    Viewer,
}

/// Stores `board` with the first client's user as its administrator. The stand-in for
/// the original's `th.CreateBoard`, which goes through the board routes.
async fn store_board(th: &TestServer, mut board: Board) -> Board {
    let admin = th.client.me().await;
    board.id = new_id();
    th.app
        .store()
        .insert_board_with_admin(&mut board, &admin.id)
        .await
        .expect("the board is stored with the first user as admin");
    board
}

/// Creates an empty board in [`TEST_TEAM_ID`] through the store, with the first client's
/// user as its administrator.
async fn create_board_in_store(th: &TestServer, board_type: BoardType) -> Board {
    let board = Board {
        team_id: TEST_TEAM_ID.to_owned(),
        board_type,
        ..Board::default()
    };
    store_board(th, board).await
}

/// Makes the client's user a member of the board, through the store.
async fn add_member_in_store(th: &TestServer, board_id: &str, client: &Client, role: Role) {
    let member = BoardMember {
        board_id: board_id.to_owned(),
        user_id: client.me().await.id,
        scheme_editor: matches!(role, Role::Editor),
        scheme_commenter: matches!(role, Role::Commenter),
        scheme_viewer: matches!(role, Role::Viewer),
        ..BoardMember::default()
    };
    th.app
        .store()
        .save_member(&member)
        .await
        .expect("the membership is stored");
}

/// A private board of the first user (`th.client`, its administrator) with one more
/// member of each role, and a second private board only the first user is on.
/// `th.client2` is a member of neither.
struct Boards {
    board: Board,
    other_board: Board,
    editor: Client,
    commenter: Client,
    viewer: Client,
}

async fn boards_with_roles(th: &TestServer) -> Boards {
    let board = create_board_in_store(th, BoardType::Private).await;
    let other_board = create_board_in_store(th, BoardType::Private).await;
    let editor = th.add_user("editor").await;
    let commenter = th.add_user("commenter").await;
    let viewer = th.add_user("viewer").await;
    add_member_in_store(th, &board.id, &editor, Role::Editor).await;
    add_member_in_store(th, &board.id, &commenter, Role::Commenter).await;
    add_member_in_store(th, &board.id, &viewer, Role::Viewer).await;
    Boards {
        board,
        other_board,
        editor,
        commenter,
        viewer,
    }
}

/// A card of the first user with a text and a comment of the commenter on it, titled
/// by their types, and a card titled `other` on the other board.
struct Content {
    card: Block,
    text: Block,
    comment: Block,
    other_card: Block,
}

async fn card_with_content(th: &TestServer, boards: &Boards) -> Content {
    let board_id = &boards.board.id;
    let card = insert_one(&th.client, &new_block(board_id, BlockType::Card, "card")).await;
    let text = insert_one(&th.client, &child_of(&card, BlockType::Attachment, "text")).await;
    let comment = insert_one(
        &boards.commenter,
        &child_of(&card, BlockType::Comment, "comment"),
    )
    .await;
    let other_card = insert_one(
        &th.client,
        &new_block(&boards.other_board.id, BlockType::Card, "other"),
    )
    .await;
    Content {
        card,
        text,
        comment,
        other_card,
    }
}

/// A block as the original's tests send it: an identifier of the client's own and both
/// times set to 1.
fn new_block(board_id: &str, block_type: BlockType, title: &str) -> Block {
    Block {
        id: new_id(),
        board_id: board_id.to_owned(),
        create_at: 1,
        update_at: 1,
        block_type,
        title: title.to_owned(),
        ..Block::default()
    }
}

fn child_of(parent: &Block, block_type: BlockType, title: &str) -> Block {
    Block {
        parent_id: parent.id.clone(),
        ..new_block(&parent.board_id, block_type, title)
    }
}

fn blocks_path(board_id: &str) -> String {
    format!("/boards/{board_id}/blocks")
}

fn block_path(board_id: &str, block_id: &str) -> String {
    format!("/boards/{board_id}/blocks/{block_id}")
}

fn undelete_path(board_id: &str, block_id: &str) -> String {
    format!("{}/undelete", block_path(board_id, block_id))
}

fn duplicate_path(board_id: &str, block_id: &str) -> String {
    format!("{}/duplicate", block_path(board_id, block_id))
}

/// What a board answers that is not there, or that the user cannot see.
const NO_BOARD: &str = "board not found";

/// What a block answers that is not there, or that is on another board than the path's.
const NO_BLOCK: &str = "block not found";

/// `InsertBlocks`, expecting success.
async fn insert(client: &Client, board_id: &str, blocks: &[Block]) -> Vec<Block> {
    client.insert_blocks(board_id, blocks).await.created_json()
}

/// Inserts one block and returns it as the server answered.
async fn insert_one(client: &Client, block: &Block) -> Block {
    insert(client, &block.board_id, std::slice::from_ref(block))
        .await
        .pop()
        .expect("one block was inserted")
}

/// `GetBlocksForBoard`.
async fn blocks_of(client: &Client, board_id: &str) -> Vec<Block> {
    client.get(&blocks_path(board_id)).await.ok_json()
}

/// `PatchBlock`, expecting success.
async fn patch(client: &Client, block: &Block, patch: &BlockPatch) {
    let response = client
        .patch(&block_path(&block.board_id, &block.id), patch)
        .await;
    response.check_no_content();
}

/// `DeleteBlock`, expecting success.
async fn delete(client: &Client, block: &Block) {
    let response = client.delete(&block_path(&block.board_id, &block.id)).await;
    response.check_no_content();
}

fn ids(blocks: &[Block]) -> Vec<&str> {
    blocks.iter().map(|block| block.id.as_str()).collect()
}

/// The titles of the blocks, sorted, because lists of blocks have no defined order.
fn titles(blocks: &[Block]) -> Vec<&str> {
    let mut titles: Vec<&str> = blocks.iter().map(|block| block.title.as_str()).collect();
    titles.sort_unstable();
    titles
}

fn find<'a>(blocks: &'a [Block], block_id: &str) -> &'a Block {
    blocks
        .iter()
        .find(|block| block.id == block_id)
        .unwrap_or_else(|| panic!("block {block_id} is among {:?}", ids(blocks)))
}

fn title_patch(title: &str) -> BlockPatch {
    BlockPatch {
        title: Some(title.to_owned()),
        ..BlockPatch::default()
    }
}

/// A batch that gives each block the title next to it.
fn batch(patches: &[(&Block, &str)]) -> BlockPatchBatch {
    BlockPatchBatch {
        block_ids: patches.iter().map(|(block, _)| block.id.clone()).collect(),
        block_patches: patches
            .iter()
            .map(|(_, title)| title_patch(title))
            .collect(),
    }
}

fn fields(value: Value) -> JsonMap {
    serde_json::from_value(value).expect("the fields are a JSON object")
}

/// A client connected to the websocket endpoint.
struct Socket {
    stream: WebSocketStream<MaybeTlsStream<TcpStream>>,
}

impl Socket {
    /// Connects, authenticates as the client's user and subscribes to `team_id`;
    /// returns once the server has acted on the subscription. Commands have no
    /// acknowledgement, so the wait is for a broadcast to the user to come through.
    async fn subscribed_to_team(th: &TestServer, client: &Client, team_id: &str) -> Self {
        let stream = client.connect_websocket().await;
        let mut socket = Self { stream };
        socket
            .send(json!({"action": "SUBSCRIBE_TEAM", "teamId": team_id}))
            .await;

        let probe = Category {
            id: new_id(),
            name: "subscription probe".to_owned(),
            user_id: client.me().await.id,
            team_id: team_id.to_owned(),
            ..Category::default()
        };
        for _ in 0..50 {
            th.app.ws().broadcast_category_change(&probe);
            if socket.next_json(Duration::from_millis(100)).await.is_some() {
                // Later probes may still be queued; let them drain.
                while socket.next_json(QUIET).await.is_some() {}
                return socket;
            }
        }
        panic!("the socket was never subscribed to team {team_id}");
    }

    async fn send(&mut self, command: Value) {
        self.stream
            .send(Message::Text(command.to_string().into()))
            .await
            .expect("the command is sent");
    }

    /// The next JSON message, or `None` if none arrives within `wait`.
    async fn next_json(&mut self, wait: Duration) -> Option<Value> {
        loop {
            match tokio::time::timeout(wait, self.stream.next()).await {
                Ok(Some(Ok(Message::Text(text)))) => {
                    return Some(serde_json::from_str(&text).expect("messages are JSON"));
                }
                Ok(Some(Ok(Message::Ping(_) | Message::Pong(_)))) => {}
                _ => return None,
            }
        }
    }

    /// The block of the next message, which must be a block change in the test team.
    async fn next_block(&mut self) -> Value {
        let message = self
            .next_json(PATIENCE)
            .await
            .expect("a block change is announced");
        assert_eq!(message["action"], json!("UPDATE_BLOCK"), "{message}");
        assert_eq!(message["teamId"], json!(TEST_TEAM_ID), "{message}");
        message["block"].clone()
    }

    /// Checks that nothing more is announced once the server's background work is done.
    async fn expect_quiet(&mut self, th: &TestServer, why: &str) {
        assert!(
            th.app.finish_background(BACKGROUND).await,
            "background work finishes"
        );
        let message = self.next_json(QUIET).await;
        assert_eq!(message, None, "{why}");
    }
}

// -- server/integrationtests/blocks_test.go ---------------------------------------------

/// `TestGetBlocks`
#[tokio::test]
async fn get_blocks() {
    let th = Setup::one_user().start().await;
    let board = create_board_in_store(&th, BoardType::Open).await;

    let new_blocks = [
        new_block(&board.id, BlockType::Card, ""),
        new_block(&board.id, BlockType::Card, ""),
    ];
    let inserted = insert(&th.client, &board.id, &new_blocks).await;
    assert_eq!(inserted.len(), 2, "both blocks are answered");

    let blocks = blocks_of(&th.client, &board.id).await;
    assert_eq!(blocks.len(), 2, "both blocks are stored");
    let block_ids = ids(&blocks);
    assert!(block_ids.contains(&inserted[0].id.as_str()), "the first");
    assert!(block_ids.contains(&inserted[1].id.as_str()), "the second");

    th.teardown().await;
}

/// `TestPostBlock/"Create a single block"`
#[tokio::test]
async fn post_block_create_a_single_block() {
    let th = Setup::one_user().start().await;
    let board = create_board_in_store(&th, BoardType::Open).await;

    let block = new_block(&board.id, BlockType::Card, "New title");
    let inserted = insert(&th.client, &board.id, std::slice::from_ref(&block)).await;
    assert_eq!(inserted.len(), 1, "one block is answered");

    let blocks = blocks_of(&th.client, &board.id).await;
    assert_eq!(blocks.len(), 1, "one block is stored");
    assert_eq!(blocks[0].id, inserted[0].id, "the answered block");

    th.teardown().await;
}

/// `TestPostBlock/"Create a couple of blocks in the same call"`
#[tokio::test]
async fn post_block_create_a_couple_of_blocks_in_the_same_call() {
    let th = Setup::one_user().start().await;
    let board = create_board_in_store(&th, BoardType::Open).await;
    let first = insert_one(
        &th.client,
        &new_block(&board.id, BlockType::Card, "New title"),
    )
    .await;

    let new_blocks = [
        new_block(&board.id, BlockType::Card, ""),
        new_block(&board.id, BlockType::Card, ""),
    ];
    let inserted = insert(&th.client, &board.id, &new_blocks).await;
    assert_eq!(inserted.len(), 2, "both blocks are answered");
    assert_ne!(inserted[0].id, new_blocks[0].id, "a new identifier");
    assert_ne!(inserted[1].id, new_blocks[1].id, "a new identifier");

    let blocks = blocks_of(&th.client, &board.id).await;
    assert_eq!(blocks.len(), 3, "three blocks are stored");
    let block_ids = ids(&blocks);
    for expected in [&first.id, &inserted[0].id, &inserted[1].id] {
        assert!(
            block_ids.contains(&expected.as_str()),
            "{expected} is stored"
        );
    }

    th.teardown().await;
}

/// `TestPostBlock/"Update a block should not be possible through the insert endpoint"`
#[tokio::test]
async fn post_block_update_a_block_should_not_be_possible_through_the_insert_endpoint() {
    let th = Setup::one_user().start().await;
    let board = create_board_in_store(&th, BoardType::Open).await;
    let first = insert_one(
        &th.client,
        &new_block(&board.id, BlockType::Card, "New title"),
    )
    .await;

    let update = Block {
        id: first.id.clone(),
        update_at: 20,
        ..new_block(&board.id, BlockType::Card, "Updated title")
    };
    let inserted = insert(&th.client, &board.id, std::slice::from_ref(&update)).await;
    assert_eq!(inserted.len(), 1, "one block is answered");
    assert_ne!(inserted[0].id, first.id, "the identifier was replaced");

    let blocks = blocks_of(&th.client, &board.id).await;
    assert_eq!(blocks.len(), 2, "a second block was stored");
    assert_eq!(
        find(&blocks, &inserted[0].id).title,
        "Updated title",
        "the new block has the title"
    );
    assert_eq!(
        find(&blocks, &first.id).title,
        "New title",
        "the first block is untouched"
    );

    th.teardown().await;
}

/// The block the subtests of `TestPatchBlock` patch.
async fn patch_block_fixture(th: &TestServer) -> Block {
    let board = create_board_in_store(th, BoardType::Open).await;
    let block = Block {
        fields: fields(json!({"test": "test value", "test2": "test value 2"})),
        ..new_block(&board.id, BlockType::Card, "New title")
    };
    insert_one(&th.client, &block).await
}

/// `TestPatchBlock/"Patch a block basic field"`
#[tokio::test]
async fn patch_block_patch_a_block_basic_field() {
    let th = Setup::one_user().start().await;
    let block = patch_block_fixture(&th).await;

    patch(&th.client, &block, &title_patch("Updated title")).await;

    let blocks = blocks_of(&th.client, &block.board_id).await;
    assert_eq!(blocks.len(), 1, "still one block");
    assert_eq!(find(&blocks, &block.id).title, "Updated title", "the title");

    th.teardown().await;
}

fn custom_fields_patch() -> BlockPatch {
    BlockPatch {
        updated_fields: fields(json!({"test": "new test value", "test3": "new field"})),
        ..BlockPatch::default()
    }
}

/// `TestPatchBlock/"Patch a block custom fields"`
#[tokio::test]
async fn patch_block_patch_a_block_custom_fields() {
    let th = Setup::one_user().start().await;
    let block = patch_block_fixture(&th).await;
    patch(&th.client, &block, &title_patch("Updated title")).await;

    patch(&th.client, &block, &custom_fields_patch()).await;

    let blocks = blocks_of(&th.client, &block.board_id).await;
    assert_eq!(blocks.len(), 1, "still one block");
    let updated = find(&blocks, &block.id);
    assert_eq!(updated.fields["test"], json!("new test value"), "replaced");
    assert_eq!(updated.fields["test3"], json!("new field"), "added");
    assert_eq!(updated.title, "Updated title", "the title is untouched");

    th.teardown().await;
}

/// `TestPatchBlock/"Patch a block to remove custom fields"`
#[tokio::test]
async fn patch_block_patch_a_block_to_remove_custom_fields() {
    let th = Setup::one_user().start().await;
    let block = patch_block_fixture(&th).await;
    patch(&th.client, &block, &custom_fields_patch()).await;

    let remove = BlockPatch {
        deleted_fields: vec!["test".to_owned(), "test3".to_owned(), "test100".to_owned()],
        ..BlockPatch::default()
    };
    patch(&th.client, &block, &remove).await;

    let blocks = blocks_of(&th.client, &block.board_id).await;
    assert_eq!(blocks.len(), 1, "still one block");
    let updated = find(&blocks, &block.id);
    assert_eq!(updated.fields.get("test"), None, "removed");
    assert_eq!(updated.fields["test2"], json!("test value 2"), "kept");
    assert_eq!(updated.fields.get("test3"), None, "removed");

    th.teardown().await;
}

/// `TestDeleteBlock/"Create a block"`
#[tokio::test]
async fn delete_block_create_a_block() {
    let th = Setup::one_user().start().await;
    let board = create_board_in_store(&th, BoardType::Open).await;

    let block = new_block(&board.id, BlockType::Card, "New title");
    let inserted = insert(&th.client, &board.id, std::slice::from_ref(&block)).await;
    assert_eq!(inserted.len(), 1, "one block is answered");
    assert!(!inserted[0].id.is_empty(), "it has an identifier");
    assert_ne!(inserted[0].id, block.id, "a new one");

    let blocks = blocks_of(&th.client, &board.id).await;
    assert_eq!(
        ids(&blocks),
        [inserted[0].id.as_str()],
        "the block is stored"
    );

    th.teardown().await;
}

/// `TestDeleteBlock/"Delete a block"`
#[tokio::test]
async fn delete_block_delete_a_block() {
    let th = Setup::one_user().start().await;
    let board = create_board_in_store(&th, BoardType::Open).await;
    let block = insert_one(
        &th.client,
        &new_block(&board.id, BlockType::Card, "New title"),
    )
    .await;

    delete(&th.client, &block).await;

    let response = th.client.get(&blocks_path(&board.id)).await;
    response.check_ok();
    assert_eq!(
        response.text(),
        "[]",
        "no blocks are left, as an empty list"
    );

    th.teardown().await;
}

/// The block the subtests of `TestUndeleteBlock` delete and undelete: a view on an open
/// board of the first user. The original's tests use a legacy board block, a type that
/// no new block has any more.
async fn undelete_block_fixture(th: &TestServer) -> Block {
    let board = create_board_in_store(th, BoardType::Open).await;
    assert!(
        blocks_of(&th.client, &board.id).await.is_empty(),
        "the board starts empty"
    );
    insert_one(
        &th.client,
        &new_block(&board.id, BlockType::View, "New title"),
    )
    .await
}

/// `TestUndeleteBlock/"Create a block"`
#[tokio::test]
async fn undelete_block_create_a_block() {
    let th = Setup::native().start().await.init_basic().await;
    let board = create_board_in_store(&th, BoardType::Open).await;

    let block = new_block(&board.id, BlockType::View, "New title");
    let inserted = insert(&th.client, &board.id, std::slice::from_ref(&block)).await;
    assert_eq!(inserted.len(), 1, "one block is answered");
    assert!(!inserted[0].id.is_empty(), "it has an identifier");
    assert_ne!(inserted[0].id, block.id, "a new one");

    let blocks = blocks_of(&th.client, &board.id).await;
    assert_eq!(
        ids(&blocks),
        [inserted[0].id.as_str()],
        "the block is stored"
    );

    th.teardown().await;
}

/// `TestUndeleteBlock/"Delete a block"`
#[tokio::test]
async fn undelete_block_delete_a_block() {
    let th = Setup::native().start().await.init_basic().await;
    let block = undelete_block_fixture(&th).await;

    delete(&th.client, &block).await;

    let blocks = blocks_of(&th.client, &block.board_id).await;
    assert!(blocks.is_empty(), "the block is gone: {:?}", ids(&blocks));

    th.teardown().await;
}

/// `TestUndeleteBlock/"Undelete a block"`
#[tokio::test]
async fn undelete_block_undelete_a_block() {
    let th = Setup::native().start().await.init_basic().await;
    let block = undelete_block_fixture(&th).await;
    delete(&th.client, &block).await;

    let undeleted: Block = th
        .client
        .post_empty(&undelete_path(&block.board_id, &block.id))
        .await
        .ok_json();
    assert_eq!(undeleted.id, block.id, "the answer is the restored block");
    assert_eq!(undeleted.title, "New title", "with its content");
    assert_eq!(undeleted.delete_at, 0, "and not deleted");

    let blocks = blocks_of(&th.client, &block.board_id).await;
    assert_eq!(ids(&blocks), [block.id.as_str()], "the block is back");

    th.teardown().await;
}

/// `TestUndeleteBlock/"Try to undelete a block without permissions"`
#[tokio::test]
async fn undelete_block_try_to_undelete_a_block_without_permissions() {
    let th = Setup::native().start().await.init_basic().await;
    let block = undelete_block_fixture(&th).await;
    let path = undelete_path(&block.board_id, &block.id);
    delete(&th.client, &block).await;
    th.client.post_empty(&path).await.check_ok();
    delete(&th.client, &block).await;

    // (The original's message speaks of members.)
    let response = th.client2.post_empty(&path).await;
    response.check_error(StatusCode::FORBIDDEN, NO_CHANGES);

    let blocks = blocks_of(&th.client, &block.board_id).await;
    assert!(blocks.is_empty(), "the block stays deleted");

    th.teardown().await;
}

// -- server/app/blocks_test.go, through the real store ------------------------------------

/// `TestInsertBlock/"success scenario"` (app)
#[tokio::test]
async fn app_insert_block_success_scenario() {
    let th = Setup::one_user().start().await;
    let board = create_board_in_store(&th, BoardType::Open).await;
    let mut block = new_block(&board.id, BlockType::Card, "A card");

    th.app
        .insert_block(&mut block, &th.stranger().await, ChangeNotification::Send)
        .await
        .expect("the block is inserted");

    let stored = th.app.block(&block.id).await.expect("the block is stored");
    assert_eq!(stored.title, "A card", "as given");
    assert_eq!(
        stored.created_by,
        th.stranger().await,
        "created by the modifier"
    );
    assert_eq!(
        stored.modified_by,
        th.stranger().await,
        "and modified by them"
    );

    th.teardown().await;
}

/// `TestInsertBlock/"error scenario"` (app). The original injects a store failure
/// through a mock; the failures the real store has are a block it refuses and a board
/// that does not exist.
#[tokio::test]
async fn app_insert_block_error_scenario() {
    let th = Setup::one_user().start().await;
    let board = create_board_in_store(&th, BoardType::Open).await;

    let mut too_long = new_block(
        &board.id,
        BlockType::Card,
        &"a".repeat(BLOCK_TITLE_MAX_CHARS + 1),
    );
    let error = th
        .app
        .insert_block(
            &mut too_long,
            &th.stranger().await,
            ChangeNotification::Send,
        )
        .await
        .expect_err("the title is too long");
    assert_eq!(error.kind(), ErrorKind::BadRequest, "{error:?}");
    assert!(
        th.app.block(&too_long.id).await.is_err(),
        "nothing was stored"
    );

    let mut homeless = new_block("no-such-board", BlockType::Card, "");
    let error = th
        .app
        .insert_block(
            &mut homeless,
            &th.stranger().await,
            ChangeNotification::Send,
        )
        .await
        .expect_err("the board does not exist");
    assert!(error.is_not_found(), "{error:?}");

    th.teardown().await;
}

/// `TestPatchBlocks/"patchBlocks success scenario"` (app)
#[tokio::test]
async fn app_patch_blocks_success_scenario() {
    let th = Setup::one_user().start().await;
    let board = create_board_in_store(&th, BoardType::Open).await;
    let block = insert_one(&th.client, &new_block(&board.id, BlockType::Card, "old")).await;

    th.app
        .patch_blocks(
            TEST_TEAM_ID,
            &batch(&[(&block, "new title")]),
            &th.stranger().await,
            ChangeNotification::Send,
        )
        .await
        .expect("the batch is applied");

    let stored = th.app.block(&block.id).await.expect("the block exists");
    assert_eq!(stored.title, "new title", "patched");
    assert_eq!(stored.modified_by, th.stranger().await, "by the modifier");

    th.teardown().await;
}

/// `TestPatchBlocks/"patchBlocks error scenario"` (app). The original has the mocked
/// lookup of the blocks fail; with the real store it fails for an unknown identifier,
/// and then nothing is patched.
#[tokio::test]
async fn app_patch_blocks_error_scenario() {
    let th = Setup::one_user().start().await;
    let board = create_board_in_store(&th, BoardType::Open).await;
    let block = insert_one(&th.client, &new_block(&board.id, BlockType::Card, "old")).await;
    let missing = new_block(&board.id, BlockType::Card, "never stored");

    let error = th
        .app
        .patch_blocks(
            TEST_TEAM_ID,
            &batch(&[(&block, "new title"), (&missing, "new title")]),
            &th.stranger().await,
            ChangeNotification::Send,
        )
        .await
        .expect_err("one block does not exist");
    assert!(error.is_not_found(), "{error:?}");

    let stored = th.app.block(&block.id).await.expect("the block exists");
    assert_eq!(stored.title, "old", "nothing was patched");

    th.teardown().await;
}

/// `TestDeleteBlock/"success scenario"` (app)
#[tokio::test]
async fn app_delete_block_success_scenario() {
    let th = Setup::one_user().start().await;
    let board = create_board_in_store(&th, BoardType::Open).await;
    let block = insert_one(&th.client, &new_block(&board.id, BlockType::Card, "")).await;

    th.app
        .delete_block(&block.id, &th.stranger().await, ChangeNotification::Send)
        .await
        .expect("the block is deleted");

    let error = th
        .app
        .block(&block.id)
        .await
        .expect_err("the block is gone");
    assert!(error.is_not_found(), "{error:?}");
    let deleted = th
        .app
        .deleted_block(&block.id)
        .await
        .expect("the trash is read")
        .expect("the block is in the trash");
    assert_ne!(deleted.block.delete_at, 0, "the trash records the deletion");
    assert_eq!(deleted.deleted_by, th.stranger().await, "and who deleted");

    th.teardown().await;
}

/// `TestDeleteBlock/"error scenario"` (app). The original injects a store failure
/// through a mock; the failure the real store has is a block that does not exist.
#[tokio::test]
async fn app_delete_block_error_scenario() {
    let th = Setup::one_user().start().await;

    let error = th
        .app
        .delete_block(
            "no-such-block",
            &th.stranger().await,
            ChangeNotification::Send,
        )
        .await
        .expect_err("there is no such block");
    assert!(error.is_not_found(), "{error:?}");
    assert_eq!(error.to_string(), "not found", "no identifier in it");

    th.teardown().await;
}

/// `TestUndeleteBlock/"success scenario"` (app)
#[tokio::test]
async fn app_undelete_block_success_scenario() {
    let th = Setup::one_user().start().await;
    let board = create_board_in_store(&th, BoardType::Open).await;
    let block = insert_one(&th.client, &new_block(&board.id, BlockType::Card, "back")).await;
    th.app
        .delete_block(&block.id, &th.stranger().await, ChangeNotification::Send)
        .await
        .expect("the block is deleted");

    let undeleted = th
        .app
        .undelete_block(&block.id, &th.stranger().await)
        .await
        .expect("the block is undeleted");
    assert_eq!(undeleted.id, block.id, "the restored block is returned");
    assert_eq!(undeleted.title, "back", "with its content");

    let stored = th.app.block(&block.id).await.expect("the block is stored");
    assert_eq!(stored.delete_at, 0, "and not deleted");

    th.teardown().await;
}

/// What is not in the trash is not there to restore: a block that never existed, by a
/// well-formed identifier or by text that is none, and a block that exists.
#[tokio::test]
async fn app_undelete_block_that_is_not_in_the_trash() {
    let th = Setup::one_user().start().await;
    let board = create_board_in_store(&th, BoardType::Open).await;
    let block = insert_one(&th.client, &new_block(&board.id, BlockType::Card, "here")).await;

    for block_id in [UNKNOWN_ID, "no-such-block", block.id.as_str()] {
        let error = th
            .app
            .undelete_block(block_id, &th.stranger().await)
            .await
            .expect_err("nothing to restore");
        assert!(error.is_not_found(), "{block_id}: {error:?}");
    }
    let stored = th.app.block(&block.id).await.expect("the block is stored");
    assert_eq!(stored.modified_by, block.modified_by, "and untouched");

    th.teardown().await;
}

/// `TestInsertBlocks/"success scenario"` (app)
#[tokio::test]
async fn app_insert_blocks_success_scenario() {
    let th = Setup::one_user().start().await;
    let board = create_board_in_store(&th, BoardType::Open).await;
    let mut blocks = vec![new_block(&board.id, BlockType::Card, "A card")];

    th.app
        .insert_blocks(&mut blocks, &th.stranger().await, ChangeNotification::Send)
        .await
        .expect("the blocks are inserted");

    let stored = th.app.blocks_for_board(&board.id).await.expect("readable");
    assert_eq!(ids(&stored), [blocks[0].id.as_str()], "the block is stored");
    assert_eq!(
        blocks[0].modified_by,
        th.stranger().await,
        "the caller's copy says who stored it"
    );

    th.app
        .insert_blocks(&mut [], &th.stranger().await, ChangeNotification::Send)
        .await
        .expect("no blocks are no error");

    th.teardown().await;
}

/// `TestInsertBlocks/"error scenario"` (app). The original injects a store failure
/// through a mock; the real store refuses a block that is too large. Blocks of several
/// boards are refused before anything is stored.
#[tokio::test]
async fn app_insert_blocks_error_scenario() {
    let th = Setup::one_user().start().await;
    let board = create_board_in_store(&th, BoardType::Open).await;
    let other = create_board_in_store(&th, BoardType::Open).await;

    let mut too_long = vec![new_block(
        &board.id,
        BlockType::Card,
        &"a".repeat(BLOCK_TITLE_MAX_CHARS + 1),
    )];
    let error = th
        .app
        .insert_blocks(
            &mut too_long,
            &th.stranger().await,
            ChangeNotification::Send,
        )
        .await
        .expect_err("the title is too long");
    assert_eq!(error.kind(), ErrorKind::BadRequest, "{error:?}");

    let mut mixed = vec![
        new_block(&board.id, BlockType::Card, ""),
        new_block(&other.id, BlockType::Card, ""),
    ];
    let error = th
        .app
        .insert_blocks(&mut mixed, &th.stranger().await, ChangeNotification::Send)
        .await
        .expect_err("two boards in one call");
    assert_eq!(error.kind(), ErrorKind::BadRequest, "{error:?}");
    assert_eq!(
        error.to_string(),
        "the blocks belong to more than one board",
        "what is wrong with the request"
    );

    let stored = th.app.blocks_for_board(&board.id).await.expect("readable");
    assert!(stored.is_empty(), "nothing was stored: {:?}", ids(&stored));

    th.teardown().await;
}

// -- GET /boards/{board_id}/blocks ------------------------------------------------------

/// Who may read the blocks of a board, and what the others are told.
#[tokio::test]
async fn get_blocks_permissions() {
    let th = Setup::native().start().await.init_basic().await;
    let boards = boards_with_roles(&th).await;
    let open_board = create_board_in_store(&th, BoardType::Open).await;
    card_with_content(&th, &boards).await;
    let path = blocks_path(&boards.board.id);

    let response = th.anonymous().get(&path).await;
    response.check_unauthorized();
    assert_eq!(
        response.error_message(),
        "access denied to board",
        "no session"
    );

    // Without a session the board is not even looked up.
    let response = th.anonymous().get(&blocks_path("no-such-board")).await;
    response.check_unauthorized();
    assert_eq!(
        response.error_message(),
        "access denied to board",
        "no session, no board"
    );

    let response = th.client.get(&blocks_path("no-such-board")).await;
    response.check_error(StatusCode::NOT_FOUND, NO_BOARD);

    // No member: the open board can be seen but not read; the private one does not
    // exist.
    let response = th.client2.get(&blocks_path(&open_board.id)).await;
    response.check_error(StatusCode::FORBIDDEN, "access denied to board");
    let response = th.client2.get(&blocks_path(&boards.board.id)).await;
    response.check_error(StatusCode::NOT_FOUND, NO_BOARD);

    for (role, client) in [
        ("viewer", &boards.viewer),
        ("commenter", &boards.commenter),
        ("editor", &boards.editor),
        ("admin", &th.client),
    ] {
        let blocks: Vec<Block> = client.get(&path).await.ok_json();
        assert_eq!(titles(&blocks), ["card", "comment", "text"], "{role}");
    }

    th.teardown().await;
}

/// The blocks of an open template are for everyone in the team; a private template is
/// for its members.
#[tokio::test]
async fn get_blocks_of_templates() {
    let th = Setup::native().start().await.init_basic().await;
    let template = |board_type| Board {
        team_id: TEST_TEAM_ID.to_owned(),
        board_type,
        is_template: true,
        ..Board::default()
    };
    let open_template = store_board(&th, template(BoardType::Open)).await;
    let private_template = store_board(&th, template(BoardType::Private)).await;
    for board in [&open_template, &private_template] {
        let block = new_block(&board.id, BlockType::Card, "in a template");
        insert_one(&th.client, &block).await;
    }

    let blocks: Vec<Block> = th
        .client2
        .get(&blocks_path(&open_template.id))
        .await
        .ok_json();
    assert_eq!(titles(&blocks), ["in a template"], "no membership needed");

    // A private template is for its members; for everyone else it does not exist.
    let response = th.client2.get(&blocks_path(&private_template.id)).await;
    response.check_error(StatusCode::NOT_FOUND, NO_BOARD);

    let blocks: Vec<Block> = th
        .client
        .get(&blocks_path(&private_template.id))
        .await
        .ok_json();
    assert_eq!(blocks.len(), 1, "a member reads the private template");

    th.teardown().await;
}

/// Fills the board with user identifiers: person properties, a card whose body mentions
/// `me` and whose values name `me` and `editor`, and a view that filters and groups by
/// them.
async fn board_with_people(th: &TestServer, board_id: &str, me: &str, editor: &str) {
    let people = json!([
        {"id": "owner", "name": "Owner", "type": "person"},
        {"id": "team", "name": "Team", "type": "multiPerson"},
        {"id": "by", "name": "By", "type": "createdBy"},
        {"id": "note", "name": "Note", "type": "text"},
    ]);
    th.client
        .patch(
            &format!("/boards/{board_id}"),
            &json!({"updatedCardProperties": people}),
        )
        .await
        .check_ok();
    let mention = json!({"type": "mention", "attrs": {"userId": me}});
    let body = json!({"type": "doc", "content": [{"type": "paragraph", "content": [mention]}]});
    th.client
        .post(
            &format!("/boards/{board_id}/cards"),
            &json!({
                "title": "mentions",
                "body": body,
                "properties": {"owner": me, "team": [me, editor], "note": me, "gone": me},
            }),
        )
        .await
        .check_created();
    let mut view = new_block(board_id, BlockType::View, "view");
    view.fields = fields(json!({
        "filter": {"operation": "and", "filters": [
            {"propertyId": "owner", "condition": "includes", "values": [me]},
            {"operation": "or", "filters": [
                {"propertyId": "by", "condition": "includes", "values": [me]},
                {"propertyId": "note", "condition": "includes", "values": [me]},
            ]},
        ]},
        "groupById": "owner",
        "hiddenOptionIds": [me, ""],
        "collapsedOptionIds": [editor],
    }));
    insert_one(&th.client, &view).await;
}

/// What the blocks of [`board_with_people`] say about people.
fn people_in(blocks: &Value) -> Value {
    let of_type = |block_type: &str| {
        blocks
            .as_array()
            .into_iter()
            .flatten()
            .find(|block| block["type"] == block_type)
            .cloned()
            .unwrap_or_else(|| panic!("a {block_type} is read: {blocks}"))
    };
    let card = of_type("card");
    let view = of_type("view");
    json!({
        "createdBy": card["createdBy"],
        "mention": card["body"]["content"][0]["content"][0]["attrs"]["userId"],
        "properties": card["fields"]["properties"],
        "filter": view["fields"]["filter"],
        "hidden": view["fields"]["hiddenOptionIds"],
        "collapsed": view["fields"]["collapsedOptionIds"],
        "viewBy": view["modifiedBy"],
    })
}

/// Whoever reads a shared board by its read token without being a member, without a
/// session or signed in, is told no user's identifier: not who wrote a block, nor who
/// its body mentions, nor whom person properties name in values, filters and the groups
/// of views. A member reading with the token is.
#[tokio::test]
async fn a_reader_by_read_token_is_not_told_who_is_mentioned() {
    let th = Setup::local_mode().start().await.init_basic().await;
    let boards = boards_with_roles(&th).await;
    let me = th.client.me().await;
    let editor = boards.editor.me().await;
    board_with_people(&th, &boards.board.id, &me.id, &editor.id).await;
    let read_token = th.share(&boards.board.id).await;
    let path = format!("{}?readToken={read_token}", blocks_path(&boards.board.id));
    let hidden = json!({
        "createdBy": "",
        "mention": "",
        "properties": {"owner": "", "team": ["", ""], "note": me.id},
        "filter": {"operation": "and", "filters": [
            {"propertyId": "owner", "condition": "includes", "values": [""]},
            {"operation": "or", "filters": [
                {"propertyId": "by", "condition": "includes", "values": [""]},
                {"propertyId": "note", "condition": "includes", "values": [me.id]},
            ]},
        ]},
        "hidden": [""],
        "collapsed": [],
        "viewBy": "",
    });

    // As JSON: the answer is a shape that is only sent, which the allowlist of a write
    // refuses.
    let blocks = th.anonymous().get(&path).await.value();
    assert_eq!(people_in(&blocks), hidden, "nobody, to the token alone");
    let blocks = th.client2.get(&path).await.value();
    assert_eq!(
        people_in(&blocks),
        hidden,
        "nobody, to a user who is no member"
    );
    let shown = people_in(&th.client.get(&path).await.value());
    assert_eq!(shown["mention"], json!(me.id), "the user, to a member");
    assert_eq!(
        shown["properties"]["team"],
        json!([me.id, editor.id]),
        "{shown}"
    );
    assert_eq!(shown["properties"]["gone"], json!(me.id), "{shown}");
    assert_eq!(
        shown["filter"]["filters"][0]["values"],
        json!([me.id]),
        "{shown}"
    );
    assert_eq!(shown["hidden"], json!([me.id, ""]), "{shown}");
    let blocks = th.client.get(&blocks_path(&boards.board.id)).await.value();
    assert_eq!(
        people_in(&blocks),
        shown,
        "the token changes nothing for a member"
    );
    th.teardown().await;
}

/// A shared board's read token lets anyone read its blocks, and nothing else.
#[tokio::test]
async fn get_blocks_with_a_read_token() {
    let th = Setup::local_mode().start().await.init_basic().await;
    let boards = boards_with_roles(&th).await;
    let content = card_with_content(&th, &boards).await;
    let read_token = th.share(&boards.board.id).await;
    let anonymous = th.anonymous();
    let with_token = |board: &Board| format!("{}?readToken={read_token}", blocks_path(&board.id));

    let blocks: Vec<Block> = anonymous.get(&with_token(&boards.board)).await.ok_json();
    assert_eq!(
        titles(&blocks),
        ["card", "comment", "text"],
        "the token reads the board"
    );

    // A signed-in user who is no member reads with the token too.
    let blocks: Vec<Block> = th.client2.get(&with_token(&boards.board)).await.ok_json();
    assert_eq!(blocks.len(), 3, "the token counts for a non-member");

    let wrong = format!("{}?readToken=wrong", blocks_path(&boards.board.id));
    let response = anonymous.get(&wrong).await;
    response.check_unauthorized();
    assert_eq!(
        response.error_message(),
        "access denied to board",
        "a wrong token is no token"
    );
    th.client2
        .get(&wrong)
        .await
        .check_error(StatusCode::NOT_FOUND, NO_BOARD);

    // The token of one board opens no other board, by path or by block identifier.
    anonymous
        .get(&with_token(&boards.other_board))
        .await
        .check_unauthorized();
    let response = anonymous
        .get(&format!(
            "{}&blockId={}",
            with_token(&boards.board),
            content.other_card.id
        ))
        .await;
    response.check_error(StatusCode::NOT_FOUND, NO_BLOCK);

    // It does not let anyone write.
    let block = new_block(&boards.board.id, BlockType::Card, "");
    anonymous
        .post(&with_token(&boards.board), &[block])
        .await
        .check_unauthorized();

    th.teardown().await;
}

/// The list parameters of `GET /boards/{board_id}/blocks`: `all`, `parentId` and
/// `type`.
#[tokio::test]
async fn get_blocks_filters() {
    let th = Setup::one_user().start().await;
    let board = create_board_in_store(&th, BoardType::Open).await;
    let path = blocks_path(&board.id);

    let empty = th.client.get(&path).await;
    empty.check_ok();
    assert_eq!(empty.text(), "[]", "an empty board is an empty list");

    let card = insert_one(&th.client, &new_block(&board.id, BlockType::Card, "card")).await;
    let children = [
        child_of(&card, BlockType::Attachment, "text"),
        child_of(&card, BlockType::Comment, "comment"),
    ];
    insert(&th.client, &board.id, &children).await;
    insert_one(&th.client, &new_block(&board.id, BlockType::Card, "card 2")).await;
    let children_of_card = format!("parentId={}", card.id);

    let everything = vec!["card", "card 2", "comment", "text"];
    let none: Vec<&str> = Vec::new();
    let cases = [
        ("no parameters", String::new(), everything.clone()),
        ("all", "?all=true".to_owned(), everything.clone()),
        (
            "all wins over the filters",
            "?all=true&type=card&parentId=x".to_owned(),
            everything.clone(),
        ),
        (
            "an empty all is no all",
            "?all=".to_owned(),
            everything.clone(),
        ),
        (
            "the children of the card",
            format!("?{children_of_card}"),
            vec!["comment", "text"],
        ),
        ("by type", "?type=card".to_owned(), vec!["card", "card 2"]),
        (
            "by type and parent",
            format!("?type=attachment&{children_of_card}"),
            vec!["text"],
        ),
        (
            "no card has the card as parent",
            format!("?type=card&{children_of_card}"),
            none.clone(),
        ),
        (
            "a type the server does not know is matched as written: no block has it",
            "?type=h1".to_owned(),
            none.clone(),
        ),
        ("the type is case-sensitive", "?type=Card".to_owned(), none),
        (
            "preserved quirk: the type unknown does not filter",
            "?type=unknown".to_owned(),
            everything.clone(),
        ),
        (
            "there is no paging on this route",
            "?page=1&perPage=1".to_owned(),
            everything,
        ),
    ];
    for (case, query, expected) in cases {
        let blocks: Vec<Block> = th.client.get(&format!("{path}{query}")).await.ok_json();
        assert_eq!(titles(&blocks), expected, "{case}");
    }

    let no_children = th.client.get(&format!("{path}?parentId=nobody")).await;
    no_children.check_ok();
    assert_eq!(no_children.text(), "[]", "no match is an empty list");

    th.teardown().await;
}

/// `GET /boards/{board_id}/blocks?blockId=`: one block, if it is on the board.
#[tokio::test]
async fn get_blocks_by_block_id() {
    let th = Setup::one_user().start().await;
    let board = create_board_in_store(&th, BoardType::Open).await;
    let other_board = create_board_in_store(&th, BoardType::Open).await;
    let path = blocks_path(&board.id);
    let card = insert_one(&th.client, &new_block(&board.id, BlockType::Card, "card")).await;
    insert_one(&th.client, &child_of(&card, BlockType::Attachment, "text")).await;
    let other_block = insert_one(
        &th.client,
        &new_block(&other_board.id, BlockType::Card, "other"),
    )
    .await;

    let blocks: Vec<Block> = th
        .client
        .get(&format!("{path}?blockId={}&type=text", card.id))
        .await
        .ok_json();
    assert_eq!(
        ids(&blocks),
        [card.id.as_str()],
        "the one block, whatever the filters say"
    );

    let response = th
        .client
        .get(&format!("{path}?blockId=no-such-block"))
        .await;
    response.check_error(StatusCode::NOT_FOUND, NO_BLOCK);

    let response = th
        .client
        .get(&format!("{path}?blockId={}", other_block.id))
        .await;
    // A block of another board answers as one that does not exist.
    response.check_error(StatusCode::NOT_FOUND, NO_BLOCK);

    th.teardown().await;
}

/// A query parameter has one spelling. Any other spelling of it is a parameter the server
/// does not know, and those are ignored: it filters nothing, opens nothing and names no
/// source board.
#[tokio::test]
async fn a_query_parameter_spelled_another_way_is_no_parameter() {
    let th = Setup::local_mode().start().await.init_basic().await;
    let boards = boards_with_roles(&th).await;
    let content = card_with_content(&th, &boards).await;
    let path = blocks_path(&boards.board.id);

    // No filter: the answer is the one to no parameters at all.
    for query in [
        format!("parent_id={}", content.card.id),
        "parent_id=nobody".to_owned(),
        format!("parentID={}", content.card.id),
        format!("block_id={}", content.text.id),
        "block_id=no-such-block".to_owned(),
        format!("blockID={}", content.text.id),
    ] {
        let blocks: Vec<Block> = th.client.get(&format!("{path}?{query}")).await.ok_json();
        assert_eq!(titles(&blocks), ["card", "comment", "text"], "{query}");
    }
    let children: Vec<Block> = th
        .client
        .get(&format!("{path}?parentId={}", content.card.id))
        .await
        .ok_json();
    assert_eq!(
        titles(&children),
        ["comment", "text"],
        "the parameter itself filters"
    );

    // No read token: without a session the request is one without a session, and with
    // the session of somebody who is no member the board does not exist.
    let read_token = th.share(&boards.board.id).await;
    let anonymous = th.anonymous();
    for route in [path.clone(), format!("/boards/{}", boards.board.id)] {
        anonymous
            .get(&format!("{route}?readToken={read_token}"))
            .await
            .check_ok();
        for name in ["read_token", "readtoken", "ReadToken"] {
            let with_token = format!("{route}?{name}={read_token}");
            let response = anonymous.get(&with_token).await;
            response.check_unauthorized();
            assert_eq!(
                response.problem_type(),
                "/problems/unauthorized",
                "{with_token}"
            );
            th.client2
                .get(&with_token)
                .await
                .check_error(StatusCode::NOT_FOUND, NO_BOARD);
        }
    }

    // No source board: a source that is not there is refused, and its other spellings
    // are not looked at.
    let card = new_block(&boards.board.id, BlockType::Card, "posted");
    let response = th
        .client
        .post(&format!("{path}?sourceBoardId=no-such-board"), &[&card])
        .await;
    response.check_error(StatusCode::NOT_FOUND, "source board not found");
    for name in ["sourceBoardID", "source_board_id"] {
        let response = th
            .client
            .post(&format!("{path}?{name}=no-such-board"), &[&card])
            .await;
        assert_eq!(
            response.status,
            StatusCode::CREATED,
            "{name}: {}",
            response.text()
        );
    }

    th.teardown().await;
}

// -- POST /boards/{board_id}/blocks -----------------------------------------------------

/// What an insert answers and what it stores: new identifiers, the user as creator, and
/// the request's `createAt` in the answer while the stored block has the current time.
#[tokio::test]
async fn post_blocks_answer_and_stored_block() {
    let th = Setup::native().start().await.init_basic().await;
    let board = create_board_in_store(&th, BoardType::Private).await;
    let user = th.client.me().await;
    let before = fvoci_server::auth::now_millis();

    let card = new_block(&board.id, BlockType::Card, "card");
    let text = child_of(&card, BlockType::Attachment, "text");
    let response = th
        .client
        .insert_blocks(&board.id, &[card.clone(), text])
        .await;
    let answered: Vec<Block> = response.created_json();

    assert_eq!(answered.len(), 2, "both blocks are answered, in order");
    assert_ne!(answered[0].id, card.id, "the card has a new identifier");
    assert!(
        fvoci_model::is_id(&answered[0].id),
        "a generated one: {}",
        answered[0].id
    );
    assert_eq!(
        answered[1].parent_id, answered[0].id,
        "the link between the blocks follows the new identifier"
    );
    for block in &answered {
        assert_eq!(block.created_by, user.id, "created by the user");
        assert_eq!(block.modified_by, user.id, "modified by the user");
        assert!(
            block.create_at >= before,
            "created now, whatever the request says"
        );
        assert!(block.update_at >= before, "the modification time is now");
    }
    // A block sent without fields has an empty object, where the original says null.
    assert_eq!(response.value()[0]["fields"], json!({}), "no null fields");

    let stored = blocks_of(&th.client, &board.id).await;
    assert_eq!(stored, answered, "both are stored as answered");

    th.teardown().await;
}

/// Blocks may come without an identifier, as the contract says, and the blocks of one
/// request are listed in the order they were sent in, whichever of them the others
/// refer to: they are stored within a millisecond of each other, and what orders them
/// then is the identifier the server gave each.
#[tokio::test]
async fn post_blocks_without_identifiers_and_in_the_order_of_the_request() {
    let th = Setup::native().start().await.init_basic().await;
    let board = create_board_in_store(&th, BoardType::Private).await;

    let nameless: Vec<Value> = (0..3)
        .map(|index| {
            json!({
                "boardId": board.id, "type": "card", "title": format!("nameless {index}"),
                "createAt": 1, "updateAt": 1,
            })
        })
        .collect();
    let answered: Vec<Block> = th
        .client
        .post(&blocks_path(&board.id), &nameless)
        .await
        .created_json();
    assert_eq!(answered.len(), 3);
    for block in &answered {
        assert!(fvoci_model::is_id(&block.id), "{}", block.id);
        assert_eq!(block.parent_id, "", "a block on the board has no parent");
    }
    assert_ne!(
        answered[0].id, answered[1].id,
        "each has its own identifier"
    );

    // Cards and their texts, the texts first: the cards are what is referred to.
    let mut request: Vec<Block> = Vec::new();
    for index in 0..20 {
        let card = new_block(&board.id, BlockType::Card, &format!("card {index}"));
        request.push(child_of(
            &card,
            BlockType::Attachment,
            &format!("text {index}"),
        ));
        request.push(card);
    }
    let made = insert(&th.client, &board.id, &request).await;
    let listed = blocks_of(&th.client, &board.id).await;
    // In the order they come in, not sorted.
    let in_order = |blocks: &[Block]| -> Vec<String> {
        blocks.iter().map(|block| block.title.clone()).collect()
    };
    let sent = in_order(&request);
    assert_eq!(in_order(&made), sent, "answered as sent");
    let expected: Vec<String> = in_order(&answered).into_iter().chain(sent).collect();
    assert_eq!(in_order(&listed), expected, "listed as sent");

    th.teardown().await;
}

/// The refusals of an insert that come before any permission is looked at.
#[tokio::test]
async fn post_blocks_validation() {
    let th = Setup::native().start().await.init_basic().await;
    let board = create_board_in_store(&th, BoardType::Private).await;
    let other_board = create_board_in_store(&th, BoardType::Private).await;
    let path = blocks_path(&board.id);
    let valid =
        json!({"id": "x1", "boardId": board.id, "type": "card", "createAt": 1, "updateAt": 1});
    let without = |member: &str| {
        let mut block = valid.clone();
        block
            .as_object_mut()
            .expect("the block is an object")
            .remove(member);
        json!([block])
    };
    let mut elsewhere = valid.clone();
    elsewhere["boardId"] = json!(other_board.id);

    let cases = [
        (without("type"), "missing type for block id x1"),
        (without("createAt"), "invalid createAt for block id x1"),
        (without("updateAt"), "invalid updateAt for block id x1"),
        (without("boardId"), "invalid boardId for block id x1"),
        (json!([elsewhere]), "invalid boardId for block id x1"),
    ];
    for (body, message) in &cases {
        let response = th.client.post(&path, body).await;
        response.check_error(StatusCode::BAD_REQUEST, message);
        // A user who is no member of the private board learns nothing from the body.
        let response = th.client2.post(&path, body).await;
        response.check_error(StatusCode::NOT_FOUND, NO_BOARD);
    }

    // A body that is no list of blocks is a bad request. (The original answers 500, and
    // reads `null` as an empty list.)
    for body in [
        "not json",
        "null",
        "{\"a\":1}",
        "[null]",
        "[{\"createAt\":\"x\"}]",
    ] {
        let response = th.client.send_raw_body(Method::POST, &path, body).await;
        response.check_bad_request();
        assert_eq!(response.problem_type(), "/problems/bad-request", "{body}");
    }

    assert!(
        blocks_of(&th.client, &board.id).await.is_empty(),
        "nothing was stored"
    );
    th.teardown().await;
}

/// A request without blocks creates nothing and needs no more than to see the board.
/// (The original checks such a request for nothing: anyone signed in gets an empty list,
/// for any board, existing or not.)
#[tokio::test]
async fn post_blocks_without_blocks() {
    let th = Setup::native().start().await.init_basic().await;
    let board = create_board_in_store(&th, BoardType::Private).await;
    let path = blocks_path(&board.id);

    let response = th.client.send_raw_body(Method::POST, &path, "[]").await;
    response.check_created();
    assert_eq!(response.text(), "[]", "an empty list for a member");
    for (client, path) in [
        (&th.client2, path.as_str()),
        (&th.client2, "/boards/no-such-board/blocks"),
        (&th.client, "/boards/no-such-board/blocks"),
    ] {
        let response = client.send_raw_body(Method::POST, path, "[]").await;
        response.check_error(StatusCode::NOT_FOUND, NO_BOARD);
    }

    let response = th
        .anonymous()
        .send_raw_body(Method::POST, &path, "[]")
        .await;
    response.check_unauthorized();
    assert_eq!(
        response.error_message(),
        "no session token",
        "a session is needed all the same"
    );

    // With a block, the missing board shows as a missing permission.
    let block = new_block("no-such-board", BlockType::Card, "");
    let response = th
        .client
        .post("/boards/no-such-board/blocks", &[block])
        .await;
    response.check_error(StatusCode::NOT_FOUND, NO_BOARD);

    th.teardown().await;
}

/// Comments need the permission to comment, everything else the permission to manage
/// cards.
#[tokio::test]
async fn post_blocks_permissions_by_block_type() {
    let th = Setup::native().start().await.init_basic().await;
    let boards = boards_with_roles(&th).await;
    let board_id = &boards.board.id;
    let outsider = &th.client2;
    let card = || new_block(board_id, BlockType::Card, "card");
    let comment = || new_block(board_id, BlockType::Comment, "comment");

    let cases: [(&str, &Client, Vec<Block>, Option<&str>); 8] = [
        ("editor, card", &boards.editor, vec![card()], None),
        ("editor, comment", &boards.editor, vec![comment()], None),
        (
            "commenter, card",
            &boards.commenter,
            vec![card()],
            Some(NO_CHANGES),
        ),
        (
            "commenter, comment",
            &boards.commenter,
            vec![comment()],
            None,
        ),
        (
            "commenter, comment and card",
            &boards.commenter,
            vec![comment(), card()],
            Some(NO_CHANGES),
        ),
        (
            "viewer, card",
            &boards.viewer,
            vec![card()],
            Some(NO_CHANGES),
        ),
        (
            "viewer, comment",
            &boards.viewer,
            vec![comment()],
            Some(NO_COMMENTS),
        ),
        (
            "admin, card and comment",
            &th.client,
            vec![card(), comment()],
            None,
        ),
    ];
    let mut expected_count = 0;
    for (case, client, blocks, refusal) in &cases {
        let response = client.insert_blocks(board_id, blocks).await;
        if let Some(message) = refusal {
            response.check_forbidden();
            assert_eq!(response.error_message(), *message, "{case}");
        } else {
            response.check_created();
            expected_count += blocks.len();
        }
    }
    // For who is no member of the private board there is no board to add to.
    for blocks in [vec![card()], vec![comment()]] {
        let response = outsider.insert_blocks(board_id, &blocks).await;
        response.check_error(StatusCode::NOT_FOUND, NO_BOARD);
    }
    assert_eq!(
        blocks_of(&th.client, board_id).await.len(),
        expected_count,
        "only the allowed inserts stored blocks"
    );

    th.teardown().await;
}

/// The title and fields size limits, and what a refused block leaves behind.
#[tokio::test]
async fn post_blocks_size_limits() {
    let th = Setup::native().start().await.init_basic().await;
    let board = create_board_in_store(&th, BoardType::Private).await;

    let longest = new_block(
        &board.id,
        BlockType::Card,
        &"a".repeat(BLOCK_TITLE_MAX_CHARS),
    );
    insert_one(&th.client, &longest).await;

    let too_long = new_block(
        &board.id,
        BlockType::Card,
        &"a".repeat(BLOCK_TITLE_MAX_CHARS + 1),
    );
    let response = th
        .client
        .insert_blocks(&board.id, std::slice::from_ref(&too_long))
        .await;
    response.check_bad_request();
    let message = response.error_message();
    assert!(
        message.starts_with("error validating block ")
            && message.ends_with(": block title size limit exceeded"),
        "{message}"
    );
    assert!(
        !message.contains(&too_long.id),
        "the message names the generated identifier: {message}"
    );

    let too_big = Block {
        fields: fields(json!({"big": "a".repeat(BLOCK_FIELDS_MAX_CHARS)})),
        ..new_block(&board.id, BlockType::Card, "big fields")
    };
    let response = th.client.insert_blocks(&board.id, &[too_big]).await;
    response.check_bad_request();
    let message = response.error_message();
    assert!(
        message.ends_with(": block fields size limit exceeded"),
        "{message}"
    );

    // A request is stored whole or not at all. (The original stores blocks one by one
    // and keeps those before the refused one.)
    let blocks = [
        new_block(&board.id, BlockType::Card, "before"),
        too_long,
        new_block(&board.id, BlockType::Card, "after"),
    ];
    th.client
        .insert_blocks(&board.id, &blocks)
        .await
        .check_bad_request();
    let stored = blocks_of(&th.client, &board.id).await;
    assert_eq!(
        titles(&stored).len(),
        1,
        "nothing of the refused request, next to the block with the longest title"
    );

    th.teardown().await;
}

// -- PATCH /boards/{board_id}/blocks/{block_id} -----------------------------------------

/// The refusals of a patch, in the order the original checks: permission, block, body.
#[tokio::test]
async fn patch_block_refusals() {
    let th = Setup::native().start().await.init_basic().await;
    let boards = boards_with_roles(&th).await;
    let content = card_with_content(&th, &boards).await;
    let board_id = &boards.board.id;
    let card_path = block_path(board_id, &content.card.id);

    // A commenter changes their own comment only
    // (`a_comment_is_changed_by_its_author_or_an_admin`), a viewer nothing.
    for (who, client, path) in [
        ("commenter, card", &boards.commenter, card_path.clone()),
        ("viewer", &boards.viewer, card_path.clone()),
    ] {
        let response = client.patch(&path, &title_patch("x")).await;
        response.check_error(StatusCode::FORBIDDEN, NO_CHANGES);
        assert_eq!(response.problem_type(), "/problems/forbidden", "{who}");
    }
    // For who is no member of the private board it does not exist, whatever the block:
    // the outsider, and an editor of one board on another board's path.
    for (client, path) in [
        (&th.client2, card_path.clone()),
        (&th.client2, block_path(board_id, "no-such-block")),
        (
            &boards.editor,
            block_path(&boards.other_board.id, &content.card.id),
        ),
    ] {
        let response = client.patch(&path, &title_patch("x")).await;
        response.check_error(StatusCode::NOT_FOUND, NO_BOARD);
    }

    let response = th
        .client
        .patch(&block_path(board_id, "no-such-block"), &title_patch("x"))
        .await;
    response.check_error(StatusCode::NOT_FOUND, NO_BLOCK);

    let response = th
        .client
        .patch(
            &block_path(board_id, &content.other_card.id),
            &title_patch("x"),
        )
        .await;
    // A block of another board answers as one that does not exist.
    response.check_error(StatusCode::NOT_FOUND, NO_BLOCK);

    // A body that is no patch is a bad request. (The original answers 500.) A list is
    // no patch either, not even an empty one.
    for body in ["not json", "null", "{\"title\":5}", "[]", "[\"x\"]"] {
        let response = th
            .client
            .send_raw_body(Method::PATCH, &card_path, body)
            .await;
        response.check_bad_request();
        assert_eq!(response.problem_type(), "/problems/bad-request", "{body}");
    }

    let too_long = title_patch(&"a".repeat(BLOCK_TITLE_MAX_CHARS + 1));
    let response = th.client.patch(&card_path, &too_long).await;
    response.check_bad_request();
    assert_eq!(
        response.error_message(),
        format!(
            "error validating block {}: block title size limit exceeded",
            content.card.id
        ),
        "the title limit"
    );

    let stored = blocks_of(&th.client, board_id).await;
    assert_eq!(
        titles(&stored),
        ["card", "comment", "text"],
        "nothing was patched"
    );

    th.teardown().await;
}

/// What a patch changes and what it leaves alone.
#[tokio::test]
async fn patch_block_changes() {
    let th = Setup::native().start().await.init_basic().await;
    let boards = boards_with_roles(&th).await;
    let content = card_with_content(&th, &boards).await;
    let card = &content.card;
    let card_path = block_path(&boards.board.id, &card.id);
    let created = blocks_of(&th.client, &boards.board.id).await;
    let created_at = find(&created, &card.id).create_at;

    patch(&boards.editor, card, &title_patch("by the editor")).await;
    let stored = blocks_of(&th.client, &boards.board.id).await;
    let patched = find(&stored, &card.id);
    assert_eq!(patched.title, "by the editor", "the editor's title");
    assert_eq!(
        patched.modified_by,
        boards.editor.me().await.id,
        "modified by the editor"
    );
    assert!(patched.update_at > card.update_at, "at a later time");
    assert_eq!(patched.create_at, created_at, "created when it was");

    // The empty patch, and members a patch does not have, change nothing but the
    // modifier.
    patch(&th.client, card, &BlockPatch::default()).await;
    let foreign_members = json!({
        "boardId": boards.other_board.id,
        "id": "another",
        "createdBy": "someone",
        "deleteAt": 5,
    });
    th.client
        .patch(&format!("{card_path}?disableNotify=true"), &foreign_members)
        .await
        .check_no_content();
    let stored = blocks_of(&th.client, &boards.board.id).await;
    let patched = find(&stored, &card.id);
    assert_eq!(patched.title, "by the editor", "the title stays");
    assert_eq!(patched.board_id, boards.board.id, "a patch moves no block");
    assert_eq!(
        patched.created_by, card.created_by,
        "nor changes its creator"
    );
    assert_eq!(
        patched.modified_by,
        th.client.me().await.id,
        "the last patch was the admin's"
    );
    assert_eq!(
        titles(&blocks_of(&th.client, &boards.other_board.id).await),
        ["other"],
        "the other board is as it was"
    );

    th.teardown().await;
}

// -- PATCH /boards/{board_id}/blocks ----------------------------------------------------

/// What a batch patch refuses; none of the refused batches changes anything.
#[tokio::test]
async fn patch_blocks_batch_refusals() {
    let th = Setup::native().start().await.init_basic().await;
    let boards = boards_with_roles(&th).await;
    let content = card_with_content(&th, &boards).await;
    let (card, text) = (&content.card, &content.text);
    let path = blocks_path(&boards.board.id);
    let missing = new_block(&boards.board.id, BlockType::Card, "never stored");

    // A block that does not exist, and one of another board, are not found: the batch
    // is for the blocks of the board in the path. (The original answers 403 for the
    // first and does not look at the path's board at all.)
    for patches in [
        batch(&[(card, "card 2"), (&missing, "x")]),
        batch(&[(card, "card 2"), (&content.other_card, "other 2")]),
    ] {
        for client in [&th.client, &boards.editor] {
            let response = client.patch(&path, &patches).await;
            response.check_error(StatusCode::NOT_FOUND, NO_BLOCK);
        }
    }

    // Who may not change the board's cards is refused; for who is no member of the
    // private board it does not exist. (The original's refusal has a stray letter at
    // its end.)
    let response = boards
        .commenter
        .patch(&path, &batch(&[(card, "card 2")]))
        .await;
    response.check_error(StatusCode::FORBIDDEN, NO_CHANGES);
    let response = th.client2.patch(&path, &batch(&[(card, "card 2")])).await;
    response.check_error(StatusCode::NOT_FOUND, NO_BOARD);
    let response = th
        .client
        .patch("/boards/no-such-board/blocks", &batch(&[(card, "card 2")]))
        .await;
    response.check_error(StatusCode::NOT_FOUND, NO_BOARD);

    // An identifier listed twice. (The original counts it as a block that is missing.)
    let response = th
        .client
        .patch(&path, &batch(&[(card, "card 2"), (card, "card 3")]))
        .await;
    response.check_error(
        StatusCode::BAD_REQUEST,
        &format!("block {} is listed twice", card.id),
    );

    // As many patches as identifiers. (The original crashes on fewer patches and
    // ignores the extra ones.)
    let mut short = batch(&[(card, "card 2"), (text, "text 2")]);
    short.block_patches.pop();
    let mut long = batch(&[(card, "card 2")]);
    long.block_patches.push(title_patch("ignored"));
    for uneven in [&short, &long] {
        let response = th.client.patch(&path, uneven).await;
        response.check_error(
            StatusCode::BAD_REQUEST,
            "blockIds and blockPatches differ in length",
        );
    }

    let too_long = "a".repeat(BLOCK_TITLE_MAX_CHARS + 1);
    let response = th
        .client
        .patch(&path, &batch(&[(card, "card 2"), (text, &too_long)]))
        .await;
    response.check_bad_request();
    assert_eq!(
        response.error_message(),
        format!(
            "error validating block {}: block title size limit exceeded",
            text.id
        ),
        "the title limit"
    );

    // A body that is no batch is a bad request. (The original answers 500.)
    for body in ["not json", "null", "{\"blockIds\":5}", "[]", "[[], []]"] {
        let response = th.client.send_raw_body(Method::PATCH, &path, body).await;
        response.check_bad_request();
        assert_eq!(response.problem_type(), "/problems/bad-request", "{body}");
    }

    // A batch is all or none.
    let stored = blocks_of(&th.client, &boards.board.id).await;
    assert_eq!(
        titles(&stored),
        ["card", "comment", "text"],
        "nothing changed"
    );

    th.teardown().await;
}

/// What a batch patch changes, and the batches that change nothing.
#[tokio::test]
async fn patch_blocks_batch_changes() {
    let th = Setup::native().start().await.init_basic().await;
    let boards = boards_with_roles(&th).await;
    let content = card_with_content(&th, &boards).await;
    let (card, text) = (&content.card, &content.text);
    let path = blocks_path(&boards.board.id);

    let patches = batch(&[(card, "card 1"), (text, "text 1")]);
    assert_eq!(
        serde_json::to_value(&patches).expect("a batch is JSON"),
        json!({
            "blockIds": [card.id, text.id],
            "blockPatches": [
                {"title": "card 1", "updatedFields": {}, "deletedFields": []},
                {"title": "text 1", "updatedFields": {}, "deletedFields": []},
            ],
        }),
        "the members of a batch on the wire"
    );
    let response = boards.editor.patch(&path, &patches).await;
    response.check_no_content();
    let stored = blocks_of(&th.client, &boards.board.id).await;
    assert_eq!(
        titles(&stored),
        ["card 1", "comment", "text 1"],
        "both are patched"
    );
    assert_eq!(
        find(&stored, &card.id).modified_by,
        boards.editor.me().await.id,
        "by the editor"
    );

    // An empty batch, and one whose members are spelled differently, do nothing.
    for body in [
        json!({}),
        json!({"blockIds": [], "blockPatches": []}),
        json!({"block_ids": [card.id], "block_patches": [{"title": "snake"}]}),
    ] {
        let response = boards.editor.patch(&path, &body).await;
        assert_eq!(response.status, StatusCode::NO_CONTENT, "{body}");
        assert!(response.body.is_empty(), "{body}: {}", response.text());
    }
    let stored = blocks_of(&th.client, &boards.board.id).await;
    assert_eq!(
        titles(&stored),
        ["card 1", "comment", "text 1"],
        "none of them patched anything"
    );

    let quiet = format!("{path}?disableNotify=true");
    th.client
        .patch(&quiet, &batch(&[(text, "text 2"), (card, "card 2")]))
        .await
        .check_no_content();
    let stored = blocks_of(&th.client, &boards.board.id).await;
    assert_eq!(titles(&stored), ["card 2", "comment", "text 2"], "patched");
    let other = blocks_of(&th.client, &boards.other_board.id).await;
    assert_eq!(titles(&other), ["other"], "the other board is as it was");

    th.teardown().await;
}

// -- DELETE /boards/{board_id}/blocks/{block_id} ----------------------------------------

/// Deleting needs the permission to manage cards whatever the block is, and is refused
/// for blocks that are not on the board.
#[tokio::test]
async fn delete_block_refusals() {
    let th = Setup::native().start().await.init_basic().await;
    let boards = boards_with_roles(&th).await;
    let content = card_with_content(&th, &boards).await;
    let board_id = &boards.board.id;

    // Preserved quirk: a commenter cannot delete their own comment.
    for (who, client, path) in [
        (
            "commenter, own comment",
            &boards.commenter,
            block_path(board_id, &content.comment.id),
        ),
        (
            "viewer",
            &boards.viewer,
            block_path(board_id, &content.card.id),
        ),
    ] {
        let response = client.delete(&path).await;
        response.check_error(StatusCode::FORBIDDEN, NO_CHANGES);
        assert_eq!(response.problem_type(), "/problems/forbidden", "{who}");
    }
    // For who is no member of the private board it does not exist, whatever the block.
    for (client, path) in [
        (&th.client2, block_path(board_id, &content.card.id)),
        (&th.client2, block_path(board_id, "no-such-block")),
        (
            &boards.editor,
            block_path(&boards.other_board.id, &content.card.id),
        ),
    ] {
        let response = client.delete(&path).await;
        response.check_error(StatusCode::NOT_FOUND, NO_BOARD);
    }

    let response = th
        .client
        .delete(&block_path(board_id, "no-such-block"))
        .await;
    response.check_error(StatusCode::NOT_FOUND, NO_BLOCK);

    let response = th
        .client
        .delete(&block_path(board_id, &content.other_card.id))
        .await;
    // A block of another board answers as one that does not exist.
    response.check_error(StatusCode::NOT_FOUND, NO_BLOCK);

    assert_eq!(
        blocks_of(&th.client, board_id).await.len(),
        3,
        "nothing was deleted"
    );
    assert_eq!(
        blocks_of(&th.client, &boards.other_board.id).await.len(),
        1,
        "on the other board neither"
    );

    th.teardown().await;
}

/// Deleting a block takes its direct children along, images included, and a deleted
/// block is gone for the next deletion.
#[tokio::test]
async fn delete_block_takes_the_children_along() {
    let th = Setup::native().start().await.init_basic().await;
    let boards = boards_with_roles(&th).await;
    let content = card_with_content(&th, &boards).await;
    let board_id = &boards.board.id;
    let image = Block {
        fields: fields(json!({"fileId": "7abcdefghijklmnopqrstuvwxyz.png"})),
        ..child_of(&content.card, BlockType::Attachment, "image")
    };
    insert_one(&th.client, &image).await;
    insert_one(&th.client, &new_block(board_id, BlockType::Card, "card 2")).await;

    // The comment is the commenter's; an admin may delete it.
    delete(&th.client, &content.comment).await;
    let stored = blocks_of(&th.client, board_id).await;
    assert_eq!(
        titles(&stored),
        ["card", "card 2", "image", "text"],
        "the comment is gone"
    );

    // The card goes with all that is under it. The original cannot delete a card that
    // holds an image; that is a known difference of the store.
    let response = boards
        .editor
        .delete(&format!(
            "{}?disableNotify=true",
            block_path(board_id, &content.card.id)
        ))
        .await;
    response.check_no_content();
    let stored = blocks_of(&th.client, board_id).await;
    assert_eq!(titles(&stored), ["card 2"], "only the other card is left");

    for block in [&content.card, &content.text] {
        let response = th.client.delete(&block_path(board_id, &block.id)).await;
        response.check_error(StatusCode::NOT_FOUND, NO_BLOCK);
    }

    th.teardown().await;
}

// -- POST /boards/{board_id}/blocks/{block_id}/undelete ---------------------------------

/// The refusals of an undeletion in the original's order (board, block, permission),
/// and what it restores.
#[tokio::test]
async fn undelete_block_refusals_and_children() {
    let th = Setup::native().start().await.init_basic().await;
    let boards = boards_with_roles(&th).await;
    let content = card_with_content(&th, &boards).await;
    let (board_id, card) = (&boards.board.id, &content.card);
    delete(&th.client, card).await;

    let response = th
        .client
        .post_empty(&undelete_path("no-such-board", &card.id))
        .await;
    response.check_error(StatusCode::NOT_FOUND, NO_BOARD);

    // A block that never existed: the original crashes on it and answers 500; here it
    // is not there. For a user who is no member the board is not there either.
    let response = th
        .client
        .post_empty(&undelete_path(board_id, "no-such-block"))
        .await;
    response.check_error(StatusCode::NOT_FOUND, NO_BLOCK);
    let response = th
        .client2
        .post_empty(&undelete_path(board_id, "no-such-block"))
        .await;
    response.check_error(StatusCode::NOT_FOUND, NO_BOARD);

    let response = th
        .client
        .post_empty(&undelete_path(&boards.other_board.id, &card.id))
        .await;
    // A block of another board answers as one that does not exist.
    response.check_error(StatusCode::NOT_FOUND, NO_BLOCK);

    // (The original's refusal speaks of members.)
    for client in [&boards.commenter, &boards.viewer] {
        let response = client.post_empty(&undelete_path(board_id, &card.id)).await;
        response.check_error(StatusCode::FORBIDDEN, NO_CHANGES);
    }
    let response = th
        .client2
        .post_empty(&undelete_path(board_id, &card.id))
        .await;
    response.check_error(StatusCode::NOT_FOUND, NO_BOARD);
    assert!(
        blocks_of(&th.client, board_id).await.is_empty(),
        "still deleted"
    );

    let undeleted: Block = boards
        .editor
        .post_empty(&undelete_path(board_id, &card.id))
        .await
        .ok_json();
    assert_eq!(undeleted.id, card.id, "the card is answered");
    let stored = blocks_of(&th.client, board_id).await;
    assert_eq!(
        titles(&stored),
        ["card", "comment", "text"],
        "the children are back too"
    );

    // A block that is not deleted is not in the trash: there is nothing to restore.
    th.client
        .post_empty(&undelete_path(board_id, &card.id))
        .await
        .check_error(StatusCode::NOT_FOUND, "block not found");
    assert_eq!(
        blocks_of(&th.client, board_id).await.len(),
        3,
        "and nothing changes"
    );

    th.teardown().await;
}

// -- POST /boards/{board_id}/blocks/{block_id}/duplicate --------------------------------

/// A comment needs the permission to comment and everything else the permission to
/// manage cards; the board and the block are looked up first.
#[tokio::test]
async fn duplicate_block_refusals() {
    let th = Setup::native().start().await.init_basic().await;
    let boards = boards_with_roles(&th).await;
    let content = card_with_content(&th, &boards).await;
    let (board_id, card) = (&boards.board.id, &content.card);

    let response = th
        .client
        .post_empty(&duplicate_path("no-such-board", &card.id))
        .await;
    response.check_error(StatusCode::NOT_FOUND, NO_BOARD);

    let response = th
        .client
        .post_empty(&duplicate_path(board_id, "no-such-block"))
        .await;
    response.check_error(StatusCode::NOT_FOUND, NO_BLOCK);

    let response = th
        .client
        .post_empty(&duplicate_path(board_id, &content.other_card.id))
        .await;
    // A block of another board answers as one that does not exist.
    response.check_error(StatusCode::NOT_FOUND, NO_BLOCK);

    for client in [&boards.commenter, &boards.viewer] {
        let response = client.post_empty(&duplicate_path(board_id, &card.id)).await;
        response.check_error(StatusCode::FORBIDDEN, "access denied to modify board cards");
    }
    let response = th
        .client2
        .post_empty(&duplicate_path(board_id, &card.id))
        .await;
    response.check_error(StatusCode::NOT_FOUND, NO_BOARD);
    let comment_path = duplicate_path(board_id, &content.comment.id);
    let response = boards.viewer.post_empty(&comment_path).await;
    response.check_forbidden();
    assert_eq!(
        response.error_message(),
        "access denied to comment on board cards",
        "a viewer and a comment"
    );

    // A commenter may comment, but comments are never copied. (The original crashes on
    // this and answers 500.)
    let response = boards.commenter.post_empty(&comment_path).await;
    response.check_error(StatusCode::BAD_REQUEST, "a comment cannot be duplicated");

    assert_eq!(
        blocks_of(&th.client, board_id).await.len(),
        3,
        "nothing was copied"
    );
    th.teardown().await;
}

/// The original reads a request body of any size. Three blocks that are each within the
/// fields limit make a body of more than 2 MB, which the framework would refuse by
/// default.
#[tokio::test]
async fn post_blocks_larger_than_two_megabytes() {
    let th = Setup::one_user().start().await;
    let board = create_board_in_store(&th, BoardType::Open).await;
    // Views, whose fields may be that large.
    let big = |title: &str| Block {
        fields: fields(json!({"big": "a".repeat(750_000)})),
        ..new_block(&board.id, BlockType::View, title)
    };
    let blocks = [big("1"), big("2"), big("3")];
    let size = serde_json::to_vec(&blocks).expect("blocks are JSON").len();
    assert!(size > 2 * 1024 * 1024, "the body has {size} bytes");

    let inserted = insert(&th.client, &board.id, &blocks).await;
    assert_eq!(titles(&inserted), ["1", "2", "3"], "all three are answered");

    // A patch is not limited either.
    let grow = BlockPatch {
        updated_fields: fields(json!({"more": "b".repeat(40_000)})),
        ..BlockPatch::default()
    };
    patch(&th.client, &inserted[0], &grow).await;
    let stored = blocks_of(&th.client, &board.id).await;
    assert_eq!(stored.len(), 3, "all three are stored");

    th.teardown().await;
}

/// Duplicating a card copies it and its content, not its comments, under new
/// identifiers.
#[tokio::test]
async fn duplicate_block_copies() {
    let th = Setup::native().start().await.init_basic().await;
    let boards = boards_with_roles(&th).await;
    let content = card_with_content(&th, &boards).await;
    let (board_id, card) = (&boards.board.id, &content.card);
    let path = duplicate_path(board_id, &card.id);

    let copies: Vec<Block> = boards.editor.post_empty(&path).await.created_json();
    assert_eq!(
        titles(&copies),
        ["card", "text"],
        "the comment is not copied"
    );
    let (card_copy, text_copy) = (&copies[0], &copies[1]);
    assert_eq!(
        card_copy.block_type,
        BlockType::Card,
        "the block itself is first"
    );
    assert_ne!(card_copy.id, card.id, "under a new identifier");
    assert_ne!(text_copy.id, content.text.id, "the child too");
    assert_eq!(
        text_copy.parent_id, card_copy.id,
        "the child follows the copy"
    );
    assert_eq!(
        card_copy.fields["isTemplate"],
        json!(false),
        "an ordinary copy"
    );
    assert_eq!(
        card_copy.created_by,
        boards.editor.me().await.id,
        "the copy is the editor's"
    );

    let copies: Vec<Block> = th
        .client
        .post_empty(&format!("{path}?asTemplate=true"))
        .await
        .created_json();
    assert_eq!(copies[0].fields["isTemplate"], json!(true), "a template");

    let stored = blocks_of(&th.client, board_id).await;
    assert_eq!(
        titles(&stored),
        ["card", "card", "card", "comment", "text", "text", "text"],
        "both copies are stored"
    );
    assert_eq!(
        find(&stored, &card.id).fields.get("isTemplate"),
        None,
        "the source is untouched"
    );

    th.teardown().await;
}

// -- who is told about a change -----------------------------------------------------------

/// Every change of a block is broadcast to the board's members who listen to its team,
/// with or without `disableNotify`; except, as in the original, a batch patch.
#[tokio::test]
async fn block_changes_are_broadcast() {
    let th = Setup::native().start().await.init_basic().await;
    let board = create_board_in_store(&th, BoardType::Private).await;
    let mut member = Socket::subscribed_to_team(&th, &th.client, TEST_TEAM_ID).await;
    let mut outsider = Socket::subscribed_to_team(&th, &th.client2, TEST_TEAM_ID).await;
    let path = blocks_path(&board.id);

    // Insert: one message per block, in order.
    let card = new_block(&board.id, BlockType::Card, "card");
    let text = child_of(&card, BlockType::Attachment, "text");
    let inserted = insert(&th.client, &board.id, &[card, text]).await;
    let (card, text) = (&inserted[0], &inserted[1]);
    for block in [card, text] {
        let announced = member.next_block().await;
        assert_eq!(announced["id"], json!(block.id), "{announced}");
        assert_eq!(announced["title"], json!(block.title), "{announced}");
        assert_eq!(announced["boardId"], json!(board.id), "{announced}");
    }
    let quiet_card = new_block(&board.id, BlockType::Card, "quiet card");
    let quiet: Vec<Block> = th
        .client
        .post(&format!("{path}?disableNotify=true"), &[quiet_card])
        .await
        .created_json();
    let announced = member.next_block().await;
    assert_eq!(announced["id"], json!(quiet[0].id), "{announced}");

    // Patch.
    patch(&th.client, card, &title_patch("patched")).await;
    let announced = member.next_block().await;
    assert_eq!(announced["id"], json!(card.id), "{announced}");
    assert_eq!(announced["title"], json!("patched"), "{announced}");
    th.client
        .patch(
            &format!("{}?disableNotify=true", block_path(&board.id, &card.id)),
            &title_patch("patched quietly"),
        )
        .await
        .check_no_content();
    let announced = member.next_block().await;
    assert_eq!(announced["title"], json!("patched quietly"), "{announced}");

    // A batch patch is announced block by block, like single patches. (The original
    // broadcasts it to the team with the empty identifier, which nobody listens to.)
    th.client
        .patch(&path, &batch(&[(card, "batch 1"), (text, "batch 2")]))
        .await
        .check_no_content();
    let mut announced = vec![member.next_block().await, member.next_block().await];
    announced.sort_by_key(|block| block["title"].as_str().map(str::to_owned));
    assert_eq!(announced[0]["id"], json!(card.id), "{announced:?}");
    assert_eq!(announced[0]["title"], json!("batch 1"), "{announced:?}");
    assert_eq!(announced[1]["id"], json!(text.id), "{announced:?}");
    assert_eq!(announced[1]["title"], json!("batch 2"), "{announced:?}");

    // Duplicate: one message per copy.
    let copies: Vec<Block> = th
        .client
        .post_empty(&duplicate_path(&board.id, &card.id))
        .await
        .created_json();
    assert_eq!(copies.len(), 2, "the card and its text");
    for copy in &copies {
        let announced = member.next_block().await;
        assert_eq!(announced["id"], json!(copy.id), "{announced}");
    }

    // Delete: a block with only its identifiers and the deletion time.
    delete(&th.client, card).await;
    let announced = member.next_block().await;
    assert_eq!(announced["id"], json!(card.id), "{announced}");
    assert_eq!(announced["boardId"], json!(board.id), "{announced}");
    assert_eq!(announced["title"], json!(""), "{announced}");
    assert_ne!(announced["deleteAt"], json!(0), "{announced}");

    // Undelete: the block as restored.
    th.client
        .post_empty(&undelete_path(&board.id, &card.id))
        .await
        .check_ok();
    let announced = member.next_block().await;
    assert_eq!(announced["id"], json!(card.id), "{announced}");
    assert_eq!(announced["title"], json!("batch 1"), "{announced}");
    assert_eq!(announced["deleteAt"], json!(0), "{announced}");

    member.expect_quiet(&th, "nothing else was announced").await;
    outsider
        .expect_quiet(&th, "a user who is no member of the board hears nothing")
        .await;

    th.teardown().await;
}

/// Where the webhook test's receiver listens. The server's configuration is changed by
/// a plain function, which finds the address here.
static WEBHOOK_URL: OnceLock<String> = OnceLock::new();

/// Starts an HTTP server that keeps the JSON bodies posted to it, and puts its address
/// into [`WEBHOOK_URL`].
async fn start_webhook_receiver() -> (Arc<Mutex<Vec<Value>>>, tokio::task::JoinHandle<()>) {
    let posted: Arc<Mutex<Vec<Value>>> = Arc::default();
    let keep = {
        let posted = Arc::clone(&posted);
        move |axum::Json(block): axum::Json<Value>| async move {
            posted.lock().expect("lock poisoned").push(block);
        }
    };
    let hook = axum::Router::new().route("/hook", axum::routing::post(keep));
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("a local port is free");
    let address = listener.local_addr().expect("the listener has an address");
    WEBHOOK_URL
        .set(format!("http://{address}/hook"))
        .expect("only one test sets the webhook address");
    let serving = tokio::spawn(async move {
        axum::serve(listener, hook)
            .await
            .expect("the webhook receiver serves");
    });
    (posted, serving)
}

/// Inserted, patched and undeleted blocks are posted to the webhook; deletions and
/// duplicates are not.
#[tokio::test]
async fn block_changes_call_the_webhook() {
    let (posted, serving) = start_webhook_receiver().await;
    let th = Setup::native()
        .configure(|config| {
            config.webhook_update = WEBHOOK_URL.get().into_iter().cloned().collect();
        })
        .start()
        .await
        .init_basic()
        .await;
    let board = create_board_in_store(&th, BoardType::Private).await;
    // What the webhook got since the last call, as (identifier, title) pairs.
    let take_posted = async || -> Vec<(String, String)> {
        assert!(
            th.app.finish_background(BACKGROUND).await,
            "the webhook calls finish"
        );
        std::mem::take(&mut *posted.lock().expect("lock poisoned"))
            .iter()
            .map(|block| {
                let text = |member: &str| block[member].as_str().unwrap_or_default().to_owned();
                (text("id"), text("title"))
            })
            .collect()
    };
    let pair = |block: &Block, title: &str| (block.id.clone(), title.to_owned());
    let nothing: [(String, String); 0] = [];

    let card = new_block(&board.id, BlockType::Card, "card");
    let text = child_of(&card, BlockType::Attachment, "text");
    let inserted = insert(&th.client, &board.id, &[card, text]).await;
    let (card, text) = (&inserted[0], &inserted[1]);
    assert_eq!(
        take_posted().await,
        [pair(card, "card"), pair(text, "text")],
        "inserted blocks, in order"
    );

    th.client
        .patch(
            &format!("{}?disableNotify=true", block_path(&board.id, &card.id)),
            &title_patch("patched"),
        )
        .await
        .check_no_content();
    assert_eq!(
        take_posted().await,
        [pair(card, "patched")],
        "a patched block"
    );

    th.client
        .patch(
            &blocks_path(&board.id),
            &batch(&[(card, "batch 1"), (text, "batch 2")]),
        )
        .await
        .check_no_content();
    assert_eq!(
        take_posted().await,
        [pair(card, "batch 1"), pair(text, "batch 2")],
        "the blocks of a batch patch, in order"
    );

    th.client
        .post_empty(&duplicate_path(&board.id, &text.id))
        .await
        .check_created();
    assert_eq!(take_posted().await, nothing, "duplicates are not posted");

    delete(&th.client, text).await;
    assert_eq!(take_posted().await, nothing, "deletions are not posted");

    th.client
        .post_empty(&undelete_path(&board.id, &text.id))
        .await
        .check_ok();
    assert_eq!(
        take_posted().await,
        [pair(text, "batch 2")],
        "an undeleted block"
    );

    th.teardown().await;
    serving.abort();
}

/// Security fix: `sourceBoardId` makes the server copy the files the inserted blocks
/// name from that board. The original does not check that the user may read it
/// (`server/api/blocks.go:297-303`): on the original, a user who is on neither board of
/// another user got the file copied in storage. The port refuses before anything is
/// copied or inserted.
#[tokio::test]
async fn post_blocks_with_a_source_board_the_user_cannot_read_is_refused() {
    let th = Setup::native().start().await.init_basic().await;
    let foreign = create_board_in_store(&th, BoardType::Private).await;
    let outsider = &th.client2;
    let outsider_id = outsider.me().await.id;
    let mut own = Board {
        id: new_id(),
        team_id: TEST_TEAM_ID.to_owned(),
        board_type: BoardType::Private,
        ..Board::default()
    };
    th.app
        .store()
        .insert_board_with_admin(&mut own, &outsider_id)
        .await
        .expect("the outsider's own board is stored");

    let image = Block {
        fields: fields(json!({"fileId": "7aaaaaaaaaaaaaaaaaaaaaaaaaa.png"})),
        ..new_block(&own.id, BlockType::Attachment, "")
    };
    for source in [foreign.id.as_str(), "no-such-board"] {
        let response = outsider
            .request(
                Method::POST,
                &format!("/boards/{}/blocks?sourceBoardId={source}", own.id),
            )
            .json(&[&image])
            .send()
            .await
            .expect("the server answers");
        // One answer for a source that is not there and one the user cannot read.
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "source {source}");
        let body: Value = response.json().await.expect("an error body");
        assert_eq!(
            body["type"],
            json!("/problems/not-found"),
            "source {source}"
        );
        assert_eq!(
            body["detail"],
            json!("source board not found"),
            "source {source}"
        );
    }
    let stored = th
        .app
        .blocks_for_board(&own.id)
        .await
        .expect("the board's blocks are read");
    assert!(stored.is_empty(), "nothing was inserted: {stored:?}");
}

/// The legitimate use of `sourceBoardId`: copying from a board the user is on.
#[tokio::test]
async fn post_blocks_with_a_source_board_the_user_can_read_still_works() {
    let th = Setup::native().start().await.init_basic().await;
    let source = create_board_in_store(&th, BoardType::Private).await;
    let target = create_board_in_store(&th, BoardType::Private).await;

    let card = new_block(&target.id, BlockType::Card, "copied card");
    let response = th
        .client
        .request(
            Method::POST,
            &format!("/boards/{}/blocks?sourceBoardId={}", target.id, source.id),
        )
        .json(&[&card])
        .send()
        .await
        .expect("the server answers");
    assert_eq!(
        response.status(),
        StatusCode::CREATED,
        "a member copies from their own board"
    );
    let inserted: Vec<Block> = response.json().await.expect("the inserted blocks");
    assert_eq!(inserted.len(), 1, "the card was inserted");
    assert_eq!(inserted[0].title, "copied card", "as sent");
}

/// Not in the original, where every editor can delete anyone's comment
/// (`server/api/blocks.go:372`; on the original, an editor's `DELETE` of another user's
/// comment answers 200). A comment is deleted by its author or by an admin.
#[tokio::test]
async fn delete_comment_of_another_user_needs_the_permission_to_delete_others_comments() {
    let th = Setup::native().start().await.init_basic().await;
    let boards = boards_with_roles(&th).await;
    let content = card_with_content(&th, &boards).await;
    let board_id = &boards.board.id;

    // The comment was written by the commenter.
    let response = boards
        .editor
        .delete(&block_path(board_id, &content.comment.id))
        .await;
    response.check_forbidden();
    assert_eq!(
        response.error_message(),
        "access denied to delete other users' comments",
        "an editor is refused"
    );
    let stored = blocks_of(&th.client, board_id).await;
    assert!(
        titles(&stored).contains(&"comment"),
        "the comment is still there: {:?}",
        titles(&stored)
    );

    // Other blocks of other users are deleted by an editor as before.
    delete(&boards.editor, &content.text).await;
}

/// The cases that stay allowed: an editor deletes a comment of their own, and an admin
/// deletes the comment of another user.
#[tokio::test]
async fn delete_comment_by_its_author_or_by_an_admin_works() {
    let th = Setup::native().start().await.init_basic().await;
    let boards = boards_with_roles(&th).await;
    let content = card_with_content(&th, &boards).await;
    let board_id = &boards.board.id;

    let own = child_of(&content.card, BlockType::Comment, "the editor's comment");
    let own = insert_one(&boards.editor, &own).await;
    delete(&boards.editor, &own).await;

    // The commenter's comment, deleted by the board's admin.
    delete(&th.client, &content.comment).await;

    let stored = blocks_of(&th.client, board_id).await;
    assert_eq!(titles(&stored), ["card", "text"], "both comments are gone");

    // Preserved quirk: the author of a comment who may not manage cards is still
    // refused, by the check that comes first.
    let comment = child_of(&content.card, BlockType::Comment, "another comment");
    let comment = insert_one(&boards.commenter, &comment).await;
    let response = boards
        .commenter
        .delete(&block_path(board_id, &comment.id))
        .await;
    response.check_forbidden();
    assert_eq!(
        response.error_message(),
        "access denied to make board changes",
        "a commenter cannot delete at all"
    );
}

/// A comment is changed by its author with the commenter's role, and the comment of
/// somebody else by the board's admins only, as it is deleted; an editor may not change
/// it. `patchBlock`, `patchBlocks` and `patchBoardsAndBlocks` alike. Who changed it and
/// when is what its `modifiedBy` and `updateAt` say to the members.
#[tokio::test]
async fn a_comment_is_changed_by_its_author_or_an_admin() {
    let th = Setup::native().start().await.init_basic().await;
    let boards = boards_with_roles(&th).await;
    let content = card_with_content(&th, &boards).await;
    let board_id = &boards.board.id;
    let commenter = boards.commenter.me().await;
    let admin = th.client.me().await;
    let comment = &content.comment;
    let path = block_path(board_id, &comment.id);
    let many = format!("/boards/{board_id}/blocks");
    let together = |patch: Value| {
        json!({
            "boardIds": [board_id], "boardPatches": [{}],
            "blockIds": [comment.id], "blockPatches": [patch],
        })
    };
    let others_refused = "access denied to change other users' comments";
    let stored = |blocks: &[Block]| {
        blocks
            .iter()
            .find(|block| block.id == comment.id)
            .cloned()
            .expect("the comment is there")
    };

    // The author, with no more than the commenter's role, on both routes.
    let mut member = Socket::subscribed_to_team(&th, &th.client, TEST_TEAM_ID).await;
    boards
        .commenter
        .patch(&path, &title_patch("by its author"))
        .await
        .check_no_content();
    let told = member.next_block().await;
    let read = stored(&blocks_of(&th.client, board_id).await);
    assert_eq!(read.title, "by its author");
    assert_eq!(read.modified_by, commenter.id, "who changed it");
    assert!(read.update_at > comment.update_at, "and when: later");
    assert_eq!(told["modifiedBy"], json!(commenter.id), "{told}");
    assert_eq!(told["updateAt"], json!(read.update_at), "{told}");
    boards
        .commenter
        .patch(&many, &batch(&[(comment, "again by its author")]))
        .await
        .check_no_content();
    th.client
        .patch(&many, &batch(&[(comment, "by the admin, many")]))
        .await
        .check_no_content();
    let read = stored(&blocks_of(&th.client, board_id).await);
    assert_eq!(
        read.modified_by, admin.id,
        "patchBlocks says who changed it"
    );
    boards
        .commenter
        .patch(&many, &batch(&[(comment, "again by its author")]))
        .await
        .check_no_content();

    // An editor is refused, on every route, and nothing changes.
    let response = boards
        .editor
        .patch(&path, &title_patch("by an editor"))
        .await;
    response.check_error(StatusCode::FORBIDDEN, others_refused);
    let response = boards
        .editor
        .patch(&many, &batch(&[(comment, "by an editor")]))
        .await;
    response.check_error(StatusCode::FORBIDDEN, others_refused);
    let response = boards
        .editor
        .patch(
            "/boards-and-blocks",
            &together(json!({"title": "by an editor"})),
        )
        .await;
    response.check_error(StatusCode::FORBIDDEN, others_refused);
    let read = stored(&blocks_of(&th.client, board_id).await);
    assert_eq!(
        read.title, "again by its author",
        "the editors changed nothing"
    );
    assert_eq!(read.modified_by, commenter.id);

    // The board's admin changes it, and is who changed it last.
    while member.next_json(QUIET).await.is_some() {}
    th.client
        .patch(&path, &title_patch("by the admin"))
        .await
        .check_no_content();
    let told = member.next_block().await;
    assert_eq!(told["modifiedBy"], json!(admin.id), "{told}");
    th.client
        .patch(
            "/boards-and-blocks",
            &together(json!({"title": "by the admin again"})),
        )
        .await
        .check_ok();
    let read = stored(&blocks_of(&th.client, board_id).await);
    assert_eq!(read.title, "by the admin again");
    assert_eq!(read.modified_by, admin.id);
}

/// The commenter's role changes the commenter's own comment, not a card, and makes no
/// card of a comment; a viewer changes nothing.
#[tokio::test]
async fn a_commenter_changes_no_card_and_makes_none() {
    let th = Setup::native().start().await.init_basic().await;
    let boards = boards_with_roles(&th).await;
    let content = card_with_content(&th, &boards).await;
    let board_id = &boards.board.id;
    let path = block_path(board_id, &content.comment.id);
    let many = format!("/boards/{board_id}/blocks");

    boards
        .commenter
        .patch(&path, &json!({"type": "card"}))
        .await
        .check_error(StatusCode::FORBIDDEN, "access denied to make board changes");
    boards
        .commenter
        .patch(
            &block_path(board_id, &content.card.id),
            &title_patch("a card"),
        )
        .await
        .check_error(StatusCode::FORBIDDEN, "access denied to make board changes");
    let own = insert_one(
        &boards.commenter,
        &child_of(&content.card, BlockType::Comment, "own"),
    )
    .await;
    boards
        .commenter
        .patch(
            &many,
            &batch(&[(&own, "own, changed"), (&content.card, "x")]),
        )
        .await
        .check_error(StatusCode::FORBIDDEN, "access denied to make board changes");
    boards
        .commenter
        .patch(
            &many,
            &json!({"blockIds": [own.id], "blockPatches": [{"type": "card"}]}),
        )
        .await
        .check_error(StatusCode::FORBIDDEN, "access denied to make board changes");
    // A viewer changes nothing.
    boards
        .viewer
        .patch(&block_path(board_id, &own.id), &title_patch("by a viewer"))
        .await
        .check_error(StatusCode::FORBIDDEN, "access denied to make board changes");
    let read = blocks_of(&th.client, board_id).await;
    assert!(
        titles(&read).contains(&"own") && titles(&read).contains(&"card"),
        "nothing changed: {:?}",
        titles(&read)
    );
}

/// A block of somebody else is made a comment by the board's admins only: a comment
/// says who wrote it, and an editor makes nobody else say anything. The author of a
/// block makes it a comment with the editor's role.
#[tokio::test]
async fn a_block_of_somebody_else_is_made_a_comment_by_an_admin() {
    let th = Setup::native().start().await.init_basic().await;
    let boards = boards_with_roles(&th).await;
    let content = card_with_content(&th, &boards).await;
    let board_id = &boards.board.id;
    let into_comment = json!({"type": "comment", "title": "I approve"});
    let refused = "access denied to change other users' comments";

    boards
        .editor
        .patch(&block_path(board_id, &content.text.id), &into_comment)
        .await
        .check_error(StatusCode::FORBIDDEN, refused);
    boards
        .editor
        .patch(
            &format!("/boards/{board_id}/blocks"),
            &json!({"blockIds": [content.text.id], "blockPatches": [into_comment]}),
        )
        .await
        .check_error(StatusCode::FORBIDDEN, refused);
    let own = insert_one(
        &boards.editor,
        &child_of(&content.card, BlockType::Attachment, "the editor's"),
    )
    .await;
    boards
        .editor
        .patch(&block_path(board_id, &own.id), &into_comment)
        .await
        .check_no_content();
    th.client
        .patch(&block_path(board_id, &content.text.id), &into_comment)
        .await
        .check_no_content();
}

// -- the types of new blocks and the limits of their fields ------------------------------

/// A new block is a card, a view, a comment or an attachment: the content blocks went
/// with the card's content order (`docs/s6-design.md` 5.7). Any other type is refused
/// as the type on the routes that make blocks, and nothing is stored.
#[tokio::test]
async fn a_new_block_of_another_type_is_refused() {
    let th = Setup::one_user().start().await;
    let board = create_board_in_store(&th, BoardType::Open).await;
    let card = insert_one(&th.client, &new_block(&board.id, BlockType::Card, "card")).await;
    for name in [
        "text",
        "image",
        "checkbox",
        "divider",
        "board",
        "h1",
        "list-item",
    ] {
        let fine = child_of(&card, BlockType::Attachment, "fine");
        let block = child_of(&card, BlockType::from(name), name);
        let response = th.client.insert_blocks(&board.id, &[fine, block]).await;
        response.check_bad_request();
        assert_eq!(response.refused_at(), "/type", "{name}");

        let response = th
            .client
            .post(
                "/boards-and-blocks",
                &json!({
                    "boards": [{"id": "b1", "teamId": TEST_TEAM_ID, "type": "O"}],
                    "blocks": [{
                        "id": "x1", "boardId": "b1", "type": name, "createAt": 1, "updateAt": 1,
                    }],
                }),
            )
            .await;
        response.check_bad_request();
        assert_eq!(response.refused_at(), "/type", "{name} with a board");
    }
    assert_eq!(
        ids(&blocks_of(&th.client, &board.id).await),
        [card.id.as_str()],
        "the card alone"
    );
    th.teardown().await;
}

/// A patch, alone or in a batch, may not give a block one of those types.
#[tokio::test]
async fn a_patch_does_not_make_a_block_of_another_type() {
    let th = Setup::one_user().start().await;
    let board = create_board_in_store(&th, BoardType::Open).await;
    let card = insert_one(&th.client, &new_block(&board.id, BlockType::Card, "card")).await;
    let to_text = BlockPatch {
        block_type: Some(BlockType::from("text")),
        ..BlockPatch::default()
    };
    let response = th
        .client
        .patch(&block_path(&board.id, &card.id), &to_text)
        .await;
    response.check_bad_request();
    assert_eq!(response.refused_at(), "/type", "one patch");
    let batch = BlockPatchBatch {
        block_ids: vec![card.id.clone()],
        block_patches: vec![to_text.clone()],
    };
    let response = th.client.patch(&blocks_path(&board.id), &batch).await;
    response.check_bad_request();
    assert_eq!(response.refused_at(), "/type", "a batch");
    assert_eq!(
        blocks_of(&th.client, &board.id).await[0].block_type,
        BlockType::Card,
        "still a card"
    );

    th.teardown().await;
}

/// The serialized fields of a card, a comment or an attachment have at most 64,000
/// characters, those of a view 800,000 (`docs/s6-design.md` 5.7), when the block is made
/// and when it is patched.
#[tokio::test]
async fn the_fields_of_a_block_are_limited_by_its_type() {
    let th = Setup::one_user().start().await;
    let board = create_board_in_store(&th, BoardType::Open).await;
    let card = insert_one(&th.client, &new_block(&board.id, BlockType::Card, "card")).await;
    // {"k":"<value>"} is the value and eight characters.
    let sized = |block_type: BlockType, len: usize| Block {
        fields: fields(json!({"k": "a".repeat(len)})),
        ..child_of(&card, block_type, "sized")
    };
    for (block_type, limit) in [
        (BlockType::Card, CARD_FIELDS_MAX_CHARS),
        (BlockType::Comment, CARD_FIELDS_MAX_CHARS),
        (BlockType::Attachment, CARD_FIELDS_MAX_CHARS),
        (BlockType::View, BLOCK_FIELDS_MAX_CHARS),
    ] {
        insert(
            &th.client,
            &board.id,
            &[sized(block_type.clone(), limit - 8)],
        )
        .await;
        let response = th
            .client
            .insert_blocks(&board.id, &[sized(block_type.clone(), limit - 7)])
            .await;
        response.check_bad_request();
        assert!(
            response.value()["detail"]
                .as_str()
                .is_some_and(|message| message.contains("block fields size limit exceeded")),
            "{block_type} over the limit: {}",
            response.value()
        );
    }

    let grow = BlockPatch {
        updated_fields: fields(json!({"more": "b".repeat(CARD_FIELDS_MAX_CHARS)})),
        ..BlockPatch::default()
    };
    let response = th
        .client
        .patch(&block_path(&board.id, &card.id), &grow)
        .await;
    response.check_bad_request();
    let stored = th.app.block(&card.id).await.expect("the card is stored");
    assert!(!stored.fields.contains_key("more"), "the card is as it was");
    th.teardown().await;
}
