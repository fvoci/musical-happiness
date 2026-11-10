//! Ports of `server/integrationtests/board_test.go` and `server/app/boards_test.go`, plus
//! tests of what was verified on the original while porting.
//!
//! Not ported here:
//!
//! - `TestGetAllBlocksForBoard`, `TestSearchBoards` and `TestGetTemplates` use routes of
//!   the blocks, search and templates areas; duplicating a template is covered by
//!   [`duplicate_board_from_a_template`].
//! - `TestUpdateMember/"should always disable the admin role on update member if the user
//!   is a guest"` runs in the host-application mode; a standalone server has no guests.
//! - `TestGetBoardMetadata/"getBoardMetadata query is correct"` (with its "After delete
//!   board") and `"getBoardMetadata should fail on Professional license"` need a license,
//!   which a standalone server never has.
//! - From `server/app/boards_test.go`: `TestAddMemberToBoard/"should convert synthetic
//!   membership into natural membership"` (no synthetic memberships), the "-team check
//!   true" cases of `TestGetMembersForBoard` and `TestGetMembersForUser` (nobody
//!   administers the team) and `TestGetBoardCount` (a pass-through to the store, tested
//!   there).
//!
//! Where the original arranges its data through routes of other areas (inserting blocks,
//! sharing a board, creating categories), these tests arrange it through the store or
//! the application layer.

use std::time::Duration;

use futures_util::{SinkExt as _, StreamExt as _};
use fvoci_model::{
    Block, BlockType, Board, BoardMember, BoardPatch, BoardRole, BoardType, BoardsAndBlocks,
    Category, CategoryBoards, IdKind, JsonMap, Session, Sharing, new_id,
};
use fvoci_store::{BlockFilter, DuplicateAs, PublicBoards};
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

use crate::common::{ApiResponse, Client, SINGLE_USER_TOKEN, Setup, TEST_TEAM_ID, TestServer};

// -- helpers --------------------------------------------------------------------------

pub(crate) fn new_board(team_id: &str, board_type: BoardType, title: &str) -> Board {
    Board {
        team_id: team_id.to_owned(),
        board_type,
        title: title.to_owned(),
        ..Board::default()
    }
}

/// `th.Server.App().CreateBoard(board, userID, true)`
pub(crate) async fn board_with_admin(th: &TestServer, board: Board, user_id: &str) -> Board {
    th.app
        .create_board_with_admin(board, user_id)
        .await
        .expect("the board is created with its creator as admin")
}

/// `th.Server.App().CreateBoard(board, userID, false)`
pub(crate) async fn board_without_members(th: &TestServer, board: Board, user_id: &str) -> Board {
    th.app
        .create_board(board, user_id)
        .await
        .expect("the board is created without members")
}

pub(crate) fn member(board_id: &str, user_id: &str) -> BoardMember {
    BoardMember {
        board_id: board_id.to_owned(),
        user_id: user_id.to_owned(),
        ..BoardMember::default()
    }
}

fn editor(board_id: &str, user_id: &str) -> BoardMember {
    BoardMember {
        scheme_editor: true,
        ..member(board_id, user_id)
    }
}

/// `th.Server.App().AddMemberToBoard(member)`
async fn add_member(th: &TestServer, member: &BoardMember) -> BoardMember {
    th.app
        .add_member_to_board(member)
        .await
        .expect("the member is added")
        .expect("the board exists")
}

/// `th.Server.App().GetMembersForBoard(boardID)`
pub(crate) async fn members_of(th: &TestServer, board_id: &str) -> Vec<BoardMember> {
    th.app
        .members_for_board(board_id)
        .await
        .expect("the members are read")
}

/// `th.Server.App().InsertBlock(block, userID)`: stores a block without going through the
/// blocks routes.
pub(crate) async fn insert_block(th: &TestServer, mut block: Block, user_id: &str) -> Block {
    th.app
        .store()
        .insert_block(&mut block, user_id)
        .await
        .expect("the block is stored");
    block
}

fn view_block(board_id: &str) -> Block {
    Block {
        id: new_id(IdKind::Block),
        board_id: board_id.to_owned(),
        create_at: 1,
        update_at: 1,
        title: "View 1".to_owned(),
        block_type: BlockType::View,
        ..Block::default()
    }
}

fn card_block(board_id: &str) -> Block {
    Block {
        block_type: BlockType::Card,
        title: "Card 1".to_owned(),
        ..view_block(board_id)
    }
}

async fn get_board(client: &Client, board_id: &str) -> ApiResponse {
    client.get(&format!("/boards/{board_id}")).await
}

async fn get_members(client: &Client, board_id: &str) -> ApiResponse {
    client.get(&format!("/boards/{board_id}/members")).await
}

async fn update_member(client: &Client, member: &BoardMember) -> ApiResponse {
    client
        .put(
            &format!("/boards/{}/members/{}", member.board_id, member.user_id),
            member,
        )
        .await
}

async fn delete_member(client: &Client, member: &BoardMember) -> ApiResponse {
    client
        .delete(&format!(
            "/boards/{}/members/{}",
            member.board_id, member.user_id
        ))
        .await
}

async fn join_board(client: &Client, board_id: &str) -> ApiResponse {
    client.post_empty(&format!("/boards/{board_id}/join")).await
}

async fn leave_board(client: &Client, board_id: &str) -> ApiResponse {
    client
        .post_empty(&format!("/boards/{board_id}/leave"))
        .await
}

async fn duplicate_board(client: &Client, board_id: &str, query: &str) -> ApiResponse {
    client
        .post_empty(&format!("/boards/{board_id}/duplicate{query}"))
        .await
}

/// A user's categories in a team, each with the identifiers of its boards.
async fn categories_of(th: &TestServer, user_id: &str, team_id: &str) -> Vec<CategoryBoards> {
    th.app
        .user_category_boards(user_id, team_id)
        .await
        .expect("the categories are read")
}

/// The name of the category a user has filed a board under, if any.
async fn category_name_of(
    th: &TestServer,
    user_id: &str,
    team_id: &str,
    board_id: &str,
) -> Option<String> {
    categories_of(th, user_id, team_id)
        .await
        .into_iter()
        .find(|entry| {
            entry
                .board_metadata
                .iter()
                .any(|metadata| metadata.board_id == board_id)
        })
        .map(|entry| entry.category.name)
}

/// Turns on public sharing of a board with the given read token; the counterpart of
/// `th.Client.PostSharing(sharing)`.
async fn share_board(th: &TestServer, board_id: &str, token: &str) {
    let sharing = Sharing {
        id: board_id.to_owned(),
        enabled: true,
        token: token.to_owned(),
        update_at: 1,
        ..Sharing::default()
    };
    th.app
        .store()
        .upsert_sharing(&sharing)
        .await
        .expect("the sharing record is stored");
}

fn sorted_ids(boards: &[Board]) -> Vec<String> {
    let mut ids: Vec<String> = boards.iter().map(|board| board.id.clone()).collect();
    ids.sort();
    ids
}

// -- TestGetBoards --------------------------------------------------------------------

/// `TestGetBoards/"a non authenticated client should be rejected"`
#[tokio::test]
async fn get_boards_a_non_authenticated_client_should_be_rejected() {
    let th = Setup::native().start().await.init_basic().await;
    board_without_members(&th, new_board("0", BoardType::Open, ""), "user-id").await;

    let response = th.anonymous().get("/teams/0/boards").await;

    response.check_unauthorized();
    th.teardown().await;
}

/// `TestGetBoards/"should only return the boards that the user is a member of"`
#[tokio::test]
async fn get_boards_should_only_return_the_boards_that_the_user_is_a_member_of() {
    let th = Setup::native().start().await.init_basic().await;
    let team_id = "0";
    let other_team_id = "other-team-id";
    let user1 = th.client.me().await;
    let user2 = th.client2.me().await;

    let board1 = board_with_admin(
        &th,
        new_board(team_id, BoardType::Open, "Board 1"),
        &user1.id,
    )
    .await;
    let board2 = board_without_members(
        &th,
        new_board(team_id, BoardType::Open, "Board 2"),
        &user2.id,
    )
    .await;
    let board3 = board_with_admin(
        &th,
        new_board(team_id, BoardType::Private, "Board 3"),
        &user1.id,
    )
    .await;
    board_without_members(
        &th,
        new_board(team_id, BoardType::Private, "Board 4"),
        &user1.id,
    )
    .await;
    board_with_admin(
        &th,
        new_board(team_id, BoardType::Private, "Board 5"),
        &user2.id,
    )
    .await;
    let board6 = board_with_admin(
        &th,
        new_board(other_team_id, BoardType::Open, ""),
        &user1.id,
    )
    .await;

    let mut listed: Vec<Board> = th.client.get("/teams/0/boards").await.ok_json();
    listed.sort_by(|a, b| a.title.cmp(&b.title));
    assert_eq!(
        listed,
        vec![board1, board2, board3],
        "the user's boards and the open boards of the team"
    );

    let from_other_team: Vec<Board> = th
        .client
        .get(&format!("/teams/{other_team_id}/boards"))
        .await
        .ok_json();
    assert_eq!(
        sorted_ids(&from_other_team),
        vec![board6.id],
        "only the board of the other team"
    );

    th.teardown().await;
}

/// Verified on the original: a team without boards answers with an empty list, not
/// `null`.
#[tokio::test]
async fn get_boards_of_a_team_without_boards_is_an_empty_list() {
    let th = Setup::native().start().await.init_basic().await;

    let response = th.client.get("/teams/empty-team/boards").await;

    response.check_ok();
    assert_eq!(response.text(), "[]", "an empty JSON list");
    th.teardown().await;
}

// -- TestCreateBoard ------------------------------------------------------------------

/// `TestCreateBoard/"a non authenticated user should be rejected"`
#[tokio::test]
async fn create_board_a_non_authenticated_user_should_be_rejected() {
    let th = Setup::native().start().await.init_basic().await;

    let response = th
        .anonymous()
        .create_board(&new_board(TEST_TEAM_ID, BoardType::Open, "board title"))
        .await;

    response.check_unauthorized();
    th.teardown().await;
}

/// `TestCreateBoard/"create public board"`, with its subtests about the board. The
/// subtests' assertions about the board's blocks are in
/// [`create_board_create_public_board_and_its_blocks`].
#[tokio::test]
async fn create_board_create_public_board() {
    let th = Setup::native().start().await.init_basic().await;
    let me = th.client.me().await;
    let title = "board title 1";

    let board: Board = th
        .client
        .create_board(&new_board(TEST_TEAM_ID, BoardType::Open, title))
        .await
        .ok_json();
    assert_eq!(board.id.len(), 27, "the board got an identifier");
    assert!(
        board.id.starts_with('b'),
        "a board identifier: {}",
        board.id
    );
    assert_eq!(board.title, title, "the title is kept");
    assert_eq!(board.board_type, BoardType::Open, "the type is kept");
    assert_eq!(board.team_id, TEST_TEAM_ID, "the team is kept");
    assert_eq!(board.created_by, me.id, "created by the session's user");
    assert_eq!(board.modified_by, me.id, "modified by the session's user");

    // "creating a board should make the creator an admin"
    let members = members_of(&th, &board.id).await;
    assert_eq!(members.len(), 1, "one member: {members:?}");
    assert_eq!(members[0].user_id, me.id, "the creator is the member");
    assert_eq!(members[0].board_id, board.id, "of this board");
    assert!(members[0].scheme_admin, "as an admin");

    // "creator should be able to access the public board and its blocks"
    let read: Board = get_board(&th.client, &board.id).await.ok_json();
    assert_eq!(read, board, "the creator reads the board back");

    // "A non-member user should be able to access the public board but not its blocks"
    let read: Board = get_board(&th.client2, &board.id).await.ok_json();
    assert_eq!(read, board, "another user of the team reads an open board");

    th.teardown().await;
}

/// The assertions of `TestCreateBoard/"create public board"` about the board's blocks.
#[tokio::test]
async fn create_board_create_public_board_and_its_blocks() {
    let th = Setup::native().start().await.init_basic().await;
    let board: Board = th
        .client
        .create_board(&new_board(TEST_TEAM_ID, BoardType::Open, "board title 1"))
        .await
        .ok_json();

    // "creator should be able to access the public board and its blocks"
    let blocks = th.client.get(&format!("/boards/{}/blocks", board.id)).await;
    assert_eq!(blocks.ok_json::<Vec<Block>>(), vec![], "no blocks yet");

    // "A non-member user should be able to access the public board but not its blocks"
    th.client2
        .get(&format!("/boards/{}/blocks", board.id))
        .await
        .check_forbidden();

    th.teardown().await;
}

/// `TestCreateBoard/"create private board"`, with its subtests about the board.
#[tokio::test]
async fn create_board_create_private_board() {
    let th = Setup::native().start().await.init_basic().await;
    let me = th.client.me().await;
    let title = "private board title";

    let board: Board = th
        .client
        .create_board(&new_board(TEST_TEAM_ID, BoardType::Private, title))
        .await
        .ok_json();
    assert_eq!(board.id.len(), 27, "the board got an identifier");
    assert_eq!(board.title, title, "the title is kept");
    assert_eq!(board.board_type, BoardType::Private, "the type is kept");
    assert_eq!(board.team_id, TEST_TEAM_ID, "the team is kept");
    assert_eq!(board.created_by, me.id, "created by the session's user");
    assert_eq!(board.modified_by, me.id, "modified by the session's user");

    // "creating a board should make the creator an admin"
    let members = members_of(&th, &board.id).await;
    assert_eq!(members.len(), 1, "one member: {members:?}");
    assert_eq!(members[0].user_id, me.id, "the creator is the member");
    assert_eq!(members[0].board_id, board.id, "of this board");
    assert!(members[0].scheme_admin, "as an admin");

    // "creator should be able to access the private board and its blocks"
    let read: Board = get_board(&th.client, &board.id).await.ok_json();
    assert_eq!(read, board, "the creator reads the board back");

    // "unauthorized user should not be able to access the private board or its blocks":
    // for them it does not exist.
    get_board(&th.client2, &board.id)
        .await
        .check_error(StatusCode::NOT_FOUND, "board not found");

    th.teardown().await;
}

/// The assertions of `TestCreateBoard/"create private board"` about the board's blocks.
#[tokio::test]
async fn create_board_create_private_board_and_its_blocks() {
    let th = Setup::native().start().await.init_basic().await;
    let board: Board = th
        .client
        .create_board(&new_board(TEST_TEAM_ID, BoardType::Private, "private"))
        .await
        .ok_json();

    // "creator should be able to access the private board and its blocks"
    let blocks = th.client.get(&format!("/boards/{}/blocks", board.id)).await;
    assert_eq!(blocks.ok_json::<Vec<Block>>(), vec![], "no blocks yet");

    // "unauthorized user should not be able to access the private board or its blocks":
    // for them it does not exist.
    th.client2
        .get(&format!("/boards/{}/blocks", board.id))
        .await
        .check_error(StatusCode::NOT_FOUND, "board not found");

    th.teardown().await;
}

async fn assert_no_boards(th: &TestServer, user_id: &str) {
    let boards = th
        .app
        .boards_for_user_and_team(user_id, TEST_TEAM_ID, PublicBoards::Include)
        .await
        .expect("the boards are read");
    assert!(boards.is_empty(), "no board was created: {boards:?}");
}

/// `TestCreateBoard/"create invalid board"/"invalid board type"`
#[tokio::test]
async fn create_board_create_invalid_board_invalid_board_type() {
    let th = Setup::native().start().await.init_basic().await;
    let user1 = th.client.me().await;

    let response = th
        .client
        .post(
            "/boards",
            &json!({"title": "invalid board title", "teamId": TEST_TEAM_ID, "type": "invalid"}),
        )
        .await;

    response.check_bad_request();
    assert_eq!(
        response.error_message(),
        "invalid-board-type",
        "the original's message"
    );
    assert_no_boards(&th, &user1.id).await;
    th.teardown().await;
}

/// `TestCreateBoard/"create invalid board"/"no type"`. The original's client sends an
/// empty string; a client that leaves the member out gets the same answer.
#[tokio::test]
async fn create_board_create_invalid_board_no_type() {
    let th = Setup::native().start().await.init_basic().await;
    let user1 = th.client.me().await;

    for body in [
        json!({"title": "invalid board title", "teamId": TEST_TEAM_ID, "type": ""}),
        json!({"title": "invalid board title", "teamId": TEST_TEAM_ID}),
        json!({"title": "invalid board title", "teamId": TEST_TEAM_ID, "type": null}),
    ] {
        let response = th.client.post("/boards", &body).await;
        response.check_bad_request();
        assert_eq!(response.error_message(), "invalid-board-type", "{body}");
    }

    assert_no_boards(&th, &user1.id).await;
    th.teardown().await;
}

/// `TestCreateBoard/"create invalid board"/"no team ID"`: the request is forbidden
/// because the permission check fails on an empty team, before the type is looked at.
#[tokio::test]
async fn create_board_create_invalid_board_no_team_id() {
    let th = Setup::native().start().await.init_basic().await;
    let user1 = th.client.me().await;

    let response = th
        .client
        .post(
            "/boards",
            &json!({"title": "invalid board title", "teamId": "", "type": ""}),
        )
        .await;
    response.check_forbidden();
    assert_eq!(
        response.error_message(),
        "access denied to create private boards",
        "anything but an open board counts as private"
    );

    // Verified on the original: with a valid type the message names the kind of board.
    let response = th
        .client
        .post("/boards", &json!({"title": "x", "type": "O"}))
        .await;
    response.check_forbidden();
    assert_eq!(
        response.error_message(),
        "access denied to create public boards",
        "an open board without a team"
    );

    assert_no_boards(&th, &user1.id).await;
    th.teardown().await;
}

/// Verified on the original: what `POST /boards` answers to bodies its tests do not
/// cover.
#[tokio::test]
async fn create_board_validates_the_body_like_the_original() {
    let th = Setup::native().start().await.init_basic().await;
    let user1 = th.client.me().await;

    // Not JSON, or JSON of the wrong shape: a bad request.
    for body in [
        "{\"teamId\":",
        "",
        "{\"teamId\":\"t\",\"type\":5}",
        "[]",
        "5",
        // (`null` crashes the original's handler, which its recovery turns into a 500.)
        "null",
    ] {
        let response = th.client.send_raw_body(Method::POST, "/boards", body).await;
        response.check_bad_request();
        assert_eq!(response.error_code(), "bad_request", "{body}");
    }

    let response = th
        .client
        .post(
            "/boards",
            &json!({"teamId": TEST_TEAM_ID, "type": "O", "minimumRole": "boss"}),
        )
        .await;
    response.check_bad_request();
    assert_eq!(
        response.error_message(),
        "invalid-board-minimum-role",
        "the original's message"
    );

    // The type is checked before the minimum role.
    let response = th
        .client
        .post(
            "/boards",
            &json!({"teamId": TEST_TEAM_ID, "type": "X", "minimumRole": "boss"}),
        )
        .await;
    response.check_bad_request();
    assert_eq!(response.error_message(), "invalid-board-type", "type first");

    // A new board does not choose its identifier. (The original answers 500.)
    let response = th
        .client
        .post(
            "/boards",
            &json!({"id": "my-id", "teamId": TEST_TEAM_ID, "type": "O"}),
        )
        .await;
    response.check_error(StatusCode::BAD_REQUEST, "new board cannot have an ID");
    assert_eq!(
        response.value()["field"],
        json!("id"),
        "the member at fault"
    );

    assert_no_boards(&th, &user1.id).await;
    th.teardown().await;
}

/// The wire format of a board, compared with what the original answers. The original
/// sends `null` for `properties` and `cardProperties` that were never set; the port sends
/// empty values (a deliberate difference of the model).
#[tokio::test]
async fn create_board_wire_format() {
    let th = Setup::native().start().await.init_basic().await;
    let me = th.client.me().await;

    let response = th
        .client
        .post(
            "/boards",
            &json!({"teamId": TEST_TEAM_ID, "type": "O", "title": "open one"}),
        )
        .await;
    let value = response.value();
    response.check_ok();

    assert_eq!(
        value,
        json!({
            "id": value["id"],
            "teamId": TEST_TEAM_ID,
            "channelId": "",
            "createdBy": me.id,
            "modifiedBy": me.id,
            "type": "O",
            "minimumRole": "",
            "title": "open one",
            "description": "",
            "icon": "",
            "showDescription": false,
            "isTemplate": false,
            "templateVersion": 0,
            "properties": {},
            "cardProperties": [],
            "createAt": value["createAt"],
            "updateAt": value["createAt"],
            "deleteAt": 0,
        }),
        "the members of a board on the wire"
    );
    assert!(value["createAt"].as_i64() > Some(0), "stamped: {value}");
    th.teardown().await;
}

/// Board creation files the board under the creator's default category; a template is
/// filed nowhere (`server/app/boards.go:269-273`).
#[tokio::test]
async fn create_board_adds_the_board_to_the_default_category() {
    let th = Setup::native().start().await.init_basic().await;
    let me = th.client.me().await;

    let board: Board = th
        .client
        .create_board(&new_board(TEST_TEAM_ID, BoardType::Open, "filed"))
        .await
        .ok_json();
    let template: Board = th
        .client
        .create_board(&Board {
            is_template: true,
            ..new_board(TEST_TEAM_ID, BoardType::Open, "a template")
        })
        .await
        .ok_json();

    assert_eq!(
        category_name_of(&th, &me.id, TEST_TEAM_ID, &board.id).await,
        Some("Boards".to_owned()),
        "the board is in the default category"
    );
    assert_eq!(
        category_name_of(&th, &me.id, TEST_TEAM_ID, &template.id).await,
        None,
        "the template is in no category"
    );
    th.teardown().await;
}

// -- TestCreateBoardTemplate ----------------------------------------------------------

/// `TestCreateBoardTemplate/"create public board template"`, with its subtests about the
/// board.
#[tokio::test]
async fn create_board_template_create_public_board_template() {
    let th = Setup::native().start().await.init_basic().await;
    let me = th.client.me().await;
    let title = "board template 1";

    let board: Board = th
        .client
        .create_board(&Board {
            is_template: true,
            ..new_board(TEST_TEAM_ID, BoardType::Open, title)
        })
        .await
        .ok_json();
    assert_eq!(board.id.len(), 27, "the board got an identifier");
    assert_eq!(board.title, title, "the title is kept");
    assert_eq!(board.board_type, BoardType::Open, "the type is kept");
    assert_eq!(board.team_id, TEST_TEAM_ID, "the team is kept");
    assert_eq!(board.created_by, me.id, "created by the session's user");
    assert_eq!(board.modified_by, me.id, "modified by the session's user");
    assert!(board.is_template, "a template");

    // "creating a board template should make the creator an admin"
    let members = members_of(&th, &board.id).await;
    assert_eq!(members.len(), 1, "one member: {members:?}");
    assert_eq!(members[0].user_id, me.id, "the creator is the member");
    assert_eq!(members[0].board_id, board.id, "of this board");
    assert!(members[0].scheme_admin, "as an admin");

    // "creator should be able to access the public board template and its blocks"
    let read: Board = get_board(&th.client, &board.id).await.ok_json();
    assert_eq!(read, board, "the creator reads the template back");

    // "another user should be able to access the public board template and its blocks"
    let read: Board = get_board(&th.client2, &board.id).await.ok_json();
    assert_eq!(read, board, "another user reads an open template");

    th.teardown().await;
}

/// The assertions of `TestCreateBoardTemplate/"create public board template"` about the
/// template's blocks.
#[tokio::test]
async fn create_board_template_create_public_board_template_and_its_blocks() {
    let th = Setup::native().start().await.init_basic().await;
    let board: Board = th
        .client
        .create_board(&Board {
            is_template: true,
            ..new_board(TEST_TEAM_ID, BoardType::Open, "board template 1")
        })
        .await
        .ok_json();

    // "creator should be able to access the public board template and its blocks"
    th.client
        .get(&format!("/boards/{}/blocks", board.id))
        .await
        .check_ok();

    // "another user should be able to access the public board template and its blocks"
    th.client2
        .get(&format!("/boards/{}/blocks", board.id))
        .await
        .check_ok();

    th.teardown().await;
}

/// `TestCreateBoardTemplate/"create private board template"`, with its subtests about the
/// board.
#[tokio::test]
async fn create_board_template_create_private_board_template() {
    let th = Setup::native().start().await.init_basic().await;
    let me = th.client.me().await;
    let title = "private board template title";

    let board: Board = th
        .client
        .create_board(&Board {
            is_template: true,
            ..new_board(TEST_TEAM_ID, BoardType::Private, title)
        })
        .await
        .ok_json();
    assert_eq!(board.id.len(), 27, "the board got an identifier");
    assert_eq!(board.title, title, "the title is kept");
    assert_eq!(board.board_type, BoardType::Private, "the type is kept");
    assert_eq!(board.team_id, TEST_TEAM_ID, "the team is kept");
    assert_eq!(board.created_by, me.id, "created by the session's user");
    assert_eq!(board.modified_by, me.id, "modified by the session's user");

    // "creating a board template should make the creator an admin"
    let members = members_of(&th, &board.id).await;
    assert_eq!(members.len(), 1, "one member: {members:?}");
    assert_eq!(members[0].user_id, me.id, "the creator is the member");
    assert_eq!(members[0].board_id, board.id, "of this board");
    assert!(members[0].scheme_admin, "as an admin");

    // "creator should be able to access the private board template and its blocks"
    let read: Board = get_board(&th.client, &board.id).await.ok_json();
    assert_eq!(read, board, "the creator reads the template back");

    // "unauthorized user should not be able to access the private board template or its
    // blocks": for them it does not exist.
    get_board(&th.client2, &board.id)
        .await
        .check_error(StatusCode::NOT_FOUND, "board not found");

    th.teardown().await;
}

/// The assertions of `TestCreateBoardTemplate/"create private board template"` about the
/// template's blocks.
#[tokio::test]
async fn create_board_template_create_private_board_template_and_its_blocks() {
    let th = Setup::native().start().await.init_basic().await;
    let board: Board = th
        .client
        .create_board(&Board {
            is_template: true,
            ..new_board(TEST_TEAM_ID, BoardType::Private, "private template")
        })
        .await
        .ok_json();

    // "creator should be able to access the private board template and its blocks"
    th.client
        .get(&format!("/boards/{}/blocks", board.id))
        .await
        .check_ok();

    // "unauthorized user should not be able to access the private board template or its
    // blocks": for them it does not exist.
    th.client2
        .get(&format!("/boards/{}/blocks", board.id))
        .await
        .check_error(StatusCode::NOT_FOUND, "board not found");

    th.teardown().await;
}

// -- TestGetBoard ---------------------------------------------------------------------

/// `TestGetBoard/"a non authenticated user should be rejected"`
#[tokio::test]
async fn get_board_a_non_authenticated_user_should_be_rejected() {
    let th = Setup::native().start().await.init_basic().await;

    let response = get_board(&th.anonymous(), "boar-id").await;

    response.check_unauthorized();
    assert_eq!(
        response.error_message(),
        "access denied to board",
        "the handler's own refusal, not the session check's"
    );
    th.teardown().await;
}

/// `TestGetBoard/"valid read token should be enough to get the board"`
#[tokio::test]
async fn get_board_valid_read_token_should_be_enough_to_get_the_board() {
    let th = Setup::local_mode().start().await.init_basic().await;
    let user1 = th.client.me().await;
    let sharing_token = new_id(IdKind::Token);
    let board = board_with_admin(
        &th,
        new_board(
            TEST_TEAM_ID,
            BoardType::Open,
            "public board where user1 is admin",
        ),
        &user1.id,
    )
    .await;
    share_board(&th, &board.id, &sharing_token).await;

    // Without a session the board cannot be read...
    let anonymous = th.anonymous();
    get_board(&anonymous, &board.id).await.check_unauthorized();
    anonymous
        .get(&format!("/boards/{}?read_token=wrong", board.id))
        .await
        .check_unauthorized();

    // ...but it can with the read token.
    let read: Board = anonymous
        .get(&format!("/boards/{}?read_token={sharing_token}", board.id))
        .await
        .ok_json();
    assert_eq!(read, board, "the shared board");

    th.teardown().await;
}

/// `TestGetBoard/"nonexisting board"`
#[tokio::test]
async fn get_board_nonexisting_board() {
    let th = Setup::native().start().await.init_basic().await;

    let response = get_board(&th.client, "nonexistent board").await;

    response.check_error(StatusCode::NOT_FOUND, "board not found");
    th.teardown().await;
}

/// `TestGetBoard/"a user that doesn't have permissions to a private board cannot
/// retrieve it"`
#[tokio::test]
async fn get_board_a_user_without_permissions_to_a_private_board_cannot_retrieve_it() {
    let th = Setup::native().start().await.init_basic().await;
    let user1 = th.client.me().await;
    let board = board_without_members(
        &th,
        new_board(TEST_TEAM_ID, BoardType::Private, ""),
        &user1.id,
    )
    .await;

    let response = get_board(&th.client, &board.id).await;

    // The answer is the one for a board that does not exist. (The original answers 403.)
    response.check_error(StatusCode::NOT_FOUND, "board not found");
    let missing = get_board(&th.client, "nonexistent board").await;
    assert_eq!(response.status, missing.status, "the same status");
    assert_eq!(
        response.error_message(),
        missing.error_message(),
        "and the same message"
    );
    th.teardown().await;
}

/// `TestGetBoard/"a user that has permissions to a private board can retrieve it"`
#[tokio::test]
async fn get_board_a_user_with_permissions_to_a_private_board_can_retrieve_it() {
    let th = Setup::native().start().await.init_basic().await;
    let user1 = th.client.me().await;
    let board = board_with_admin(
        &th,
        new_board(TEST_TEAM_ID, BoardType::Private, ""),
        &user1.id,
    )
    .await;

    let read: Board = get_board(&th.client, &board.id).await.ok_json();

    assert_eq!(read, board, "the member reads the board");
    th.teardown().await;
}

/// `TestGetBoard/"a user that doesn't have permissions to a public board but have them
/// to its team can retrieve it"`
#[tokio::test]
async fn get_board_a_user_with_permissions_to_the_team_can_retrieve_a_public_board() {
    let th = Setup::native().start().await.init_basic().await;
    let user1 = th.client.me().await;
    let board = board_without_members(
        &th,
        new_board(TEST_TEAM_ID, BoardType::Open, "title"),
        &user1.id,
    )
    .await;

    let read: Board = get_board(&th.client, &board.id).await.ok_json();

    assert_eq!(read, board, "a user of the team reads an open board");
    th.teardown().await;
}

/// In single-user mode the original hands every request to this route the single-user
/// session, whatever token it carries, so anyone who reaches the port reads any board.
/// The port gives a session only for the right token; without it the request ends at the
/// original's refusal for "no session, no valid read token".
#[tokio::test]
async fn get_board_in_single_user_mode_needs_the_token() {
    let th = Setup::single_user().start().await;
    let board = th.create_board(TEST_TEAM_ID, BoardType::Private).await;

    // The right token reads the board.
    let read: Board = get_board(&th.client, &board.id).await.ok_json();
    assert_eq!(read, board, "the single user reads their board");

    // No token and a wrong token are refused.
    for client in [th.anonymous(), th.client_with_token("WRONG")] {
        let response = get_board(&client, &board.id).await;
        response.check_unauthorized();
        assert_eq!(
            response.error_message(),
            "access denied to board",
            "the refusal of a request without session and read token"
        );
    }
    assert_ne!(SINGLE_USER_TOKEN, "WRONG", "the wrong token is wrong");

    th.teardown().await;
}

// -- TestGetBoardMetadata -------------------------------------------------------------
//
// The original's metadata route needs a license a standalone server never has, and
// answered 501. The route is gone; `admin::routes_of_the_host_application_do_not_exist`
// checks that its path answers like any other that does not exist.

// -- TestPatchBoard -------------------------------------------------------------------

fn title_patch(title: &str) -> BoardPatch {
    BoardPatch {
        title: Some(title.to_owned()),
        ..BoardPatch::default()
    }
}

async fn patch_board(
    client: &Client,
    board_id: &str,
    patch: &impl serde::Serialize,
) -> ApiResponse {
    client.patch(&format!("/boards/{board_id}"), patch).await
}

async fn stored_board(th: &TestServer, board_id: &str) -> Board {
    th.app.board(board_id).await.expect("the board exists")
}

/// `TestPatchBoard/"a non authenticated user should be rejected"`
#[tokio::test]
async fn patch_board_a_non_authenticated_user_should_be_rejected() {
    let th = Setup::native().start().await.init_basic().await;
    let initial_title = "title 1";
    let board = board_without_members(
        &th,
        new_board(TEST_TEAM_ID, BoardType::Open, initial_title),
        "user-id",
    )
    .await;

    let response = patch_board(&th.anonymous(), &board.id, &title_patch("a new title 1")).await;

    response.check_unauthorized();
    assert_eq!(
        stored_board(&th, &board.id).await.title,
        initial_title,
        "the board is unchanged"
    );
    th.teardown().await;
}

/// `TestPatchBoard/"non existing board"`
#[tokio::test]
async fn patch_board_non_existing_board() {
    let th = Setup::native().start().await.init_basic().await;

    let response = patch_board(
        &th.client,
        "non-existing-board",
        &title_patch("a new title 2"),
    )
    .await;

    response.check_not_found();
    th.teardown().await;
}

/// `TestPatchBoard/"invalid patch on a board with permissions"`
#[tokio::test]
async fn patch_board_invalid_patch_on_a_board_with_permissions() {
    let th = Setup::native().start().await.init_basic().await;
    let user1 = th.client.me().await;
    let board = board_with_admin(
        &th,
        new_board(TEST_TEAM_ID, BoardType::Open, "title"),
        &user1.id,
    )
    .await;

    let response = patch_board(&th.client, &board.id, &json!({"type": "invalid"})).await;
    response.check_bad_request();
    assert_eq!(response.error_message(), "invalid-board-type", "the type");

    let response = patch_board(&th.client, &board.id, &json!({"minimumRole": "boss"})).await;
    response.check_bad_request();
    assert_eq!(
        response.error_message(),
        "invalid-board-minimum-role",
        "the minimum role"
    );

    // The permission is checked before the body is looked at. (The original checks the
    // validity first.)
    let response = patch_board(&th.client2, &board.id, &json!({"type": "invalid"})).await;
    response.check_error(
        StatusCode::FORBIDDEN,
        "access denied to modifying board properties",
    );

    assert_eq!(
        stored_board(&th, &board.id).await,
        board,
        "the board is unchanged"
    );
    th.teardown().await;
}

/// `TestPatchBoard/"valid patch on a board with permissions"`
#[tokio::test]
async fn patch_board_valid_patch_on_a_board_with_permissions() {
    let th = Setup::native().start().await.init_basic().await;
    let user1 = th.client.me().await;
    let board = board_with_admin(
        &th,
        new_board(TEST_TEAM_ID, BoardType::Open, "title"),
        &user1.id,
    )
    .await;
    let new_title = "a new title";

    let patched: Board = patch_board(&th.client, &board.id, &title_patch(new_title))
        .await
        .ok_json();

    assert_eq!(patched.title, new_title, "the new title is answered");
    assert_eq!(
        stored_board(&th, &board.id).await,
        patched,
        "and stored as answered"
    );
    th.teardown().await;
}

/// `TestPatchBoard/"valid patch on a board without permissions"`
#[tokio::test]
async fn patch_board_valid_patch_on_a_board_without_permissions() {
    let th = Setup::native().start().await.init_basic().await;
    let user1 = th.client.me().await;
    let initial_title = "title";
    let board = board_without_members(
        &th,
        new_board(TEST_TEAM_ID, BoardType::Open, initial_title),
        &user1.id,
    )
    .await;

    let response = patch_board(&th.client, &board.id, &title_patch("a new title")).await;

    response.check_forbidden();
    assert_eq!(
        response.error_message(),
        "access denied to modifying board properties",
        "why"
    );
    assert_eq!(
        stored_board(&th, &board.id).await.title,
        initial_title,
        "the board is unchanged"
    );
    th.teardown().await;
}

/// Verified on the original: an editor changes a board's title, but only an admin its
/// type, minimum role and channel; each refusal has its own message.
#[tokio::test]
async fn patch_board_permissions_by_what_the_patch_touches() {
    let th = Setup::native().start().await.init_basic().await;
    let user1 = th.client.me().await;
    let user2 = th.client2.me().await;
    let board = board_with_admin(
        &th,
        new_board(TEST_TEAM_ID, BoardType::Open, "title"),
        &user1.id,
    )
    .await;
    add_member(&th, &editor(&board.id, &user2.id)).await;

    let patched: Board = patch_board(&th.client2, &board.id, &title_patch("by the editor"))
        .await
        .ok_json();
    assert_eq!(
        patched.title, "by the editor",
        "an editor changes the title"
    );
    assert_eq!(patched.modified_by, user2.id, "and is recorded as modifier");

    for (patch, message) in [
        (
            json!({"type": "P"}),
            "access denied to modifying board type",
        ),
        (
            json!({"minimumRole": "viewer"}),
            "access denied to modifying board type",
        ),
        (
            json!({"channelId": "channel"}),
            "access denied to modifying board access",
        ),
    ] {
        let response = patch_board(&th.client2, &board.id, &patch).await;
        response.check_forbidden();
        assert_eq!(response.error_message(), message, "{patch}");
    }

    // The admin may do all of it.
    let patched: Board = patch_board(
        &th.client,
        &board.id,
        &json!({"type": "P", "minimumRole": "viewer", "channelId": "channel"}),
    )
    .await
    .ok_json();
    assert_eq!(patched.board_type, BoardType::Private, "the type");
    assert_eq!(patched.minimum_role, BoardRole::Viewer, "the minimum role");
    assert_eq!(patched.channel_id, "channel", "the channel");

    // The board's minimum role shows in its memberships.
    let members: Vec<BoardMember> = get_members(&th.client, &board.id).await.ok_json();
    assert!(
        members.iter().all(|member| member.minimum_role == "viewer"),
        "{members:?}"
    );

    // Malformed bodies are bad requests, `null` too (which crashes the original's
    // handler).
    for body in ["{bad", "", "[]", "null", "{\"type\":5}"] {
        th.client
            .send_raw_body(Method::PATCH, &format!("/boards/{}", board.id), body)
            .await
            .check_bad_request();
    }

    th.teardown().await;
}

// -- TestDeleteBoard ------------------------------------------------------------------

/// `TestDeleteBoard/"a non authenticated user should be rejected"`
#[tokio::test]
async fn delete_board_a_non_authenticated_user_should_be_rejected() {
    let th = Setup::native().start().await.init_basic().await;
    let board = board_without_members(
        &th,
        new_board(TEST_TEAM_ID, BoardType::Open, "title"),
        "user-id",
    )
    .await;

    let response = th
        .anonymous()
        .delete(&format!("/boards/{}", board.id))
        .await;

    response.check_unauthorized();
    assert_eq!(stored_board(&th, &board.id).await, board, "still there");
    th.teardown().await;
}

/// `TestDeleteBoard/"a user without permissions should be rejected"`
#[tokio::test]
async fn delete_board_a_user_without_permissions_should_be_rejected() {
    let th = Setup::native().start().await.init_basic().await;
    let board = board_without_members(
        &th,
        new_board(TEST_TEAM_ID, BoardType::Open, "title"),
        "some-user-id",
    )
    .await;

    let response = th.client.delete(&format!("/boards/{}", board.id)).await;

    response.check_forbidden();
    assert_eq!(
        response.error_message(),
        "access denied to delete board",
        "why"
    );
    assert_eq!(stored_board(&th, &board.id).await, board, "still there");
    th.teardown().await;
}

/// `TestDeleteBoard/"non existing board"`
#[tokio::test]
async fn delete_board_non_existing_board() {
    let th = Setup::native().start().await.init_basic().await;

    let response = th.client.delete("/boards/non-existing-board").await;

    response.check_not_found();
    th.teardown().await;
}

/// `TestDeleteBoard/"an existing board should be correctly deleted"`
#[tokio::test]
async fn delete_board_an_existing_board_should_be_correctly_deleted() {
    let th = Setup::native().start().await.init_basic().await;
    let user1 = th.client.me().await;
    let board = board_with_admin(
        &th,
        new_board(TEST_TEAM_ID, BoardType::Open, "title"),
        &user1.id,
    )
    .await;

    let response = th.client.delete(&format!("/boards/{}", board.id)).await;

    response.check_ok();
    assert_eq!(response.text(), "{}", "an empty object");
    let error = th
        .app
        .board(&board.id)
        .await
        .expect_err("the board is gone");
    assert!(error.is_not_found(), "not found: {error:?}");

    // Verified on the original: deleting it again is a 404.
    th.client
        .delete(&format!("/boards/{}", board.id))
        .await
        .check_not_found();
    th.teardown().await;
}

// -- TestUndeleteBoard ----------------------------------------------------------------

async fn undelete_board(client: &Client, board_id: &str) -> ApiResponse {
    client
        .post_empty(&format!("/boards/{board_id}/undelete"))
        .await
}

async fn assert_board_is_gone(th: &TestServer, board_id: &str) {
    let error = th
        .app
        .board(board_id)
        .await
        .expect_err("the board stays deleted");
    assert!(error.is_not_found(), "not found: {error:?}");
}

/// `TestUndeleteBoard/"a non authenticated user should be rejected"`
#[tokio::test]
async fn undelete_board_a_non_authenticated_user_should_be_rejected() {
    let th = Setup::native().start().await.init_basic().await;
    let board = board_without_members(
        &th,
        new_board(TEST_TEAM_ID, BoardType::Open, "title"),
        "user-id",
    )
    .await;
    th.app
        .delete_board(&board.id, "user-id")
        .await
        .expect("the board is deleted");

    let response = undelete_board(&th.anonymous(), &board.id).await;

    response.check_unauthorized();
    assert_board_is_gone(&th, &board.id).await;
    th.teardown().await;
}

/// `TestUndeleteBoard/"a user without membership should be rejected"`
#[tokio::test]
async fn undelete_board_a_user_without_membership_should_be_rejected() {
    let th = Setup::native().start().await.init_basic().await;
    let board = board_without_members(
        &th,
        new_board(TEST_TEAM_ID, BoardType::Open, "title"),
        "some-user-id",
    )
    .await;
    th.app
        .delete_board(&board.id, "some-user-id")
        .await
        .expect("the board is deleted");

    let response = undelete_board(&th.client, &board.id).await;

    // A deleted board the user was no member of answers as one that never existed.
    response.check_error(StatusCode::NOT_FOUND, "board not found");
    assert_board_is_gone(&th, &board.id).await;
    th.teardown().await;
}

/// `TestUndeleteBoard/"a user with membership but without permissions should be
/// rejected"`. The original gives the membership to a user who is not the one asking;
/// here the asking user is an editor as well, which is the case the name describes.
#[tokio::test]
async fn undelete_board_a_user_with_membership_but_without_permissions_should_be_rejected() {
    let th = Setup::native().start().await.init_basic().await;
    let user1 = th.client.me().await;
    let board = board_without_members(
        &th,
        new_board(TEST_TEAM_ID, BoardType::Open, "title"),
        "some-user-id",
    )
    .await;
    add_member(&th, &editor(&board.id, "user-id")).await;
    add_member(&th, &editor(&board.id, &user1.id)).await;
    th.app
        .delete_board(&board.id, "some-user-id")
        .await
        .expect("the board is deleted");

    let response = undelete_board(&th.client, &board.id).await;

    response.check_forbidden();
    assert_board_is_gone(&th, &board.id).await;
    th.teardown().await;
}

/// `TestUndeleteBoard/"non existing board"`
#[tokio::test]
async fn undelete_board_non_existing_board() {
    let th = Setup::native().start().await.init_basic().await;

    let response = undelete_board(&th.client, "non-existing-board").await;

    response.check_error(StatusCode::NOT_FOUND, "board not found");
    th.teardown().await;
}

/// `TestUndeleteBoard/"an existing deleted board should be correctly undeleted"`
#[tokio::test]
async fn undelete_board_an_existing_deleted_board_should_be_correctly_undeleted() {
    let th = Setup::native().start().await.init_basic().await;
    let user1 = th.client.me().await;
    let board = board_with_admin(
        &th,
        new_board(TEST_TEAM_ID, BoardType::Open, "title"),
        &user1.id,
    )
    .await;

    // Verified on the original: undeleting a board that is not deleted succeeds.
    undelete_board(&th.client, &board.id).await.check_ok();

    th.app
        .delete_board(&board.id, "user-id")
        .await
        .expect("the board is deleted");
    assert_board_is_gone(&th, &board.id).await;

    let response = undelete_board(&th.client, &board.id).await;

    response.check_ok();
    assert_eq!(response.text(), "{}", "an empty object");
    let restored = stored_board(&th, &board.id).await;
    assert_eq!(restored.title, board.title, "the board is back");
    assert_eq!(restored.delete_at, 0, "and not deleted");
    th.teardown().await;
}

// -- TestGetMembersForBoard -----------------------------------------------------------

/// `createBoardWithUsers`: an open board with user1 as admin and user2 as editor.
async fn board_with_users(th: &TestServer) -> Board {
    let user1 = th.client.me().await;
    let user2 = th.client2.me().await;
    let board = board_with_admin(
        th,
        new_board(TEST_TEAM_ID, BoardType::Open, "title"),
        &user1.id,
    )
    .await;
    add_member(th, &editor(&board.id, &user2.id)).await;
    board
}

/// `TestGetMembersForBoard/"a non authenticated user should be rejected"`
#[tokio::test]
async fn get_members_for_board_a_non_authenticated_user_should_be_rejected() {
    let th = Setup::native().start().await.init_basic().await;
    let board = board_with_users(&th).await;

    let response = get_members(&th.anonymous(), &board.id).await;

    response.check_unauthorized();
    th.teardown().await;
}

/// `TestGetMembersForBoard/"a user without permissions should be rejected"`
#[tokio::test]
async fn get_members_for_board_a_user_without_permissions_should_be_rejected() {
    let th = Setup::native().start().await.init_basic().await;
    let user2 = th.client2.me().await;
    let board = board_with_users(&th).await;
    th.app
        .delete_board_member(&board.id, &user2.id)
        .await
        .expect("user2 is removed");

    let response = get_members(&th.client2, &board.id).await;

    response.check_forbidden();
    assert_eq!(
        response.error_message(),
        "access denied to board members",
        "why"
    );
    th.teardown().await;
}

/// `TestGetMembersForBoard/"non existing board"`
#[tokio::test]
async fn get_members_for_board_non_existing_board() {
    let th = Setup::native().start().await.init_basic().await;

    let response = get_members(&th.client, "non-existing-board").await;

    response.check_error(StatusCode::NOT_FOUND, "board not found");
    th.teardown().await;
}

/// `TestGetMembersForBoard/"should correctly return board members for a valid board"`,
/// and the "-base case" and "-team check false" cases of `TestGetMembersForBoard` in
/// `server/app/boards_test.go`: members are answered as stored, nobody is raised to
/// admin.
#[tokio::test]
async fn get_members_for_board_should_correctly_return_board_members_for_a_valid_board() {
    let th = Setup::native().start().await.init_basic().await;
    let user1 = th.client.me().await;
    let user2 = th.client2.me().await;
    let board = board_with_users(&th).await;

    let response = get_members(&th.client, &board.id).await;
    let mut members: Vec<BoardMember> = response.ok_json();
    members.sort_by_key(|member| member.user_id != user1.id);

    assert_eq!(
        members,
        vec![
            BoardMember {
                scheme_admin: true,
                scheme_editor: true,
                ..member(&board.id, &user1.id)
            },
            editor(&board.id, &user2.id),
        ],
        "the admin and the editor"
    );
    assert_eq!(
        members_of(&th, &board.id).await.len(),
        2,
        "the application layer answers the same members"
    );

    // The wire format of a membership.
    let first = response
        .value()
        .as_array()
        .and_then(|members| {
            members
                .iter()
                .find(|member| member["userId"] == json!(user2.id))
                .cloned()
        })
        .expect("user2 is listed");
    assert_eq!(
        first,
        json!({
            "boardId": board.id,
            "userId": user2.id,
            "roles": "",
            "minimumRole": "",
            "schemeAdmin": false,
            "schemeEditor": true,
            "schemeCommenter": false,
            "schemeViewer": false,
            "synthetic": false,
        }),
        "the members of a membership on the wire"
    );
    th.teardown().await;
}

/// The "-base case" and "-team check false" cases of `TestGetMembersForUser` in
/// `server/app/boards_test.go`, through `GET /users/me/memberships`: memberships are
/// answered as stored, also of a board that was deleted.
#[tokio::test]
async fn get_members_for_user_returns_the_memberships_as_stored() {
    let th = Setup::native().start().await.init_basic().await;
    let user2 = th.client2.me().await;
    let board = board_with_users(&th).await;

    let memberships: Vec<BoardMember> = th.client2.get("/users/me/memberships").await.ok_json();
    assert_eq!(
        memberships,
        vec![editor(&board.id, &user2.id)],
        "an editor, not an admin"
    );

    th.app
        .delete_board(&board.id, &user2.id)
        .await
        .expect("the board is deleted");
    let memberships: Vec<BoardMember> = th.client2.get("/users/me/memberships").await.ok_json();
    assert_eq!(memberships.len(), 1, "the membership outlives the board");
    assert!(!memberships[0].scheme_admin, "still not an admin");

    th.teardown().await;
}

// -- TestAddMember --------------------------------------------------------------------

/// `TestAddMember/"a non authenticated user should be rejected"`
#[tokio::test]
async fn add_member_a_non_authenticated_user_should_be_rejected() {
    let th = Setup::native().start().await.init_basic().await;
    let board = board_without_members(
        &th,
        new_board(TEST_TEAM_ID, BoardType::Open, "title"),
        "user-id",
    )
    .await;

    let response = th.anonymous().add_member(&editor(&board.id, "user1")).await;

    response.check_unauthorized();
    th.teardown().await;
}

/// `TestAddMember/"a user without permissions should be rejected"`
#[tokio::test]
async fn add_member_a_user_without_permissions_should_be_rejected() {
    let th = Setup::native().start().await.init_basic().await;
    let board = board_without_members(
        &th,
        new_board(TEST_TEAM_ID, BoardType::Private, "title"),
        "user-id",
    )
    .await;

    let response = th.client.add_member(&editor(&board.id, "user1")).await;

    // A private board the user is no member of does not exist for them.
    response.check_error(StatusCode::NOT_FOUND, "board not found");
    th.teardown().await;
}

/// `TestAddMember/"non existing board"`
#[tokio::test]
async fn add_member_non_existing_board() {
    let th = Setup::native().start().await.init_basic().await;

    let response = th
        .client
        .add_member(&editor("non-existing-board-id", "user1"))
        .await;

    response.check_not_found();
    th.teardown().await;
}

/// `TestAddMember/"should correctly add a new member for a valid board"/"a private board
/// through an admin user"`
#[tokio::test]
async fn add_member_a_private_board_through_an_admin_user() {
    let th = Setup::native().start().await.init_basic().await;
    let user1 = th.client.me().await;
    let user2 = th.client2.me().await;
    let board = board_with_admin(
        &th,
        new_board(TEST_TEAM_ID, BoardType::Private, "title"),
        &user1.id,
    )
    .await;
    let new_member = editor(&board.id, &user2.id);

    let added: BoardMember = th.client.add_member(&new_member).await.ok_json();

    assert_eq!(added, new_member, "the membership as asked for");
    assert_eq!(
        category_name_of(&th, &user2.id, TEST_TEAM_ID, &board.id).await,
        Some("Boards".to_owned()),
        "the board is filed under the new member's default category"
    );
    th.teardown().await;
}

/// `TestAddMember/"should correctly add a new member for a valid board"/"a public board
/// through a user that is not yet a member"`
#[tokio::test]
async fn add_member_a_public_board_through_a_user_that_is_not_yet_a_member() {
    let th = Setup::native().start().await.init_basic().await;
    let user1 = th.client.me().await;
    let user2 = th.client2.me().await;
    let board = board_with_admin(
        &th,
        new_board(TEST_TEAM_ID, BoardType::Open, "title"),
        &user1.id,
    )
    .await;
    let new_member = editor(&board.id, &user2.id);

    th.client2.add_member(&new_member).await.check_forbidden();
    get_members(&th.client2, &board.id).await.check_forbidden();

    // Join board - will become an editor
    let joined: BoardMember = join_board(&th.client2, &board.id).await.ok_json();
    assert_eq!(joined, new_member, "joined as an editor");

    let added: BoardMember = th.client2.add_member(&new_member).await.ok_json();
    assert_eq!(added, new_member, "the existing membership is answered");

    let members: Vec<BoardMember> = get_members(&th.client2, &board.id).await.ok_json();
    assert_eq!(members.len(), 2, "two members: {members:?}");
    th.teardown().await;
}

/// `TestAddMember/"should correctly add a new member for a valid board"/"should always
/// add a new member as given board role"`
#[tokio::test]
async fn add_member_should_always_add_a_new_member_as_given_board_role() {
    let th = Setup::native().start().await.init_basic().await;
    let user1 = th.client.me().await;
    let user2 = th.client2.me().await;
    let board = board_with_admin(
        &th,
        new_board(TEST_TEAM_ID, BoardType::Private, "title"),
        &user1.id,
    )
    .await;
    let new_member = BoardMember {
        scheme_commenter: true,
        ..member(&board.id, &user2.id)
    };

    let added: BoardMember = th.client.add_member(&new_member).await.ok_json();

    assert_eq!(added, new_member, "a commenter and nothing else");
    th.teardown().await;
}

/// `TestAddMember/"should do nothing if the member already exists"`
#[tokio::test]
async fn add_member_should_do_nothing_if_the_member_already_exists() {
    let th = Setup::native().start().await.init_basic().await;
    let user1 = th.client.me().await;
    let board = board_with_admin(
        &th,
        new_board(TEST_TEAM_ID, BoardType::Private, "title"),
        &user1.id,
    )
    .await;
    let members = members_of(&th, &board.id).await;
    assert_eq!(members.len(), 1, "one member: {members:?}");
    assert!(
        members[0].scheme_admin && members[0].scheme_editor,
        "an admin and editor"
    );

    let answered: BoardMember = th
        .client
        .add_member(&editor(&board.id, &user1.id))
        .await
        .ok_json();

    assert!(
        answered.scheme_admin && answered.scheme_editor,
        "the existing membership is answered: {answered:?}"
    );
    assert_eq!(
        members_of(&th, &board.id).await,
        members,
        "and nothing was changed"
    );
    th.teardown().await;
}

/// Verified on the original: how `POST /boards/{board_id}/members` treats the body.
#[tokio::test]
async fn add_member_validates_the_body_like_the_original() {
    let th = Setup::native().start().await.init_basic().await;
    let user1 = th.client.me().await;
    let user2 = th.client2.me().await;
    let board = board_with_admin(
        &th,
        new_board(TEST_TEAM_ID, BoardType::Open, "title"),
        &user1.id,
    )
    .await;
    let path = format!("/boards/{}/members", board.id);

    for body in ["{bad", "[]", "null"] {
        th.client
            .send_raw_body(Method::POST, &path, body)
            .await
            .check_bad_request();
    }
    let response = th.client.post(&path, &json!({"schemeEditor": true})).await;
    response.check_bad_request();
    assert_eq!(response.error_message(), "empty userID", "no user named");

    // Only the user and the four role flags are taken from the body; the board comes from
    // the path.
    let added: BoardMember = th
        .client
        .post(
            &path,
            &json!({
                "userId": user2.id,
                "boardId": "another-board",
                "schemeCommenter": true,
                "roles": "x",
                "minimumRole": "admin",
                "synthetic": true,
            }),
        )
        .await
        .ok_json();
    assert_eq!(
        added,
        BoardMember {
            scheme_commenter: true,
            ..member(&board.id, &user2.id)
        },
        "the rest of the body is ignored"
    );

    // The user must exist. (The original stores a membership of nobody.)
    let response = th
        .client
        .post(
            &path,
            &json!({"userId": "no-such-user", "schemeEditor": true}),
        )
        .await;
    response.check_error(StatusCode::BAD_REQUEST, "no such user");
    assert_eq!(
        response.value()["field"],
        json!("userId"),
        "the member at fault"
    );

    // A membership without any role is stored as such.
    let user3 = th.add_user("user3").await.me().await;
    let added: BoardMember = th
        .client
        .post(&path, &json!({"userId": user3.id}))
        .await
        .ok_json();
    assert_eq!(added, member(&board.id, &user3.id), "no role at all");

    th.teardown().await;
}

/// Not in the original, which lets any editor of an open board add members with any
/// role: joining an open board and adding a second account as its admin took the board
/// over (reproduced on the original). Granting the admin role now needs the permission
/// to manage the board's roles.
#[tokio::test]
async fn add_member_as_admin_needs_the_permission_to_manage_roles() {
    let th = Setup::native().start().await.init_basic().await;
    let user1 = th.client.me().await;
    let user2 = th.client2.me().await;
    let client3 = th.add_user("user3").await;
    let user3 = client3.me().await;
    let board = board_with_admin(
        &th,
        new_board(TEST_TEAM_ID, BoardType::Open, "victim"),
        &user1.id,
    )
    .await;

    // user2 joins the open board, which makes them an editor.
    join_board(&th.client2, &board.id).await.check_ok();

    // The attack: the editor adds an accomplice as admin.
    let response = th
        .client2
        .add_member(&BoardMember {
            scheme_admin: true,
            ..member(&board.id, &user3.id)
        })
        .await;
    response.check_forbidden();
    assert_eq!(
        response.error_message(),
        "access denied to modify board members",
        "the refusal the original uses for this route"
    );
    assert_eq!(
        members_of(&th, &board.id).await.len(),
        2,
        "the accomplice is not a member"
    );
    client3
        .delete(&format!("/boards/{}", board.id))
        .await
        .check_forbidden();
    delete_member(&client3, &member(&board.id, &user1.id))
        .await
        .check_forbidden();

    // Naming an existing member with the admin flag is refused as well, not answered
    // with that membership.
    th.client2
        .add_member(&BoardMember {
            scheme_admin: true,
            ..member(&board.id, &user2.id)
        })
        .await
        .check_forbidden();

    th.teardown().await;
}

/// The counterpart of [`add_member_as_admin_needs_the_permission_to_manage_roles`]: an
/// editor of an open board still adds members, and an admin still adds admins.
#[tokio::test]
async fn add_member_by_an_editor_of_an_open_board_and_as_admin_by_an_admin_still_work() {
    let th = Setup::native().start().await.init_basic().await;
    let user1 = th.client.me().await;
    let user3 = th.add_user("user3").await.me().await;
    let board = board_with_admin(
        &th,
        new_board(TEST_TEAM_ID, BoardType::Open, "open"),
        &user1.id,
    )
    .await;
    join_board(&th.client2, &board.id).await.check_ok();

    let response = th
        .client2
        .add_member(&BoardMember {
            scheme_viewer: true,
            ..member(&board.id, &user3.id)
        })
        .await;
    let added: BoardMember = response.ok_json();
    assert_eq!(added.user_id, user3.id, "added: {added:?}");

    // An admin still adds admins.
    let client4 = th.add_user("user4").await;
    let user4 = client4.me().await;
    let added: BoardMember = th
        .client
        .add_member(&BoardMember {
            scheme_admin: true,
            ..member(&board.id, &user4.id)
        })
        .await
        .ok_json();
    assert!(added.scheme_admin, "added as an admin: {added:?}");

    th.teardown().await;
}

/// The role flags of a member, in the order admin, editor, commenter, viewer.
fn role_flags(member: &BoardMember) -> [bool; 4] {
    [
        member.scheme_admin,
        member.scheme_editor,
        member.scheme_commenter,
        member.scheme_viewer,
    ]
}

const ONLY_EDITOR: [bool; 4] = [false, true, false, false];
const ONLY_COMMENTER: [bool; 4] = [false, false, true, false];
const ONLY_VIEWER: [bool; 4] = [false, false, false, true];
const NO_ROLE: [bool; 4] = [false; 4];

/// Not in the original, which stores whatever roles the request names: an editor of an
/// open board, who may add members but not manage roles, could make anyone an editor or
/// a member without any role. Such an adder now gives the board's minimum role, and
/// viewer if the board names none. The answer and the stored membership say what was
/// given.
#[tokio::test]
async fn add_member_without_managing_roles_gives_the_minimum_role_of_the_board() {
    let th = Setup::native().start().await.init_basic().await;
    let user1 = th.client.me().await;
    let editor_request = |board_id: &str, user_id: &str| BoardMember {
        scheme_editor: true,
        scheme_commenter: true,
        scheme_viewer: true,
        ..member(board_id, user_id)
    };

    let cases = [
        ("", ONLY_VIEWER),
        ("viewer", ONLY_VIEWER),
        ("commenter", ONLY_COMMENTER),
        ("editor", ONLY_EDITOR),
    ];
    for (index, (minimum_role, expected)) in cases.into_iter().enumerate() {
        let board = board_with_admin(
            &th,
            new_board(TEST_TEAM_ID, BoardType::Open, "open"),
            &user1.id,
        )
        .await;
        // The adder is an editor of the board whatever its minimum role.
        add_member(&th, &editor(&board.id, &th.client2.me().await.id)).await;
        if !minimum_role.is_empty() {
            th.client
                .patch(
                    &format!("/boards/{}", board.id),
                    &json!({"minimumRole": minimum_role}),
                )
                .await
                .check_ok();
        }
        let asked_for_editor = th.add_user(&format!("asked-editor-{index}")).await;
        let asked_for_nothing = th.add_user(&format!("asked-nothing-{index}")).await;

        for (client, request) in [
            (
                &asked_for_editor,
                editor_request(&board.id, &asked_for_editor.me().await.id),
            ),
            (
                &asked_for_nothing,
                member(&board.id, &asked_for_nothing.me().await.id),
            ),
        ] {
            let added: BoardMember = th.client2.add_member(&request).await.ok_json();
            assert_eq!(
                role_flags(&added),
                expected,
                "minimum role {minimum_role:?}, request {request:?}: the answer"
            );
            let stored = members_of(&th, &board.id)
                .await
                .into_iter()
                .find(|member| member.user_id == request.user_id)
                .expect("the membership is stored");
            assert_eq!(
                role_flags(&stored),
                expected,
                "minimum role {minimum_role:?}: the stored membership"
            );
            // The role is real: only an editor may insert a card.
            let insert = client
                .insert_blocks(&board.id, &[card_block(&board.id)])
                .await;
            if expected == ONLY_EDITOR {
                insert.check_ok();
            } else {
                insert.check_forbidden();
            }
        }
    }
    th.teardown().await;
}

/// A board's minimum role is every member's least role. If it is admin, every member
/// manages the roles, so there is nobody on that board who adds members without
/// managing roles: the request of any member is stored as it is.
#[tokio::test]
async fn add_member_on_a_board_whose_minimum_role_is_admin_is_by_a_manager_of_roles() {
    let th = Setup::native().start().await.init_basic().await;
    let user1 = th.client.me().await;
    let board = board_with_admin(
        &th,
        new_board(TEST_TEAM_ID, BoardType::Open, "open"),
        &user1.id,
    )
    .await;
    add_member(&th, &editor(&board.id, &th.client2.me().await.id)).await;
    th.client
        .patch(
            &format!("/boards/{}", board.id),
            &json!({"minimumRole": "admin"}),
        )
        .await
        .check_ok();

    let user3 = th.add_user("user3").await.me().await;
    let added: BoardMember = th
        .client2
        .add_member(&BoardMember {
            scheme_commenter: true,
            ..member(&board.id, &user3.id)
        })
        .await
        .ok_json();
    assert_eq!(
        role_flags(&added),
        ONLY_COMMENTER,
        "the role the request names, from a member whom the board makes an admin"
    );
    th.teardown().await;
}

/// Who manages the board's roles still chooses them: an admin's request is stored as it
/// is, also on a board with a minimum role, and also when it names no role at all.
#[tokio::test]
async fn add_member_by_a_manager_of_roles_gives_the_roles_of_the_request() {
    let th = Setup::native().start().await.init_basic().await;
    let user1 = th.client.me().await;
    let board = board_with_admin(
        &th,
        new_board(TEST_TEAM_ID, BoardType::Open, "open"),
        &user1.id,
    )
    .await;
    th.client
        .patch(
            &format!("/boards/{}", board.id),
            &json!({"minimumRole": "commenter"}),
        )
        .await
        .check_ok();

    let user3 = th.add_user("user3").await.me().await;
    let added: BoardMember = th
        .client
        .add_member(&editor(&board.id, &user3.id))
        .await
        .ok_json();
    assert_eq!(role_flags(&added), ONLY_EDITOR, "above the minimum role");

    let user4 = th.add_user("user4").await.me().await;
    let added: BoardMember = th
        .client
        .add_member(&BoardMember {
            scheme_viewer: true,
            ..member(&board.id, &user4.id)
        })
        .await
        .ok_json();
    assert_eq!(role_flags(&added), ONLY_VIEWER, "below the minimum role");

    // Preserved quirk: a request without a role makes a member without one.
    let user5 = th.add_user("user5").await.me().await;
    let added: BoardMember = th
        .client
        .add_member(&member(&board.id, &user5.id))
        .await
        .ok_json();
    assert_eq!(role_flags(&added), NO_ROLE, "no role asked, none given");

    th.teardown().await;
}

/// Verified on the original: who may add members. An editor may on an open board but
/// not on a private one; a commenter may on neither.
#[tokio::test]
async fn add_member_permissions_by_board_type_and_role() {
    let th = Setup::native().start().await.init_basic().await;
    let user1 = th.client.me().await;
    let user2 = th.client2.me().await;
    let user3 = th.add_user("user3").await.me().await;
    let open = board_with_admin(
        &th,
        new_board(TEST_TEAM_ID, BoardType::Open, "open"),
        &user1.id,
    )
    .await;
    let private = board_with_admin(
        &th,
        new_board(TEST_TEAM_ID, BoardType::Private, "private"),
        &user1.id,
    )
    .await;

    // A commenter of an open board.
    add_member(
        &th,
        &BoardMember {
            scheme_commenter: true,
            ..member(&open.id, &user2.id)
        },
    )
    .await;
    th.client2
        .add_member(&editor(&open.id, &user3.id))
        .await
        .check_forbidden();

    // An editor of a private board.
    add_member(&th, &editor(&private.id, &user2.id)).await;
    th.client2
        .add_member(&editor(&private.id, &user3.id))
        .await
        .check_forbidden();

    // An editor of an open board.
    th.app
        .update_board_member(&editor(&open.id, &user2.id))
        .await
        .expect("user2 becomes an editor");
    th.client2
        .add_member(&editor(&open.id, &user3.id))
        .await
        .check_ok();

    th.teardown().await;
}

// -- TestAddMemberToBoard (server/app/boards_test.go) ---------------------------------

/// `TestAddMemberToBoard/"base case"` in `server/app/boards_test.go`, through the real
/// store.
#[tokio::test]
async fn add_member_to_board_base_case() {
    let th = Setup::native().start().await;
    let board =
        board_without_members(&th, new_board("team_id_1", BoardType::Open, ""), "creator").await;
    let new_member = editor(&board.id, "user_id_1");

    let added = add_member(&th, &new_member).await;

    assert_eq!(added, new_member, "the membership as asked for");
    assert_eq!(
        members_of(&th, &board.id).await,
        vec![new_member],
        "and stored"
    );
    assert_eq!(
        category_name_of(&th, "user_id_1", "team_id_1", &board.id).await,
        Some("Boards".to_owned()),
        "the board is in the member's default category"
    );

    // A board that does not exist has no members to add to.
    let nothing = th
        .app
        .add_member_to_board(&editor("no-such-board", "user_id_1"))
        .await
        .expect("not an error");
    assert_eq!(nothing, None, "nothing is answered");

    th.teardown().await;
}

/// `TestAddMemberToBoard/"return existing non-synthetic membership if any"` in
/// `server/app/boards_test.go`, through the real store.
#[tokio::test]
async fn add_member_to_board_return_existing_non_synthetic_membership_if_any() {
    let th = Setup::native().start().await;
    let board =
        board_without_members(&th, new_board("team_id_1", BoardType::Open, ""), "creator").await;
    let existing = add_member(&th, &editor(&board.id, "user_id_1")).await;

    let answered = add_member(
        &th,
        &BoardMember {
            scheme_admin: true,
            ..member(&board.id, "user_id_1")
        },
    )
    .await;

    assert_eq!(answered, existing, "the existing membership, unchanged");
    assert_eq!(
        members_of(&th, &board.id).await,
        vec![existing],
        "and nothing was written"
    );
    th.teardown().await;
}

// -- TestUpdateMember -----------------------------------------------------------------

/// `TestUpdateMember/"a non authenticated user should be rejected"`
#[tokio::test]
async fn update_member_a_non_authenticated_user_should_be_rejected() {
    let th = Setup::native().start().await.init_basic().await;
    let user1 = th.client.me().await;
    let board = board_with_admin(
        &th,
        new_board(TEST_TEAM_ID, BoardType::Open, "title"),
        &user1.id,
    )
    .await;

    let response = update_member(&th.anonymous(), &editor(&board.id, &user1.id)).await;

    response.check_unauthorized();
    th.teardown().await;
}

/// `TestUpdateMember/"a user without permissions should be rejected"`
#[tokio::test]
async fn update_member_a_user_without_permissions_should_be_rejected() {
    let th = Setup::native().start().await.init_basic().await;
    let user1 = th.client.me().await;
    let board = board_with_admin(
        &th,
        new_board(TEST_TEAM_ID, BoardType::Open, "title"),
        &user1.id,
    )
    .await;

    let response = update_member(&th.client2, &editor(&board.id, &user1.id)).await;

    response.check_forbidden();
    assert_eq!(
        response.error_message(),
        "access denied to modify board members",
        "why"
    );
    th.teardown().await;
}

/// `TestUpdateMember/"non existing board"`
#[tokio::test]
async fn update_member_non_existing_board() {
    let th = Setup::native().start().await.init_basic().await;
    let user1 = th.client.me().await;

    let response = update_member(&th.client, &editor("non-existent-board-id", &user1.id)).await;

    response.check_error(StatusCode::NOT_FOUND, "board not found");
    th.teardown().await;
}

/// `TestUpdateMember/"should correctly update a member for a valid board"`
#[tokio::test]
async fn update_member_should_correctly_update_a_member_for_a_valid_board() {
    let th = Setup::native().start().await.init_basic().await;
    let user1 = th.client.me().await;
    let user2 = th.client2.me().await;
    let board = board_with_admin(
        &th,
        new_board(TEST_TEAM_ID, BoardType::Open, "title"),
        &user1.id,
    )
    .await;
    let added = add_member(&th, &editor(&board.id, &user2.id)).await;
    assert!(!added.scheme_admin && added.scheme_editor, "an editor");
    let update = BoardMember {
        scheme_admin: true,
        scheme_editor: true,
        ..member(&board.id, &user2.id)
    };

    let updated: BoardMember = update_member(&th.client, &update).await.ok_json();

    assert_eq!(updated, update, "the new roles are answered");
    let stored = th
        .app
        .find_member_for_board(&board.id, &user2.id)
        .await
        .expect("the membership is read");
    assert_eq!(stored, Some(update), "and stored");
    th.teardown().await;
}

/// `TestUpdateMember/"should not update a member if that means that a board will not
/// have any admin"`
#[tokio::test]
async fn update_member_should_not_leave_a_board_without_any_admin() {
    let th = Setup::native().start().await.init_basic().await;
    let user1 = th.client.me().await;
    let board = board_with_admin(
        &th,
        new_board(TEST_TEAM_ID, BoardType::Open, "title"),
        &user1.id,
    )
    .await;

    let response = update_member(&th.client, &editor(&board.id, &user1.id)).await;

    // The request is fine; the state of the board stands against it.
    response.check_error(StatusCode::CONFLICT, "cannot leave a board with no admins");
    assert_eq!(response.error_reason(), "last_admin", "the reason");
    let members = members_of(&th, &board.id).await;
    assert_eq!(members.len(), 1, "one member: {members:?}");
    assert!(members[0].scheme_admin, "still an admin");
    th.teardown().await;
}

/// The order of the checks of `PUT /boards/{board_id}/members/{user_id}` and what it
/// answers for a user who is not a member.
#[tokio::test]
async fn update_member_checks() {
    let th = Setup::native().start().await.init_basic().await;
    let user1 = th.client.me().await;
    let user2 = th.client2.me().await;
    let board = board_with_admin(
        &th,
        new_board(TEST_TEAM_ID, BoardType::Private, "title"),
        &user1.id,
    )
    .await;
    let path = format!("/boards/{}/members/{}", board.id, user2.id);

    // Who is no member of the private board learns nothing, whatever the body and
    // whoever the path names. (The original decodes the body and looks the user up
    // first, for anyone.)
    for target in [user2.id.as_str(), "no-such-user"] {
        for body in ["{bad", "{\"schemeEditor\":true}"] {
            let response = th
                .client2
                .send_raw_body(
                    Method::PUT,
                    &format!("/boards/{}/members/{target}", board.id),
                    body,
                )
                .await;
            response.check_error(StatusCode::NOT_FOUND, "board not found");
        }
    }

    // Then the body.
    for body in ["{bad", "[]", "null"] {
        th.client
            .send_raw_body(Method::PUT, &path, body)
            .await
            .check_bad_request();
    }

    // Then the membership: a user who is no member, or nobody at all, has none to
    // change. (The original answers 200 with `null` for the first.)
    for target in [user2.id.as_str(), "no-such-user"] {
        let response = th
            .client
            .put(
                &format!("/boards/{}/members/{target}", board.id),
                &json!({"schemeEditor": true}),
            )
            .await;
        response.check_error(StatusCode::NOT_FOUND, "member not found");
    }
    assert_eq!(
        members_of(&th, &board.id).await.len(),
        1,
        "nobody was added"
    );

    // The user and the board come from the path, whatever the body names.
    add_member(&th, &editor(&board.id, &user2.id)).await;
    let updated: BoardMember = th
        .client
        .put(
            &path,
            &json!({"userId": "ignored", "boardId": "ignored", "schemeAdmin": true}),
        )
        .await
        .ok_json();
    assert_eq!(
        updated,
        BoardMember {
            scheme_admin: true,
            ..member(&board.id, &user2.id)
        },
        "the path decides whose membership it is"
    );

    // With a second admin, the first may be demoted, and then no longer manages roles.
    update_member(&th.client, &editor(&board.id, &user1.id))
        .await
        .check_ok();
    update_member(&th.client, &editor(&board.id, &user2.id))
        .await
        .check_forbidden();
    // The remaining admin cannot demote themselves.
    update_member(&th.client2, &editor(&board.id, &user2.id))
        .await
        .check_status(StatusCode::CONFLICT);

    th.teardown().await;
}

/// In single-user mode there is no user record to look up, and the single user's
/// membership can be updated.
#[tokio::test]
async fn update_member_in_single_user_mode() {
    let th = Setup::single_user().start().await;
    let board = th.create_board(TEST_TEAM_ID, BoardType::Open).await;

    let response = update_member(
        &th.client,
        &BoardMember {
            scheme_admin: true,
            scheme_viewer: true,
            ..member(&board.id, "single-user")
        },
    )
    .await;

    let updated: BoardMember = response.ok_json();
    assert!(
        updated.scheme_admin && updated.scheme_viewer && !updated.scheme_editor,
        "the new roles: {updated:?}"
    );
    th.teardown().await;
}

// -- TestDeleteMember -----------------------------------------------------------------

/// `TestDeleteMember/"a non authenticated user should be rejected"`
#[tokio::test]
async fn delete_member_a_non_authenticated_user_should_be_rejected() {
    let th = Setup::native().start().await.init_basic().await;
    let user1 = th.client.me().await;
    let board = board_with_admin(
        &th,
        new_board(TEST_TEAM_ID, BoardType::Open, "title"),
        &user1.id,
    )
    .await;

    let response = delete_member(&th.anonymous(), &member(&board.id, &user1.id)).await;

    response.check_unauthorized();
    th.teardown().await;
}

/// `TestDeleteMember/"a user without permissions should be rejected"`
#[tokio::test]
async fn delete_member_a_user_without_permissions_should_be_rejected() {
    let th = Setup::native().start().await.init_basic().await;
    let user1 = th.client.me().await;
    let board = board_with_admin(
        &th,
        new_board(TEST_TEAM_ID, BoardType::Open, "title"),
        &user1.id,
    )
    .await;

    let response = delete_member(&th.client2, &member(&board.id, &user1.id)).await;

    response.check_forbidden();
    assert_eq!(
        response.error_message(),
        "access denied to modify board members",
        "why"
    );
    th.teardown().await;
}

/// `TestDeleteMember/"non existing board"`
#[tokio::test]
async fn delete_member_non_existing_board() {
    let th = Setup::native().start().await.init_basic().await;
    let user1 = th.client.me().await;

    let response = delete_member(&th.client, &member("non-existent-board-id", &user1.id)).await;

    response.check_not_found();
    th.teardown().await;
}

/// A private board with user1 as admin and user2 as editor, as the subtests of
/// `TestDeleteMember/"should correctly delete a member for a valid board"` set up.
async fn private_board_with_users(th: &TestServer) -> Board {
    let user1 = th.client.me().await;
    let user2 = th.client2.me().await;
    let board = board_with_admin(
        th,
        new_board(TEST_TEAM_ID, BoardType::Private, "title"),
        &user1.id,
    )
    .await;
    let added = add_member(th, &editor(&board.id, &user2.id)).await;
    assert!(!added.scheme_admin && added.scheme_editor, "an editor");
    assert_eq!(members_of(th, &board.id).await.len(), 2, "two members");
    board
}

/// `TestDeleteMember/"should correctly delete a member for a valid board"/"admin removing
/// a user"`
#[tokio::test]
async fn delete_member_admin_removing_a_user() {
    let th = Setup::native().start().await.init_basic().await;
    let user2 = th.client2.me().await;
    let board = private_board_with_users(&th).await;

    let response = delete_member(&th.client, &member(&board.id, &user2.id)).await;

    response.check_ok();
    assert_eq!(response.text(), "{}", "an empty object");
    assert_eq!(members_of(&th, &board.id).await.len(), 1, "one member left");
    th.teardown().await;
}

/// `TestDeleteMember/"should correctly delete a member for a valid board"/"user removing
/// themselves"`: this fails, a user leaves a board through the leave route.
#[tokio::test]
async fn delete_member_user_removing_themselves() {
    let th = Setup::native().start().await.init_basic().await;
    let user2 = th.client2.me().await;
    let board = private_board_with_users(&th).await;

    let response = delete_member(&th.client2, &member(&board.id, &user2.id)).await;

    response.check_forbidden();
    assert_eq!(members_of(&th, &board.id).await.len(), 2, "still two");
    th.teardown().await;
}

/// `TestDeleteMember/"should correctly delete a member for a valid board"/"a non admin
/// user should not be able to remove another user"`
#[tokio::test]
async fn delete_member_a_non_admin_user_should_not_be_able_to_remove_another_user() {
    let th = Setup::native().start().await.init_basic().await;
    let user1 = th.client.me().await;
    let board = private_board_with_users(&th).await;

    let response = delete_member(&th.client2, &member(&board.id, &user1.id)).await;

    response.check_forbidden();
    assert_eq!(members_of(&th, &board.id).await.len(), 2, "still two");
    th.teardown().await;
}

/// `TestDeleteMember/"should not delete a member if that means that a board will not
/// have any admin"`
#[tokio::test]
async fn delete_member_should_not_leave_a_board_without_any_admin() {
    let th = Setup::native().start().await.init_basic().await;
    let user1 = th.client.me().await;
    let board = board_with_admin(
        &th,
        new_board(TEST_TEAM_ID, BoardType::Private, "title"),
        &user1.id,
    )
    .await;

    let response = delete_member(&th.client, &member(&board.id, &user1.id)).await;

    // The request is fine; the state of the board stands against it.
    response.check_error(StatusCode::CONFLICT, "cannot leave a board with no admins");
    assert_eq!(response.error_reason(), "last_admin", "the reason");
    let members = members_of(&th, &board.id).await;
    assert_eq!(members.len(), 1, "one member: {members:?}");
    assert!(members[0].scheme_admin, "still an admin");

    // Verified on the original: removing somebody who is not a member succeeds.
    delete_member(&th.client, &member(&board.id, "no-such-user"))
        .await
        .check_ok();
    th.teardown().await;
}

// -- TestDuplicateBoard ---------------------------------------------------------------

/// `TestDuplicateBoard/"create and duplicate public board"`
#[tokio::test]
async fn duplicate_board_create_and_duplicate_public_board() {
    let th = Setup::native().start().await.init_basic().await;
    let me = th.client.me().await;
    let user2 = th.client2.me().await;
    let board: Board = th
        .client
        .create_board(&new_board(TEST_TEAM_ID, BoardType::Open, "Public board"))
        .await
        .ok_json();
    assert_eq!(board.created_by, me.id, "created by the session's user");
    insert_block(&th, view_block(&board.id), &me.id).await;
    th.client
        .add_member(&editor(&board.id, &user2.id))
        .await
        .check_ok();
    assert_eq!(members_of(&th, &board.id).await.len(), 2, "two members");

    // Duplicate the board
    let copy: BoardsAndBlocks = duplicate_board(
        &th.client,
        &board.id,
        &format!("?asTemplate=false&toTeam={TEST_TEAM_ID}"),
    )
    .await
    .ok_json();

    assert_eq!(copy.boards.len(), 1, "one board");
    assert_eq!(copy.blocks.len(), 1, "one block");
    let duplicate = &copy.boards[0];
    assert_eq!(
        duplicate.board_type,
        BoardType::Private,
        "Duplicated board should be private"
    );
    assert_ne!(duplicate.id, board.id, "under a new identifier");
    assert_eq!(copy.blocks[0].board_id, duplicate.id, "with its block");
    assert_eq!(copy.blocks[0].title, "View 1", "a copy of the view");

    let members = members_of(&th, &duplicate.id).await;
    assert_eq!(
        members.len(),
        1,
        "Duplicated board should only have one member"
    );
    assert_eq!(members[0].user_id, me.id, "the user who duplicated");
    assert_eq!(members[0].board_id, duplicate.id, "of the copy");
    assert!(members[0].scheme_admin, "as an admin");

    assert_eq!(
        category_name_of(&th, &me.id, TEST_TEAM_ID, &duplicate.id).await,
        Some("Boards".to_owned()),
        "the copy is in the default category, like its source"
    );
    th.teardown().await;
}

/// `TestDuplicateBoard/"create and duplicate public board from a custom category"`
#[tokio::test]
async fn duplicate_board_create_and_duplicate_public_board_from_a_custom_category() {
    let th = Setup::native().start().await.init_basic().await;
    let me = th.client.me().await;
    let user2 = th.client2.me().await;
    let category = th
        .app
        .create_category(Category {
            name: "My Category".to_owned(),
            user_id: me.id.clone(),
            team_id: TEST_TEAM_ID.to_owned(),
            ..Category::default()
        })
        .await
        .expect("the category is created");
    let board: Board = th
        .client
        .create_board(&new_board(TEST_TEAM_ID, BoardType::Open, "Public board"))
        .await
        .ok_json();

    // move board to custom category
    th.app
        .add_update_user_category_board(
            TEST_TEAM_ID,
            &me.id,
            &category.id,
            std::slice::from_ref(&board.id),
        )
        .await
        .expect("the board is moved to the custom category");
    insert_block(&th, view_block(&board.id), &me.id).await;
    th.client
        .add_member(&editor(&board.id, &user2.id))
        .await
        .check_ok();
    assert_eq!(members_of(&th, &board.id).await.len(), 2, "two members");

    // Duplicate the board
    let copy: BoardsAndBlocks = duplicate_board(
        &th.client,
        &board.id,
        &format!("?asTemplate=false&toTeam={TEST_TEAM_ID}"),
    )
    .await
    .ok_json();

    assert_eq!(copy.boards.len(), 1, "one board");
    assert_eq!(copy.blocks.len(), 1, "one block");
    let duplicate = &copy.boards[0];
    assert_eq!(
        duplicate.board_type,
        BoardType::Private,
        "Duplicated board should be private"
    );
    assert_eq!(duplicate.title, "Public board copy", "the title says so");

    let members = members_of(&th, &duplicate.id).await;
    assert_eq!(
        members.len(),
        1,
        "Duplicated board should only have one member"
    );
    assert_eq!(members[0].user_id, me.id, "the user who duplicated");
    assert!(members[0].scheme_admin, "as an admin");

    // verify duplicated board is in the same custom category
    assert_eq!(
        category_name_of(&th, &me.id, TEST_TEAM_ID, &duplicate.id).await,
        Some("My Category".to_owned()),
        "the copy is filed where its source is"
    );
    th.teardown().await;
}

/// `TestDuplicateBoard/"base case"` and `"duplicating board as template should not set
/// it's category"` in `server/app/boards_test.go`, through the real store.
#[tokio::test]
async fn duplicate_board_base_case_and_as_template() {
    let th = Setup::native().start().await;
    let board = board_with_admin(
        &th,
        new_board("team_id_1", BoardType::Open, "Source"),
        "user_id_1",
    )
    .await;
    insert_block(&th, view_block(&board.id), "user_id_1").await;

    let (copy, members) = th
        .app
        .duplicate_board(&board.id, "user_id_1", "team_id_1", DuplicateAs::Copy)
        .await
        .expect("the board is duplicated");
    assert_eq!(copy.boards.len(), 1, "one board");
    assert_eq!(copy.blocks.len(), 1, "one block");
    assert_eq!(members.len(), 1, "one membership");
    assert_eq!(
        category_name_of(&th, "user_id_1", "team_id_1", &copy.boards[0].id).await,
        Some("Boards".to_owned()),
        "a copy is filed under a category"
    );

    let (template, members) = th
        .app
        .duplicate_board(&board.id, "user_id_1", "team_id_1", DuplicateAs::Template)
        .await
        .expect("the board is duplicated as a template");
    assert!(template.boards[0].is_template, "a template");
    assert_eq!(members.len(), 1, "one membership");
    assert_eq!(
        category_name_of(&th, "user_id_1", "team_id_1", &template.boards[0].id).await,
        None,
        "a template is filed nowhere"
    );
    th.teardown().await;
}

/// Verified on the original: who may duplicate what, and what the query parameters do.
#[tokio::test]
async fn duplicate_board_permissions_and_parameters() {
    let th = Setup::native().start().await.init_basic().await;
    let user1 = th.client.me().await;
    let user2 = th.client2.me().await;
    let board = board_with_admin(
        &th,
        new_board(TEST_TEAM_ID, BoardType::Open, "Public board"),
        &user1.id,
    )
    .await;
    let card = insert_block(
        &th,
        Block {
            id: new_id(IdKind::Card),
            board_id: board.id.clone(),
            block_type: BlockType::Card,
            title: "Card 1".to_owned(),
            ..Block::default()
        },
        &user1.id,
    )
    .await;
    insert_block(
        &th,
        Block {
            id: new_id(IdKind::Block),
            board_id: board.id.clone(),
            parent_id: card.id.clone(),
            block_type: BlockType::Comment,
            title: "a comment".to_owned(),
            ..Block::default()
        },
        &user1.id,
    )
    .await;

    duplicate_board(&th.anonymous(), &board.id, "")
        .await
        .check_unauthorized();
    duplicate_board(&th.client, "nonexistent", "")
        .await
        .check_not_found();

    // An open board that is not a template can only be duplicated by its members.
    let response = duplicate_board(&th.client2, &board.id, "").await;
    response.check_forbidden();
    assert_eq!(response.error_message(), "access denied to board", "why");

    // Without `toTeam` the copy stays in the team of the source; comments are not
    // copied.
    let copy: BoardsAndBlocks = duplicate_board(&th.client, &board.id, "").await.ok_json();
    assert_eq!(copy.boards[0].team_id, TEST_TEAM_ID, "the source's team");
    assert_eq!(copy.boards[0].title, "Public board copy", "a copy");
    assert_eq!(copy.blocks.len(), 1, "the card without its comment");
    assert_eq!(copy.blocks[0].title, "Card 1", "the card");

    // `asTemplate=true` makes a template, which is filed under no category. Only the
    // exact value counts.
    let template: BoardsAndBlocks = duplicate_board(&th.client, &board.id, "?asTemplate=true")
        .await
        .ok_json();
    assert!(template.boards[0].is_template, "a template");
    assert_eq!(template.boards[0].title, "New board template", "its title");
    assert_eq!(
        category_name_of(&th, &user1.id, TEST_TEAM_ID, &template.boards[0].id).await,
        None,
        "not in a category"
    );
    let not_a_template: BoardsAndBlocks =
        duplicate_board(&th.client, &board.id, "?asTemplate=True")
            .await
            .ok_json();
    assert!(
        !not_a_template.boards[0].is_template,
        "`True` is not `true`"
    );

    // `toTeam` puts the copy, and its category, into another team.
    let moved: BoardsAndBlocks = duplicate_board(&th.client, &board.id, "?toTeam=other-team")
        .await
        .ok_json();
    assert_eq!(moved.boards[0].team_id, "other-team", "the other team");
    assert_eq!(
        category_name_of(&th, &user1.id, "other-team", &moved.boards[0].id).await,
        Some("Boards".to_owned()),
        "in the default category of that team"
    );

    // A viewer of a private board may duplicate it, and administers the copy.
    let private = board_with_admin(
        &th,
        new_board(TEST_TEAM_ID, BoardType::Private, "Private one"),
        &user1.id,
    )
    .await;
    insert_block(&th, view_block(&private.id), &user1.id).await;
    add_member(
        &th,
        &BoardMember {
            scheme_viewer: true,
            ..member(&private.id, &user2.id)
        },
    )
    .await;
    let copy: BoardsAndBlocks = duplicate_board(&th.client2, &private.id, "")
        .await
        .ok_json();
    assert_eq!(copy.boards[0].created_by, user2.id, "created by the viewer");
    let members = members_of(&th, &copy.boards[0].id).await;
    assert_eq!(members.len(), 1, "one member: {members:?}");
    assert_eq!(members[0].user_id, user2.id, "the viewer");
    assert!(members[0].scheme_admin, "administers the copy");

    th.teardown().await;
}

/// The part of `TestGetTemplates` that belongs to this area: any user of the team
/// duplicates an open template into a board of their own. Verified on the original, also
/// for a template of the global team and for a private template.
#[tokio::test]
async fn duplicate_board_from_a_template() {
    let th = Setup::native().start().await.init_basic().await;
    let user1 = th.client.me().await;
    let user2 = th.client2.me().await;
    let mut templates = Vec::new();
    for (team_id, board_type, title) in [
        (TEST_TEAM_ID, BoardType::Open, "Open template"),
        ("0", BoardType::Open, "Global template"),
        ("0", BoardType::Private, "Private template"),
    ] {
        let template = board_with_admin(
            &th,
            Board {
                is_template: true,
                ..new_board(team_id, board_type, title)
            },
            &user1.id,
        )
        .await;
        insert_block(&th, view_block(&template.id), &user1.id).await;
        templates.push(template);
    }

    // user2 is not a member of any of them.
    let copy: BoardsAndBlocks = duplicate_board(
        &th.client2,
        &templates[0].id,
        &format!("?asTemplate=false&toTeam={TEST_TEAM_ID}"),
    )
    .await
    .ok_json();
    let board = &copy.boards[0];
    assert_eq!(board.title, "Open template", "the template's title");
    assert!(!board.is_template, "a board, not a template");
    assert_eq!(board.board_type, BoardType::Private, "private to its user");
    assert_eq!(board.created_by, user2.id, "created by the user");
    assert_eq!(copy.blocks.len(), 1, "with the template's blocks");
    let read: Board = get_board(&th.client2, &board.id).await.ok_json();
    assert_eq!(&read, board, "the user reads their new board");
    let stored_blocks = th
        .app
        .store()
        .get_blocks(&BlockFilter::board(&board.id))
        .await
        .expect("the blocks are read");
    assert_eq!(
        stored_blocks.len(),
        copy.blocks.len(),
        "the blocks are stored"
    );
    assert_eq!(
        category_name_of(&th, &user2.id, TEST_TEAM_ID, &board.id).await,
        Some("Boards".to_owned()),
        "filed under the user's default category"
    );

    // A template of a template keeps being one, and says "copy".
    let copy: BoardsAndBlocks = duplicate_board(&th.client2, &templates[0].id, "?asTemplate=true")
        .await
        .ok_json();
    assert!(copy.boards[0].is_template, "a template");
    assert_eq!(copy.boards[0].title, "Open template copy", "a copy");

    // A template of the global team, into the user's team.
    let copy: BoardsAndBlocks = duplicate_board(
        &th.client2,
        &templates[1].id,
        &format!("?toTeam={TEST_TEAM_ID}"),
    )
    .await
    .ok_json();
    assert_eq!(copy.boards[0].team_id, TEST_TEAM_ID, "in the user's team");

    // A private template is for its members only; for others it does not exist.
    duplicate_board(
        &th.client2,
        &templates[2].id,
        &format!("?toTeam={TEST_TEAM_ID}"),
    )
    .await
    .check_error(StatusCode::NOT_FOUND, "board not found");

    th.teardown().await;
}

/// A board without blocks is duplicated as a board without blocks. (The original
/// cannot duplicate it and answers 500.)
#[tokio::test]
async fn duplicate_board_without_blocks() {
    let th = Setup::native().start().await.init_basic().await;
    let board = th.create_board(TEST_TEAM_ID, BoardType::Open).await;

    let copy: BoardsAndBlocks = duplicate_board(&th.client, &board.id, "").await.ok_json();

    assert_eq!(copy.boards.len(), 1, "one board");
    assert!(copy.blocks.is_empty(), "no blocks");
    let boards: Vec<Board> = th
        .client
        .get(&format!("/teams/{TEST_TEAM_ID}/boards"))
        .await
        .ok_json();
    let mut expected = vec![board.id.clone(), copy.boards[0].id.clone()];
    expected.sort_unstable();
    assert_eq!(sorted_ids(&boards), expected, "the board and its copy");
    th.teardown().await;
}

// -- TestJoinBoard --------------------------------------------------------------------

/// `TestJoinBoard/"create and join public board"`
#[tokio::test]
async fn join_board_create_and_join_public_board() {
    let th = Setup::native().start().await.init_basic().await;
    let me = th.client.me().await;
    let user2 = th.client2.me().await;
    let board: Board = th
        .client
        .create_board(&new_board(
            TEST_TEAM_ID,
            BoardType::Open,
            "Test Public board",
        ))
        .await
        .ok_json();
    assert_eq!(board.created_by, me.id, "created by the session's user");
    assert_eq!(board.minimum_role, BoardRole::None, "no minimum role");

    let joined: BoardMember = join_board(&th.client2, &board.id).await.ok_json();

    assert_eq!(joined.board_id, board.id, "of the board");
    assert_eq!(joined.user_id, user2.id, "the joining user");
    assert_eq!(
        category_name_of(&th, &user2.id, TEST_TEAM_ID, &board.id).await,
        Some("Boards".to_owned()),
        "the board is filed under the new member's default category"
    );

    // Verified on the original: joining again, or as the admin, answers the membership
    // there is.
    let again: BoardMember = join_board(&th.client2, &board.id).await.ok_json();
    assert_eq!(again, joined, "the same membership");
    let admin: BoardMember = join_board(&th.client, &board.id).await.ok_json();
    assert!(admin.scheme_admin, "the admin stays an admin: {admin:?}");
    th.teardown().await;
}

/// `TestJoinBoard/"create and join public board should match the minimumRole in the
/// membership"`, and the same for the other minimum roles (verified on the original).
#[tokio::test]
async fn join_board_should_match_the_minimum_role_in_the_membership() {
    let th = Setup::native().start().await.init_basic().await;
    let user2 = th.client2.me().await;

    for (minimum_role, expected) in [
        (
            BoardRole::Commenter,
            BoardMember {
                scheme_commenter: true,
                ..BoardMember::default()
            },
        ),
        (
            BoardRole::Viewer,
            BoardMember {
                scheme_viewer: true,
                ..BoardMember::default()
            },
        ),
        (
            BoardRole::Editor,
            BoardMember {
                scheme_editor: true,
                ..BoardMember::default()
            },
        ),
        (
            BoardRole::Admin,
            BoardMember {
                scheme_admin: true,
                ..BoardMember::default()
            },
        ),
    ] {
        let board: Board = th
            .client
            .create_board(&Board {
                minimum_role,
                ..new_board(TEST_TEAM_ID, BoardType::Open, "Public board with a role")
            })
            .await
            .ok_json();

        let joined: BoardMember = join_board(&th.client2, &board.id).await.ok_json();

        assert_eq!(
            joined,
            BoardMember {
                board_id: board.id.clone(),
                user_id: user2.id.clone(),
                ..expected
            },
            "joining a board whose minimum role is {minimum_role:?}"
        );
    }
    th.teardown().await;
}

/// `TestJoinBoard/"create and join public board should match editor role in the
/// membership when MinimumRole is empty"`
#[tokio::test]
async fn join_board_should_match_editor_role_when_minimum_role_is_empty() {
    let th = Setup::native().start().await.init_basic().await;
    let user2 = th.client2.me().await;
    let board: Board = th
        .client
        .create_board(&new_board(
            TEST_TEAM_ID,
            BoardType::Open,
            "Public board for editors",
        ))
        .await
        .ok_json();

    let joined: BoardMember = join_board(&th.client2, &board.id).await.ok_json();

    assert_eq!(
        joined,
        editor(&board.id, &user2.id),
        "new member should be editor and nothing else"
    );
    th.teardown().await;
}

/// A template is not joined. An open template is there for every user of its team to
/// copy; joining made anyone its editor, and the built-in templates are open boards of
/// the team every user is in. (The original lets anyone join them.)
#[tokio::test]
async fn join_board_refuses_a_template() {
    let th = Setup::native().start().await.init_basic().await;
    let user1 = th.client.me().await;
    let template = board_with_admin(
        &th,
        Board {
            is_template: true,
            ..new_board(TEST_TEAM_ID, BoardType::Open, "an open template")
        },
        &user1.id,
    )
    .await;

    let response = join_board(&th.client2, &template.id).await;
    response.check_error(StatusCode::FORBIDDEN, "a template cannot be joined");
    assert_eq!(
        members_of(&th, &template.id).await.len(),
        1,
        "nobody joined"
    );
    // It can still be read and copied by who is no member.
    get_board(&th.client2, &template.id).await.check_ok();

    th.teardown().await;
}

/// `TestJoinBoard/"create and join private board (should not succeed)"`
#[tokio::test]
async fn join_board_create_and_join_private_board_should_not_succeed() {
    let th = Setup::native().start().await.init_basic().await;
    let board: Board = th
        .client
        .create_board(&new_board(
            TEST_TEAM_ID,
            BoardType::Private,
            "Private board",
        ))
        .await
        .ok_json();

    // Verified on the original: `allow_admin` is for team administrators, which a
    // standalone server does not have.
    for query in ["", "?allow_admin", "?allow_admin=true"] {
        let response = th
            .client2
            .post_empty(&format!("/boards/{}/join{query}", board.id))
            .await;
        // A private board does not exist for who is no member, so there is nothing
        // to join.
        response.check_error(StatusCode::NOT_FOUND, "board not found");
        assert_eq!(response.error_code(), "not_found", "{query}");
    }
    assert_eq!(members_of(&th, &board.id).await.len(), 1, "nobody joined");
    th.teardown().await;
}

/// `TestJoinBoard/"join invalid board"`
#[tokio::test]
async fn join_board_join_invalid_board() {
    let th = Setup::native().start().await.init_basic().await;

    let response = join_board(&th.client2, "nonexistent-board-ID").await;

    response.check_not_found();
    th.teardown().await;
}

// -- leaving a board ------------------------------------------------------------------

/// Not in the original's tests; verified on the original: a member leaves a board, the
/// last admin cannot, and a user who is not a member is refused.
#[tokio::test]
async fn leave_board_cases() {
    let th = Setup::native().start().await.init_basic().await;
    let board = board_with_users(&th).await;

    leave_board(&th.anonymous(), &board.id)
        .await
        .check_unauthorized();
    let response = leave_board(&th.client, "nonexistent").await;
    response.check_error(StatusCode::NOT_FOUND, "board not found");
    let outsider = th.add_user("user3").await;
    leave_board(&outsider, &board.id).await.check_forbidden();

    let response = leave_board(&th.client2, &board.id).await;
    response.check_ok();
    assert_eq!(response.text(), "{}", "an empty object");
    assert_eq!(members_of(&th, &board.id).await.len(), 1, "user2 left");
    leave_board(&th.client2, &board.id).await.check_forbidden();

    let response = leave_board(&th.client, &board.id).await;
    response.check_status(StatusCode::CONFLICT);
    assert_eq!(response.error_reason(), "last_admin", "the reason");
    assert_eq!(
        response.error_message(),
        "cannot leave a board with no admins",
        "the last admin stays"
    );

    // Memberships outlive a deleted board, but it cannot be left any more.
    join_board(&th.client2, &board.id).await.check_ok();
    th.client
        .delete(&format!("/boards/{}", board.id))
        .await
        .check_ok();
    leave_board(&th.client2, &board.id).await.check_not_found();

    th.teardown().await;
}

// -- users that do not exist ----------------------------------------------------------

/// A client whose session belongs to a user that has no record, as after the user was
/// removed from the database.
async fn client_of_missing_user(th: &TestServer, user_id: &str) -> Client {
    let token = new_id(IdKind::Token);
    let session = Session {
        id: new_id(IdKind::Session),
        token: token.clone(),
        user_id: user_id.to_owned(),
        auth_service: "native".to_owned(),
        props: JsonMap::new(),
        create_at: 0,
        update_at: 0,
    };
    th.app
        .store()
        .create_session(&session)
        .await
        .expect("the session is stored");
    th.client_with_token(&token)
}

/// There are no guests on a standalone server, but where the original asks whether the
/// user is one, a user without a record is a 404. Verified on the original by deleting
/// the record of a signed-in user: these are its answers, in the order of its checks.
#[tokio::test]
async fn a_session_whose_user_is_gone_gets_not_found_where_the_user_is_looked_up() {
    let th = Setup::native().start().await.init_basic().await;
    let user1 = th.client.me().await;
    let ghost = client_of_missing_user(&th, "ghost").await;
    let open = board_with_admin(
        &th,
        new_board(TEST_TEAM_ID, BoardType::Open, "open"),
        &user1.id,
    )
    .await;
    let private = board_with_admin(
        &th,
        new_board(TEST_TEAM_ID, BoardType::Private, "private"),
        &user1.id,
    )
    .await;
    add_member(&th, &editor(&open.id, "ghost")).await;
    let card = json!({"id": "b1", "boardId": "bd", "type": "card", "createAt": 1, "updateAt": 1});

    let not_found = [
        ghost.get(&format!("/teams/{TEST_TEAM_ID}/boards")).await,
        ghost
            .post("/boards", &json!({"teamId": TEST_TEAM_ID, "type": "O"}))
            .await,
        // The user is looked up before the board is validated...
        ghost
            .post("/boards", &json!({"teamId": TEST_TEAM_ID, "type": "X"}))
            .await,
        get_board(&ghost, &open.id).await,
        // ...and before the board is found to have no blocks to copy.
        duplicate_board(&ghost, &open.id, "").await,
        join_board(&ghost, &open.id).await,
        ghost
            .post(
                "/boards-and-blocks",
                &json!({
                    "boards": [{"id": "bd", "teamId": TEST_TEAM_ID, "type": "O"}],
                    "blocks": [card],
                }),
            )
            .await,
    ];
    for response in &not_found {
        response.check_not_found();
        assert_eq!(response.error_code(), "not_found", "the user");
    }

    // A membership is changed without looking the member up as a user.
    update_member(&th.client, &editor(&open.id, "ghost"))
        .await
        .check_ok();

    // The checks that come before the lookup still come first.
    let response = ghost.post("/boards", &json!({"type": "O"})).await;
    response.check_forbidden();
    get_board(&ghost, &private.id).await.check_not_found();
    join_board(&ghost, &private.id).await.check_not_found();

    // Where the original does not ask, the membership is all that counts.
    get_members(&ghost, &open.id).await.check_ok();
    let patched: Board = patch_board(&ghost, &open.id, &title_patch("by a ghost"))
        .await
        .ok_json();
    assert_eq!(patched.modified_by, "ghost", "the editor without a record");
    delete_member(&th.client, &member(&open.id, "ghost"))
        .await
        .check_ok();
    leave_board(&ghost, &open.id).await.check_forbidden();

    th.teardown().await;
}

// -- TestPatchBoard (server/app/boards_test.go) ---------------------------------------

/// `TestPatchBoard/"base case, title patch"` in `server/app/boards_test.go`, through the
/// real store.
#[tokio::test]
async fn app_patch_board_base_case_title_patch() {
    let th = Setup::native().start().await;
    let board = board_with_admin(
        &th,
        new_board("team_id_1", BoardType::Open, "Title"),
        "user_id_1",
    )
    .await;

    let patched = th
        .app
        .patch_board(&title_patch("Patched Title"), &board.id, "user_id_1")
        .await
        .expect("the board is patched");

    assert_eq!(patched.title, "Patched Title", "the new title");
    assert_eq!(stored_board(&th, &board.id).await, patched, "and stored");
    th.teardown().await;
}

/// `TestPatchBoard/"patch type channel, user without post permissions"`, `"patch type
/// channel, user with post permissions"` and `"patch type remove channel, user without
/// post permissions"` in `server/app/boards_test.go`. On a standalone server everybody
/// with a session may post to any channel, so only a patch on behalf of nobody is
/// refused.
#[tokio::test]
async fn app_patch_board_channel() {
    let th = Setup::native().start().await;
    let board = board_with_admin(
        &th,
        new_board("team_id_1", BoardType::Open, "Title"),
        "user_id_2",
    )
    .await;
    let link = BoardPatch {
        channel_id: Some("myChannel".to_owned()),
        ..BoardPatch::default()
    };
    let unlink = BoardPatch {
        board_type: Some(BoardType::Open),
        channel_id: Some(String::new()),
        ..BoardPatch::default()
    };

    // "user without post permissions"
    let error = th
        .app
        .patch_board(&link, &board.id, "")
        .await
        .expect_err("nobody may not link a channel");
    assert_eq!(error.status(), StatusCode::FORBIDDEN, "{error:?}");
    assert_eq!(error.to_string(), "access denied to channel", "why");

    // "user with post permissions"
    let patched = th
        .app
        .patch_board(&link, &board.id, "user_id_2")
        .await
        .expect("the channel is linked");
    assert_eq!(patched.channel_id, "myChannel", "the channel");

    // "patch type remove channel, user without post permissions"
    let error = th
        .app
        .patch_board(&unlink, &board.id, "")
        .await
        .expect_err("nobody may not unlink a channel");
    assert_eq!(error.status(), StatusCode::FORBIDDEN, "{error:?}");
    let patched = th
        .app
        .patch_board(&unlink, &board.id, "user_id_2")
        .await
        .expect("the channel is unlinked");
    assert_eq!(patched.channel_id, "", "no channel");

    // A board that does not exist: the error names it.
    let error = th
        .app
        .patch_board(&link, "no-such-board", "user_id_2")
        .await
        .expect_err("there is no such board");
    assert!(error.is_not_found(), "{error:?}");
    assert_eq!(error.to_string(), "board not found", "the message");

    th.teardown().await;
}

// -- TestBoardCategory (server/app/boards_test.go) ------------------------------------

/// `TestBoardCategory/"no boards default category exists"` in
/// `server/app/boards_test.go`, through the real store: a user who has categories but
/// not the default one gets it when a board needs filing.
#[tokio::test]
async fn board_category_no_boards_default_category_exists() {
    let th = Setup::native().start().await;
    for name in ["Category 1", "Category 2", "Category 3"] {
        th.app
            .create_category(Category {
                name: name.to_owned(),
                user_id: "user_id".to_owned(),
                team_id: "team_id".to_owned(),
                ..Category::default()
            })
            .await
            .expect("the category is created");
    }

    let mut board_ids = Vec::new();
    for title in ["board 1", "board 2", "board 3"] {
        let board =
            board_with_admin(&th, new_board("team_id", BoardType::Open, title), "user_id").await;
        board_ids.push(board.id);
    }

    let categories = categories_of(&th, "user_id", "team_id").await;
    assert_eq!(categories.len(), 4, "the default category was added");
    let default = categories
        .iter()
        .find(|entry| entry.category.name == "Boards")
        .expect("the default category exists");
    let mut filed: Vec<String> = default
        .board_metadata
        .iter()
        .map(|metadata| metadata.board_id.clone())
        .collect();
    filed.sort();
    board_ids.sort();
    assert_eq!(filed, board_ids, "the boards are in the default category");
    th.teardown().await;
}

// -- websocket broadcasts -------------------------------------------------------------

/// How long to wait for a message that should arrive.
const PATIENCE: Duration = Duration::from_secs(5);
/// How long to wait before deciding that no more messages arrive.
const QUIET: Duration = Duration::from_millis(300);

/// A websocket connection of one user, subscribed to one team.
pub(crate) struct TeamSocket {
    stream: WebSocketStream<MaybeTlsStream<TcpStream>>,
}

impl TeamSocket {
    /// Connects, authenticates with the client's token and subscribes to the team.
    pub(crate) async fn subscribe(
        th: &TestServer,
        client: &Client,
        team_id: &str,
        user_id: &str,
    ) -> Self {
        let (stream, _) = tokio_tungstenite::connect_async(client.websocket_url())
            .await
            .expect("the websocket endpoint accepts the connection");
        let mut socket = Self { stream };
        socket
            .send(json!({"action": "AUTH", "token": client.token}))
            .await;
        socket
            .send(json!({"action": "SUBSCRIBE_TEAM", "teamId": team_id}))
            .await;

        // Commands have no acknowledgement: wait until a broadcast comes through.
        let probe = Category {
            id: new_id(IdKind::None),
            name: "subscription probe".to_owned(),
            user_id: user_id.to_owned(),
            team_id: team_id.to_owned(),
            ..Category::default()
        };
        for _ in 0..50 {
            th.app.ws().broadcast_category_change(&probe);
            if socket.next(Duration::from_millis(100)).await.is_some() {
                socket.drain().await;
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

    /// The next JSON message, if one arrives in time.
    async fn next(&mut self, wait: Duration) -> Option<Value> {
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

    /// Reads messages until none arrives for a while and returns them.
    pub(crate) async fn drain(&mut self) -> Vec<Value> {
        let mut messages = Vec::new();
        while let Some(message) = self.next(QUIET).await {
            messages.push(message);
        }
        messages
    }

    /// Waits for a message with the given action whose `path` member equals `expected`,
    /// skipping others, and returns it.
    pub(crate) async fn expect(&mut self, action: &str, path: &str, expected: &str) -> Value {
        let mut skipped = Vec::new();
        while let Some(message) = self.next(PATIENCE).await {
            if message["action"] == json!(action) && message.pointer(path) == Some(&json!(expected))
            {
                return message;
            }
            skipped.push(message);
        }
        panic!("no {action} message with {path} = {expected} arrived; skipped: {skipped:?}");
    }
}

/// The messages with the given action among `messages`.
fn with_action<'m>(messages: &'m [Value], action: &str) -> Vec<&'m Value> {
    messages
        .iter()
        .filter(|message| message["action"] == json!(action))
        .collect()
}

/// Creating a board tells its creator about the board and the membership; patching and
/// deleting it tell the members (`server/app/boards.go:249-266`, `:367`, `:462`).
#[tokio::test]
async fn board_changes_are_broadcast_to_the_members() {
    let th = Setup::native().start().await.init_basic().await;
    let user1 = th.client.me().await;
    let user2 = th.client2.me().await;
    let mut socket1 = TeamSocket::subscribe(&th, &th.client, TEST_TEAM_ID, &user1.id).await;
    let mut socket2 = TeamSocket::subscribe(&th, &th.client2, TEST_TEAM_ID, &user2.id).await;

    let board: Board = th
        .client
        .create_board(&new_board(TEST_TEAM_ID, BoardType::Open, "Roadmap"))
        .await
        .ok_json();
    let message = socket1.expect("UPDATE_BOARD", "/board/id", &board.id).await;
    assert_eq!(message["teamId"], json!(TEST_TEAM_ID), "{message}");
    assert_eq!(message["board"]["title"], json!("Roadmap"), "{message}");
    let message = socket1
        .expect("UPDATE_MEMBER", "/member/boardId", &board.id)
        .await;
    assert_eq!(message["member"]["userId"], json!(user1.id), "{message}");
    assert_eq!(message["member"]["schemeAdmin"], json!(true), "{message}");

    patch_board(&th.client, &board.id, &title_patch("Roadmap 2"))
        .await
        .check_ok();
    let message = socket1.expect("UPDATE_BOARD", "/board/id", &board.id).await;
    assert_eq!(message["board"]["title"], json!("Roadmap 2"), "{message}");

    th.client
        .delete(&format!("/boards/{}", board.id))
        .await
        .check_ok();
    let message = socket1.expect("UPDATE_BOARD", "/board/id", &board.id).await;
    assert!(
        message["board"]["deleteAt"].as_i64() > Some(0),
        "a deletion: {message}"
    );

    undelete_board(&th.client, &board.id).await.check_ok();
    let message = socket1.expect("UPDATE_BOARD", "/board/id", &board.id).await;
    assert_eq!(
        message["board"]["deleteAt"],
        json!(0),
        "restored: {message}"
    );

    let heard_by_outsider = socket2.drain().await;
    assert_eq!(
        heard_by_outsider,
        Vec::<Value>::new(),
        "a user who is not a member hears nothing"
    );
    th.teardown().await;
}

/// Adding, updating and removing a member tell the board's members; the removed user is
/// told too (`server/app/boards.go:551`, `:594`, `:647-654`).
#[tokio::test]
async fn member_changes_are_broadcast() {
    let th = Setup::native().start().await.init_basic().await;
    let user1 = th.client.me().await;
    let user2 = th.client2.me().await;
    let board = board_with_admin(
        &th,
        new_board(TEST_TEAM_ID, BoardType::Private, "title"),
        &user1.id,
    )
    .await;
    let mut socket1 = TeamSocket::subscribe(&th, &th.client, TEST_TEAM_ID, &user1.id).await;
    let mut socket2 = TeamSocket::subscribe(&th, &th.client2, TEST_TEAM_ID, &user2.id).await;

    th.client
        .add_member(&editor(&board.id, &user2.id))
        .await
        .check_ok();
    for socket in [&mut socket1, &mut socket2] {
        let message = socket
            .expect("UPDATE_MEMBER", "/member/userId", &user2.id)
            .await;
        assert_eq!(message["teamId"], json!(TEST_TEAM_ID), "{message}");
        assert_eq!(message["member"]["boardId"], json!(board.id), "{message}");
        assert_eq!(message["member"]["schemeEditor"], json!(true), "{message}");
    }

    update_member(
        &th.client,
        &BoardMember {
            scheme_viewer: true,
            ..member(&board.id, &user2.id)
        },
    )
    .await
    .check_ok();
    let message = socket2
        .expect("UPDATE_MEMBER", "/member/userId", &user2.id)
        .await;
    assert_eq!(message["member"]["schemeViewer"], json!(true), "{message}");
    assert_eq!(message["member"]["schemeEditor"], json!(false), "{message}");

    delete_member(&th.client, &member(&board.id, &user2.id))
        .await
        .check_ok();
    for socket in [&mut socket1, &mut socket2] {
        let message = socket
            .expect("DELETE_MEMBER", "/member/userId", &user2.id)
            .await;
        assert_eq!(message["member"]["boardId"], json!(board.id), "{message}");
    }
    th.teardown().await;
}

/// The "patch type open/private" cases of `TestPatchBoard` in
/// `server/app/boards_test.go` ("no users", "single user", "user with member"), through
/// the real store and a real connection: when a template changes between open and
/// private, the team's users who are not members of it are told that they lost or gained
/// access (`server/app/boards.go:402-432`).
#[tokio::test]
async fn patch_board_type_of_a_template_is_broadcast_to_the_team_users() {
    let th = Setup::native().start().await.init_basic().await;
    let user1 = th.client.me().await;
    let user2 = th.client2.me().await;
    let template = board_with_admin(
        &th,
        Board {
            is_template: true,
            ..new_board(TEST_TEAM_ID, BoardType::Open, "a template")
        },
        &user1.id,
    )
    .await;
    let mut socket1 = TeamSocket::subscribe(&th, &th.client, TEST_TEAM_ID, &user1.id).await;
    let mut socket2 = TeamSocket::subscribe(&th, &th.client2, TEST_TEAM_ID, &user2.id).await;

    // Private: every user who is not a member is told that their access ended. The
    // message goes to the members and to that user.
    patch_board(&th.client, &template.id, &json!({"type": "P"}))
        .await
        .check_ok();
    for socket in [&mut socket1, &mut socket2] {
        let message = socket
            .expect("DELETE_MEMBER", "/member/userId", &user2.id)
            .await;
        assert_eq!(message["teamId"], json!(TEST_TEAM_ID), "{message}");
        assert_eq!(
            message["member"]["boardId"],
            json!(template.id),
            "{message}"
        );
    }
    let rest = socket1.drain().await;
    assert_eq!(
        with_action(&rest, "DELETE_MEMBER"),
        Vec::<&Value>::new(),
        "nothing about the member themselves: {rest:?}"
    );

    // Open: the members are told about a synthetic viewer membership of every other
    // user.
    patch_board(&th.client, &template.id, &json!({"type": "O"}))
        .await
        .check_ok();
    let message = socket1
        .expect("UPDATE_MEMBER", "/member/userId", &user2.id)
        .await;
    assert_eq!(
        message["member"],
        json!({
            "boardId": template.id,
            "userId": user2.id,
            "roles": "",
            "minimumRole": "",
            "schemeAdmin": false,
            "schemeEditor": false,
            "schemeCommenter": false,
            "schemeViewer": true,
            "synthetic": true,
        }),
        "a synthetic viewer"
    );
    let rest = socket1.drain().await;
    assert_eq!(
        with_action(&rest, "UPDATE_MEMBER"),
        Vec::<&Value>::new(),
        "nothing about the member themselves: {rest:?}"
    );

    // A board that is not a template: only the board change is broadcast.
    let board = board_with_admin(
        &th,
        new_board(TEST_TEAM_ID, BoardType::Open, "a board"),
        &user1.id,
    )
    .await;
    socket1.drain().await;
    socket2.drain().await;
    patch_board(&th.client, &board.id, &json!({"type": "P"}))
        .await
        .check_ok();
    socket1.expect("UPDATE_BOARD", "/board/id", &board.id).await;
    let rest = socket1.drain().await;
    assert_eq!(
        with_action(&rest, "DELETE_MEMBER"),
        Vec::<&Value>::new(),
        "no member messages for a board: {rest:?}"
    );
    assert_eq!(
        socket2.drain().await,
        Vec::<Value>::new(),
        "and nothing for the user who is not a member"
    );

    th.teardown().await;
}

/// Duplicating a board tells its new owner about the copy, its blocks and the
/// membership (`server/app/boards.go:209-224`).
#[tokio::test]
async fn duplicate_board_is_broadcast() {
    let th = Setup::native().start().await.init_basic().await;
    let user1 = th.client.me().await;
    let board = board_with_admin(
        &th,
        new_board(TEST_TEAM_ID, BoardType::Open, "Source"),
        &user1.id,
    )
    .await;
    insert_block(&th, view_block(&board.id), &user1.id).await;
    let mut socket = TeamSocket::subscribe(&th, &th.client, TEST_TEAM_ID, &user1.id).await;

    let copy: BoardsAndBlocks = duplicate_board(&th.client, &board.id, "").await.ok_json();

    let copy_id = &copy.boards[0].id;
    socket.expect("UPDATE_BOARD", "/board/id", copy_id).await;
    socket
        .expect("UPDATE_BLOCK", "/block/id", &copy.blocks[0].id)
        .await;
    let message = socket
        .expect("UPDATE_MEMBER", "/member/boardId", copy_id)
        .await;
    assert_eq!(message["member"]["userId"], json!(user1.id), "{message}");
    th.teardown().await;
}
