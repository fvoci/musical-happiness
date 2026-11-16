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
//!   membership into natural membership"` (no synthetic memberships), the "-workspace check
//!   true" cases of `TestGetMembersForBoard` and `TestGetMembersForUser` (nobody
//!   administers the workspace) and `TestGetBoardCount` (boards are not counted any more).
//!
//! Where the original arranges its data through routes of other areas (inserting blocks,
//! sharing a board, creating categories), these tests arrange it through the store or
//! the application layer.

use fvoci_model::{
    Block, BlockType, Category, CategoryContainers, Container, ContainerMember, ContainerPatch,
    ContainerType, MinimumRole, User, Workspace, is_id, new_id,
};
use fvoci_store::PublicContainers;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

use crate::common::ROOT_WS;
use crate::common::TEST_WS;
use crate::common::{
    ApiResponse, Client, ROOT_WORKSPACE_ID, Setup, TEST_WORKSPACE_ID, TestServer, UNKNOWN_ID,
};

// -- helpers --------------------------------------------------------------------------

pub(crate) fn new_board(workspace_id: &str, board_type: ContainerType, title: &str) -> Container {
    Container {
        workspace_id: workspace_id.to_owned(),
        container_type: board_type,
        title: title.to_owned(),
        ..Container::default()
    }
}

/// `th.Server.App().CreateBoard(board, userID, true)`
pub(crate) async fn board_with_admin(
    th: &TestServer,
    board: Container,
    user_id: &str,
) -> Container {
    th.app
        .create_container_with_admin(board, user_id)
        .await
        .expect("the board is created with its creator as admin")
}

/// `th.Server.App().CreateBoard(board, userID, false)`
pub(crate) async fn board_without_members(
    th: &TestServer,
    board: Container,
    user_id: &str,
) -> Container {
    th.app
        .create_container(board, user_id)
        .await
        .expect("the board is created without members")
}

/// Stores one more workspace, next to the two every test server has, and returns its
/// identifier.
pub(crate) async fn another_workspace(th: &TestServer, member: &str) -> String {
    let workspace = Workspace {
        id: new_id(),
        ..Workspace::default()
    };
    let (store, role) = (th.app.store(), fvoci_model::WorkspaceRole::Member);
    store
        .create_workspace(&workspace)
        .await
        .expect("the workspace is stored");
    let let_in = store
        .add_workspace_member(&workspace.id, member, role)
        .await;
    let_in.expect("the user is let into the workspace");
    workspace.id
}

/// Stores a user who never signs in and returns their identifier: for tests of the
/// application layer that need a second user next to [`TestServer::stranger`].
pub(crate) async fn stored_user(th: &TestServer, username: &str) -> String {
    let mut user = User {
        id: new_id(),
        username: username.to_owned(),
        email: format!("{username}@sample.com"),
        ..User::default()
    };
    th.app
        .store()
        .create_user(&mut user)
        .await
        .expect("the user is stored");
    user.id
}

pub(crate) fn member(board_id: &str, user_id: &str) -> ContainerMember {
    ContainerMember {
        container_id: board_id.to_owned(),
        user_id: user_id.to_owned(),
        ..ContainerMember::default()
    }
}

pub(crate) fn editor(board_id: &str, user_id: &str) -> ContainerMember {
    ContainerMember {
        scheme_editor: true,
        ..member(board_id, user_id)
    }
}

/// A membership as a viewer and nothing else.
fn viewer(board_id: &str, user_id: &str) -> ContainerMember {
    ContainerMember {
        scheme_viewer: true,
        ..member(board_id, user_id)
    }
}

/// `th.Server.App().AddMemberToBoard(member)`
pub(crate) async fn add_member(th: &TestServer, member: &ContainerMember) -> ContainerMember {
    th.app
        .add_member_to_container(member)
        .await
        .expect("the member is added")
        .expect("the board exists")
}

/// `th.Server.App().GetMembersForBoard(boardID)`
pub(crate) async fn members_of(th: &TestServer, board_id: &str) -> Vec<ContainerMember> {
    th.app
        .members_for_container(board_id)
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

pub(crate) fn view_block(board_id: &str) -> Block {
    Block {
        id: new_id(),
        container_id: board_id.to_owned(),
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

pub(crate) async fn get_board(client: &Client, board_id: &str) -> ApiResponse {
    client.get(&format!("{TEST_WS}/boards/{board_id}")).await
}

async fn get_members(client: &Client, board_id: &str) -> ApiResponse {
    client
        .get(&format!("{TEST_WS}/boards/{board_id}/members"))
        .await
}

pub(crate) async fn update_member(client: &Client, member: &ContainerMember) -> ApiResponse {
    client
        .put(
            &format!(
                "{TEST_WS}/boards/{}/members/{}",
                member.container_id, member.user_id
            ),
            member,
        )
        .await
}

pub(crate) async fn delete_member(client: &Client, member: &ContainerMember) -> ApiResponse {
    client
        .delete(&format!(
            "{TEST_WS}/boards/{}/members/{}",
            member.container_id, member.user_id
        ))
        .await
}

async fn join_board(client: &Client, board_id: &str) -> ApiResponse {
    client
        .post_empty(&format!("{TEST_WS}/boards/{board_id}/join"))
        .await
}

async fn leave_board(client: &Client, board_id: &str) -> ApiResponse {
    client
        .post_empty(&format!("{TEST_WS}/boards/{board_id}/leave"))
        .await
}

pub(crate) async fn duplicate_board(client: &Client, board_id: &str, query: &str) -> ApiResponse {
    duplicate_board_in(client, TEST_WORKSPACE_ID, board_id, query).await
}

/// `duplicateBoard` of a board of the workspace.
pub(crate) async fn duplicate_board_in(
    client: &Client,
    workspace_id: &str,
    board_id: &str,
    query: &str,
) -> ApiResponse {
    client
        .post_empty(&format!(
            "/w/{workspace_id}/boards/{board_id}/duplicate{query}"
        ))
        .await
}

/// A user's categories in a workspace, each with the identifiers of its boards.
async fn categories_of(
    th: &TestServer,
    user_id: &str,
    workspace_id: &str,
) -> Vec<CategoryContainers> {
    th.app
        .user_category_containers(user_id, workspace_id)
        .await
        .expect("the categories are read")
}

/// The name of the category a user has filed a board under, if any.
pub(crate) async fn category_name_of(
    th: &TestServer,
    user_id: &str,
    workspace_id: &str,
    board_id: &str,
) -> Option<String> {
    categories_of(th, user_id, workspace_id)
        .await
        .into_iter()
        .find(|entry| {
            entry
                .container_metadata
                .iter()
                .any(|metadata| metadata.container_id == board_id)
        })
        .map(|entry| entry.category.name)
}

pub(crate) fn sorted_ids(boards: &[Container]) -> Vec<String> {
    let mut ids: Vec<String> = boards.iter().map(|board| board.id.clone()).collect();
    ids.sort();
    ids
}

// -- TestGetBoards --------------------------------------------------------------------

/// `TestGetBoards/"a non authenticated client should be rejected"`
#[tokio::test]
async fn get_boards_a_non_authenticated_client_should_be_rejected() {
    let th = Setup::native().start().await.init_basic().await;
    board_without_members(
        &th,
        new_board(ROOT_WORKSPACE_ID, ContainerType::Open, ""),
        &th.stranger().await,
    )
    .await;

    let response = th.anonymous().get(&format!("{ROOT_WS}/boards")).await;

    response.check_unauthorized();
    th.teardown().await;
}

/// `TestGetBoards/"should only return the boards that the user is a member of"`
#[tokio::test]
async fn get_boards_should_only_return_the_boards_that_the_user_is_a_member_of() {
    let th = Setup::native().start().await.init_basic().await;
    let workspace_id = ROOT_WORKSPACE_ID;
    let other_workspace_id = TEST_WORKSPACE_ID;
    let user1 = th.me_as_no_admin(&th.client).await;
    let user2 = th.client2.me().await;

    let board1 = board_with_admin(
        &th,
        new_board(workspace_id, ContainerType::Open, "Board 1"),
        &user1.id,
    )
    .await;
    let board2 = board_without_members(
        &th,
        new_board(workspace_id, ContainerType::Open, "Board 2"),
        &user2.id,
    )
    .await;
    let board3 = board_with_admin(
        &th,
        new_board(workspace_id, ContainerType::Private, "Board 3"),
        &user1.id,
    )
    .await;
    board_without_members(
        &th,
        new_board(workspace_id, ContainerType::Private, "Board 4"),
        &user1.id,
    )
    .await;
    board_with_admin(
        &th,
        new_board(workspace_id, ContainerType::Private, "Board 5"),
        &user2.id,
    )
    .await;
    let board6 = board_with_admin(
        &th,
        new_board(other_workspace_id, ContainerType::Open, ""),
        &user1.id,
    )
    .await;

    let mut listed: Vec<Container> = th
        .client
        .get(&format!("/w/{workspace_id}/boards"))
        .await
        .ok_json();
    listed.sort_by(|a, b| a.title.cmp(&b.title));
    assert_eq!(
        listed,
        vec![board1, board2, board3],
        "the user's boards and the open boards of the workspace"
    );

    let from_other_workspace: Vec<Container> = th
        .client
        .get(&format!("/w/{other_workspace_id}/boards"))
        .await
        .ok_json();
    assert_eq!(
        sorted_ids(&from_other_workspace),
        vec![board6.id],
        "only the board of the other workspace"
    );

    th.teardown().await;
}

/// Verified on the original: a workspace without boards answers with an empty list, not
/// `null`. The original answers so for any text in the place of the workspace; here the
/// workspace must exist.
#[tokio::test]
async fn get_boards_of_a_workspace_without_boards_is_an_empty_list() {
    let th = Setup::native().start().await.init_basic().await;

    let response = th.client.get(&format!("{TEST_WS}/boards")).await;

    response.check_ok();
    assert_eq!(response.text(), "[]", "an empty JSON list");

    for no_workspace in [UNKNOWN_ID, "empty-workspace"] {
        th.client
            .get(&format!("/w/{no_workspace}/boards"))
            .await
            .check_error(StatusCode::NOT_FOUND, "workspace not found");
    }
    th.teardown().await;
}

// -- TestCreateBoard ------------------------------------------------------------------

/// `TestCreateBoard/"a non authenticated user should be rejected"`
#[tokio::test]
async fn create_board_a_non_authenticated_user_should_be_rejected() {
    let th = Setup::native().start().await.init_basic().await;

    let response = th
        .anonymous()
        .create_board(&new_board(
            TEST_WORKSPACE_ID,
            ContainerType::Open,
            "board title",
        ))
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

    let board: Container = th
        .client
        .create_board(&new_board(TEST_WORKSPACE_ID, ContainerType::Open, title))
        .await
        .created_json();
    assert!(
        is_id(&board.id),
        "the board got an identifier: {}",
        board.id
    );
    assert_eq!(board.title, title, "the title is kept");
    assert_eq!(
        board.container_type,
        ContainerType::Open,
        "the type is kept"
    );
    assert_eq!(
        board.workspace_id, TEST_WORKSPACE_ID,
        "the workspace is kept"
    );
    assert_eq!(board.created_by, me.id, "created by the session's user");
    assert_eq!(board.modified_by, me.id, "modified by the session's user");

    // "creating a board should make the creator an admin"
    let members = members_of(&th, &board.id).await;
    assert_eq!(members.len(), 1, "one member: {members:?}");
    assert_eq!(members[0].user_id, me.id, "the creator is the member");
    assert_eq!(members[0].container_id, board.id, "of this board");
    assert!(members[0].scheme_admin, "as an admin");

    // "creator should be able to access the public board and its blocks"
    let read: Container = get_board(&th.client, &board.id).await.ok_json();
    assert_eq!(read, board, "the creator reads the board back");

    // "A non-member user should be able to access the public board but not its blocks"
    let read: Container = get_board(&th.client2, &board.id).await.ok_json();
    assert_eq!(
        read, board,
        "another user of the workspace reads an open board"
    );

    th.teardown().await;
}

/// The assertions of `TestCreateBoard/"create public board"` about the board's blocks.
#[tokio::test]
async fn create_board_create_public_board_and_its_blocks() {
    let th = Setup::native().start().await.init_basic().await;
    let board: Container = th
        .client
        .create_board(&new_board(
            TEST_WORKSPACE_ID,
            ContainerType::Open,
            "board title 1",
        ))
        .await
        .created_json();

    // "creator should be able to access the public board and its blocks"
    let blocks = th
        .client
        .get(&format!("{TEST_WS}/boards/{}/blocks", board.id))
        .await;
    assert_eq!(blocks.ok_json::<Vec<Block>>(), vec![], "no blocks yet");

    // "A non-member user should be able to access the public board but not its blocks"
    th.client2
        .get(&format!("{TEST_WS}/boards/{}/blocks", board.id))
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

    let board: Container = th
        .client
        .create_board(&new_board(TEST_WORKSPACE_ID, ContainerType::Private, title))
        .await
        .created_json();
    assert!(
        is_id(&board.id),
        "the board got an identifier: {}",
        board.id
    );
    assert_eq!(board.title, title, "the title is kept");
    assert_eq!(
        board.container_type,
        ContainerType::Private,
        "the type is kept"
    );
    assert_eq!(
        board.workspace_id, TEST_WORKSPACE_ID,
        "the workspace is kept"
    );
    assert_eq!(board.created_by, me.id, "created by the session's user");
    assert_eq!(board.modified_by, me.id, "modified by the session's user");

    // "creating a board should make the creator an admin"
    let members = members_of(&th, &board.id).await;
    assert_eq!(members.len(), 1, "one member: {members:?}");
    assert_eq!(members[0].user_id, me.id, "the creator is the member");
    assert_eq!(members[0].container_id, board.id, "of this board");
    assert!(members[0].scheme_admin, "as an admin");

    // "creator should be able to access the private board and its blocks"
    let read: Container = get_board(&th.client, &board.id).await.ok_json();
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
    let board: Container = th
        .client
        .create_board(&new_board(
            TEST_WORKSPACE_ID,
            ContainerType::Private,
            "private",
        ))
        .await
        .created_json();

    // "creator should be able to access the private board and its blocks"
    let blocks = th
        .client
        .get(&format!("{TEST_WS}/boards/{}/blocks", board.id))
        .await;
    assert_eq!(blocks.ok_json::<Vec<Block>>(), vec![], "no blocks yet");

    // "unauthorized user should not be able to access the private board or its blocks":
    // for them it does not exist.
    th.client2
        .get(&format!("{TEST_WS}/boards/{}/blocks", board.id))
        .await
        .check_error(StatusCode::NOT_FOUND, "board not found");

    th.teardown().await;
}

async fn assert_no_boards(th: &TestServer, user_id: &str) {
    let boards = th
        .app
        .containers_for_user_and_workspace(user_id, TEST_WORKSPACE_ID, PublicContainers::Include)
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
            &format!("{TEST_WS}/boards"),
            &json!({"title": "invalid board title", "workspaceId": TEST_WORKSPACE_ID, "type": "invalid"}),
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
        json!({"title": "invalid board title", "workspaceId": TEST_WORKSPACE_ID, "type": ""}),
        json!({"title": "invalid board title", "workspaceId": TEST_WORKSPACE_ID}),
        json!({"title": "invalid board title", "workspaceId": TEST_WORKSPACE_ID, "type": null}),
    ] {
        let response = th.client.post(&format!("{TEST_WS}/boards"), &body).await;
        response.check_bad_request();
        assert_eq!(response.error_message(), "invalid-board-type", "{body}");
    }

    assert_no_boards(&th, &user1.id).await;
    th.teardown().await;
}

/// `TestCreateBoard/"create invalid board"/"no team ID"`. The path names the workspace:
/// a body that names another, one that does not exist and text that is no identifier
/// among them, is a bad request; one that names none makes the board in the path's.
#[tokio::test]
async fn create_board_create_invalid_board_no_workspace_id() {
    let th = Setup::native().start().await.init_basic().await;
    let user1 = th.client.me().await;

    for body in [
        json!({"title": "x", "workspaceId": ROOT_WORKSPACE_ID, "type": "O"}),
        json!({"title": "x", "workspaceId": UNKNOWN_ID, "type": "O"}),
        json!({"title": "x", "workspaceId": "nope", "type": "O"}),
    ] {
        let response = th.client.post(&format!("{TEST_WS}/boards"), &body).await;
        response.check_error(
            StatusCode::BAD_REQUEST,
            "the board is made in the workspace of the path",
        );
        assert_eq!(
            response.refused_at(),
            "/workspaceId",
            "the member at fault: {body}"
        );
    }
    assert_no_boards(&th, &user1.id).await;

    let made: Container = th
        .client
        .post(
            &format!("{TEST_WS}/boards"),
            &json!({"title": "x", "type": "O"}),
        )
        .await
        .created_json();
    assert_eq!(made.workspace_id, TEST_WORKSPACE_ID, "the path's workspace");
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
        "{\"workspaceId\":",
        "",
        "{\"workspaceId\":\"t\",\"type\":5}",
        "[]",
        "5",
        // (`null` crashes the original's handler, which its recovery turns into a 500.)
        "null",
    ] {
        let response = th
            .client
            .send_raw_body(Method::POST, &format!("{TEST_WS}/boards"), body)
            .await;
        response.check_bad_request();
        assert_eq!(response.problem_type(), "/problems/bad-request", "{body}");
    }

    for role in ["boss", "", "admin"] {
        let response = th
            .client
            .post(
                &format!("{TEST_WS}/boards"),
                &json!({"workspaceId": TEST_WORKSPACE_ID, "type": "O", "minimumRole": role}),
            )
            .await;
        response.check_bad_request();
        assert_eq!(
            response.error_message(),
            "invalid-board-minimum-role",
            "the minimum role {role:?}"
        );
    }

    // The type is checked before the minimum role.
    let response = th
        .client
        .post(
            &format!("{TEST_WS}/boards"),
            &json!({"workspaceId": TEST_WORKSPACE_ID, "type": "X", "minimumRole": "boss"}),
        )
        .await;
    response.check_bad_request();
    assert_eq!(response.error_message(), "invalid-board-type", "type first");

    // A new board does not choose its identifier. (The original answers 500.)
    let response = th
        .client
        .post(
            &format!("{TEST_WS}/boards"),
            &json!({"id": "my-id", "workspaceId": TEST_WORKSPACE_ID, "type": "O"}),
        )
        .await;
    response.check_error(StatusCode::BAD_REQUEST, "new board cannot have an ID");
    assert_eq!(response.refused_at(), "/id", "the member at fault");

    assert_no_boards(&th, &user1.id).await;
    th.teardown().await;
}

/// The wire format of a board, compared with what the original answers. The original
/// sends `null` for `properties` and `cardProperties` that were never set; the port sends
/// empty values (a deliberate difference of the model). It has no `channelId`, and says
/// whether the board is a built-in template and of which kind it is.
#[tokio::test]
async fn create_board_wire_format() {
    let th = Setup::native().start().await.init_basic().await;
    let me = th.client.me().await;

    let response = th
        .client
        .post(
            &format!("{TEST_WS}/boards"),
            &json!({"workspaceId": TEST_WORKSPACE_ID, "type": "O", "title": "open one"}),
        )
        .await;
    let value = response.value();
    response.check_created();

    assert_eq!(
        value,
        json!({
            "id": value["id"],
            "workspaceId": TEST_WORKSPACE_ID,
            "createdBy": me.id,
            "modifiedBy": me.id,
            "type": "O", "kind": "board",
            "minimumRole": "editor",
            "title": "open one",
            "description": "",
            "icon": "",
            "showDescription": false,
            "isTemplate": false,
            "isBuiltin": false,
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

    let board: Container = th
        .client
        .create_board(&new_board(TEST_WORKSPACE_ID, ContainerType::Open, "filed"))
        .await
        .created_json();
    let template: Container = th
        .client
        .create_board(&Container {
            is_template: true,
            ..new_board(TEST_WORKSPACE_ID, ContainerType::Open, "a template")
        })
        .await
        .created_json();

    assert_eq!(
        category_name_of(&th, &me.id, TEST_WORKSPACE_ID, &board.id).await,
        Some("Boards".to_owned()),
        "the board is in the default category"
    );
    assert_eq!(
        category_name_of(&th, &me.id, TEST_WORKSPACE_ID, &template.id).await,
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

    let board: Container = th
        .client
        .create_board(&Container {
            is_template: true,
            ..new_board(TEST_WORKSPACE_ID, ContainerType::Open, title)
        })
        .await
        .created_json();
    assert!(
        is_id(&board.id),
        "the board got an identifier: {}",
        board.id
    );
    assert_eq!(board.title, title, "the title is kept");
    assert_eq!(
        board.container_type,
        ContainerType::Open,
        "the type is kept"
    );
    assert_eq!(
        board.workspace_id, TEST_WORKSPACE_ID,
        "the workspace is kept"
    );
    assert_eq!(board.created_by, me.id, "created by the session's user");
    assert_eq!(board.modified_by, me.id, "modified by the session's user");
    assert!(board.is_template, "a template");

    // "creating a board template should make the creator an admin"
    let members = members_of(&th, &board.id).await;
    assert_eq!(members.len(), 1, "one member: {members:?}");
    assert_eq!(members[0].user_id, me.id, "the creator is the member");
    assert_eq!(members[0].container_id, board.id, "of this board");
    assert!(members[0].scheme_admin, "as an admin");

    // "creator should be able to access the public board template and its blocks"
    let read: Container = get_board(&th.client, &board.id).await.ok_json();
    assert_eq!(read, board, "the creator reads the template back");

    // "another user should be able to access the public board template and its blocks"
    let read: Container = get_board(&th.client2, &board.id).await.ok_json();
    assert_eq!(read, board, "another user reads an open template");

    th.teardown().await;
}

/// The assertions of `TestCreateBoardTemplate/"create public board template"` about the
/// template's blocks.
#[tokio::test]
async fn create_board_template_create_public_board_template_and_its_blocks() {
    let th = Setup::native().start().await.init_basic().await;
    let board: Container = th
        .client
        .create_board(&Container {
            is_template: true,
            ..new_board(TEST_WORKSPACE_ID, ContainerType::Open, "board template 1")
        })
        .await
        .created_json();

    // "creator should be able to access the public board template and its blocks"
    th.client
        .get(&format!("{TEST_WS}/boards/{}/blocks", board.id))
        .await
        .check_ok();

    // "another user should be able to access the public board template and its blocks"
    th.client2
        .get(&format!("{TEST_WS}/boards/{}/blocks", board.id))
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

    let board: Container = th
        .client
        .create_board(&Container {
            is_template: true,
            ..new_board(TEST_WORKSPACE_ID, ContainerType::Private, title)
        })
        .await
        .created_json();
    assert!(
        is_id(&board.id),
        "the board got an identifier: {}",
        board.id
    );
    assert_eq!(board.title, title, "the title is kept");
    assert_eq!(
        board.container_type,
        ContainerType::Private,
        "the type is kept"
    );
    assert_eq!(
        board.workspace_id, TEST_WORKSPACE_ID,
        "the workspace is kept"
    );
    assert_eq!(board.created_by, me.id, "created by the session's user");
    assert_eq!(board.modified_by, me.id, "modified by the session's user");

    // "creating a board template should make the creator an admin"
    let members = members_of(&th, &board.id).await;
    assert_eq!(members.len(), 1, "one member: {members:?}");
    assert_eq!(members[0].user_id, me.id, "the creator is the member");
    assert_eq!(members[0].container_id, board.id, "of this board");
    assert!(members[0].scheme_admin, "as an admin");

    // "creator should be able to access the private board template and its blocks"
    let read: Container = get_board(&th.client, &board.id).await.ok_json();
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
    let board: Container = th
        .client
        .create_board(&Container {
            is_template: true,
            ..new_board(
                TEST_WORKSPACE_ID,
                ContainerType::Private,
                "private template",
            )
        })
        .await
        .created_json();

    // "creator should be able to access the private board template and its blocks"
    th.client
        .get(&format!("{TEST_WS}/boards/{}/blocks", board.id))
        .await
        .check_ok();

    // "unauthorized user should not be able to access the private board template or its
    // blocks": for them it does not exist.
    th.client2
        .get(&format!("{TEST_WS}/boards/{}/blocks", board.id))
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
        "no session token",
        "the session check's refusal: the board's routes need a session"
    );
    th.teardown().await;
}

/// `TestGetBoard/"valid read token should be enough to get the board"`
#[tokio::test]
async fn get_board_valid_read_token_should_be_enough_to_get_the_board() {
    let th = Setup::local_mode().start().await.init_basic().await;
    let user1 = th.client.me().await;
    let board = board_with_admin(
        &th,
        new_board(
            TEST_WORKSPACE_ID,
            ContainerType::Open,
            "public board where user1 is admin",
        ),
        &user1.id,
    )
    .await;
    let sharing_token = th.share(&board.id).await;

    // Without a session the board cannot be read...
    let anonymous = th.anonymous();
    get_board(&anonymous, &board.id).await.check_unauthorized();
    anonymous
        .get(&format!("/shared/{}?readToken=wrong", board.id))
        .await
        .check_not_found();

    // ...but it can by its link. Who reads by the link is not told who made the board,
    // nor its workspace.
    let shared_path = format!("/shared/{}?readToken={sharing_token}", board.id);
    let read: Container = anonymous.get(&shared_path).await.ok_json();
    assert_eq!(
        read,
        Container {
            workspace_id: String::new(),
            created_by: String::new(),
            modified_by: String::new(),
            ..board.clone()
        },
        "the shared board, without its workspace, creator and modifier"
    );

    // Whoever is signed in reads by the link as its reader, a member of the board too;
    // without the link, the member reads as a member.
    for client in [&th.client, &th.client2] {
        let read: Container = client.get(&shared_path).await.ok_json();
        assert_eq!(
            (read.created_by.as_str(), read.modified_by.as_str()),
            ("", ""),
            "by the link, as its reader"
        );
    }
    let read: Container = th
        .client2
        .get(&format!("{TEST_WS}/boards/{}", board.id))
        .await
        .ok_json();
    assert_eq!(
        read.created_by, user1.id,
        "without it, as a user of the workspace"
    );

    th.teardown().await;
}

/// `TestGetBoard/"nonexisting board"`
#[tokio::test]
async fn get_board_nonexisting_board() {
    let th = Setup::native().start().await.init_basic().await;

    for board_id in [UNKNOWN_ID, "nonexistent board"] {
        let response = get_board(&th.client, board_id).await;
        response.check_error(StatusCode::NOT_FOUND, "board not found");
    }
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
        new_board(TEST_WORKSPACE_ID, ContainerType::Private, ""),
        &user1.id,
    )
    .await;

    let response = get_board(&th.client, &board.id).await;

    // The answer is the one for a board that does not exist. (The original answers 403.)
    response.check_error(StatusCode::NOT_FOUND, "board not found");
    for board_id in [UNKNOWN_ID, "nonexistent board"] {
        let missing = get_board(&th.client, board_id).await;
        assert_eq!(response.status, missing.status, "the same status");
        assert_eq!(
            response.error_message(),
            missing.error_message(),
            "and the same message"
        );
    }
    th.teardown().await;
}

/// `TestGetBoard/"a user that has permissions to a private board can retrieve it"`
#[tokio::test]
async fn get_board_a_user_with_permissions_to_a_private_board_can_retrieve_it() {
    let th = Setup::native().start().await.init_basic().await;
    let user1 = th.client.me().await;
    let board = board_with_admin(
        &th,
        new_board(TEST_WORKSPACE_ID, ContainerType::Private, ""),
        &user1.id,
    )
    .await;

    let read: Container = get_board(&th.client, &board.id).await.ok_json();

    assert_eq!(read, board, "the member reads the board");
    th.teardown().await;
}

/// `TestGetBoard/"a user that doesn't have permissions to a public board but have them
/// to its workspace can retrieve it"`
#[tokio::test]
async fn get_board_a_user_with_permissions_to_the_workspace_can_retrieve_a_public_board() {
    let th = Setup::native().start().await.init_basic().await;
    let user1 = th.client.me().await;
    let board = board_without_members(
        &th,
        new_board(TEST_WORKSPACE_ID, ContainerType::Open, "title"),
        &user1.id,
    )
    .await;

    let read: Container = get_board(&th.client, &board.id).await.ok_json();

    assert_eq!(read, board, "a user of the workspace reads an open board");
    th.teardown().await;
}

// -- TestGetBoardMetadata -------------------------------------------------------------
//
// The original's metadata route needs a license a standalone server never has, and
// answered 501. The route is gone; `admin::routes_of_the_host_application_do_not_exist`
// checks that its path answers like any other that does not exist.

// -- TestPatchBoard -------------------------------------------------------------------

fn title_patch(title: &str) -> ContainerPatch {
    ContainerPatch {
        title: Some(title.to_owned()),
        ..ContainerPatch::default()
    }
}

pub(crate) async fn patch_board(
    client: &Client,
    board_id: &str,
    patch: &impl serde::Serialize,
) -> ApiResponse {
    client
        .patch(&format!("{TEST_WS}/boards/{board_id}"), patch)
        .await
}

async fn stored_board(th: &TestServer, board_id: &str) -> Container {
    th.app.container(board_id).await.expect("the board exists")
}

/// `TestPatchBoard/"a non authenticated user should be rejected"`
#[tokio::test]
async fn patch_board_a_non_authenticated_user_should_be_rejected() {
    let th = Setup::native().start().await.init_basic().await;
    let initial_title = "title 1";
    let board = board_without_members(
        &th,
        new_board(TEST_WORKSPACE_ID, ContainerType::Open, initial_title),
        &th.stranger().await,
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

    for board_id in [UNKNOWN_ID, "non-existing-board"] {
        let response = patch_board(&th.client, board_id, &title_patch("a new title 2")).await;
        response.check_error(StatusCode::NOT_FOUND, "board not found");
    }
    th.teardown().await;
}

/// `TestPatchBoard/"invalid patch on a board with permissions"`
#[tokio::test]
async fn patch_board_invalid_patch_on_a_board_with_permissions() {
    let th = Setup::native().start().await.init_basic().await;
    let user1 = th.client.me().await;
    let board = board_with_admin(
        &th,
        new_board(TEST_WORKSPACE_ID, ContainerType::Open, "title"),
        &user1.id,
    )
    .await;

    let response = patch_board(&th.client, &board.id, &json!({"type": "invalid"})).await;
    response.check_bad_request();
    assert_eq!(response.error_message(), "invalid-board-type", "the type");

    // Empty is no minimum role, and neither is admin: nobody becomes an administrator of
    // a board by joining it.
    for role in ["boss", "", "admin", "Editor"] {
        let response = patch_board(&th.client, &board.id, &json!({"minimumRole": role})).await;
        response.check_bad_request();
        assert_eq!(
            response.error_message(),
            "invalid-board-minimum-role",
            "the minimum role {role:?}"
        );
    }
    assert_eq!(
        stored_board(&th, &board.id).await.minimum_role,
        MinimumRole::Editor,
        "the board keeps the minimum role it started with"
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
        new_board(TEST_WORKSPACE_ID, ContainerType::Open, "title"),
        &user1.id,
    )
    .await;
    let new_title = "a new title";

    let patched: Container = patch_board(&th.client, &board.id, &title_patch(new_title))
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
        new_board(TEST_WORKSPACE_ID, ContainerType::Open, initial_title),
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
/// type and minimum role; each refusal has its own message.
#[tokio::test]
async fn patch_board_permissions_by_what_the_patch_touches() {
    let th = Setup::native().start().await.init_basic().await;
    let user1 = th.client.me().await;
    let user2 = th.client2.me().await;
    let board = board_with_admin(
        &th,
        new_board(TEST_WORKSPACE_ID, ContainerType::Open, "title"),
        &user1.id,
    )
    .await;
    add_member(&th, &editor(&board.id, &user2.id)).await;

    let patched: Container = patch_board(&th.client2, &board.id, &title_patch("by the editor"))
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
    ] {
        let response = patch_board(&th.client2, &board.id, &patch).await;
        response.check_forbidden();
        assert_eq!(response.error_message(), message, "{patch}");
    }

    // The admin may do all of it.
    let patched: Container = patch_board(
        &th.client,
        &board.id,
        &json!({"type": "P", "minimumRole": "viewer", "channelId": "channel"}),
    )
    .await
    .ok_json();
    assert_eq!(patched.container_type, ContainerType::Private, "the type");
    assert_eq!(
        patched.minimum_role,
        MinimumRole::Viewer,
        "the minimum role"
    );
    // Channels belonged to the original's host application. The member means nothing
    // any more: it is neither refused nor stored nor answered.
    let answered: Value = get_board(&th.client, &board.id).await.value();
    assert!(answered.get("channelId").is_none(), "{answered}");

    // The minimum role is the board's: a membership does not repeat it.
    let members: Value = get_members(&th.client, &board.id).await.value();
    assert!(
        members.as_array().is_some_and(|members| members
            .iter()
            .all(|member| member.get("minimumRole").is_none())),
        "{members}"
    );

    // Malformed bodies are bad requests, `null` too (which crashes the original's
    // handler).
    for body in ["{bad", "", "[]", "null", "{\"type\":5}"] {
        th.client
            .send_raw_body(
                Method::PATCH,
                &format!("{TEST_WS}/boards/{}", board.id),
                body,
            )
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
        new_board(TEST_WORKSPACE_ID, ContainerType::Open, "title"),
        &th.stranger().await,
    )
    .await;

    let response = th
        .anonymous()
        .delete(&format!("{TEST_WS}/boards/{}", board.id))
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
        new_board(TEST_WORKSPACE_ID, ContainerType::Open, "title"),
        &th.stranger().await,
    )
    .await;

    let response = th
        .client
        .delete(&format!("{TEST_WS}/boards/{}", board.id))
        .await;

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

    for board_id in [UNKNOWN_ID, "non-existing-board"] {
        let response = th
            .client
            .delete(&format!("{TEST_WS}/boards/{board_id}"))
            .await;
        response.check_error(StatusCode::NOT_FOUND, "board not found");
    }
    th.teardown().await;
}

/// `TestDeleteBoard/"an existing board should be correctly deleted"`
#[tokio::test]
async fn delete_board_an_existing_board_should_be_correctly_deleted() {
    let th = Setup::native().start().await.init_basic().await;
    let user1 = th.client.me().await;
    let board = board_with_admin(
        &th,
        new_board(TEST_WORKSPACE_ID, ContainerType::Open, "title"),
        &user1.id,
    )
    .await;

    let response = th
        .client
        .delete(&format!("{TEST_WS}/boards/{}", board.id))
        .await;

    response.check_no_content();
    let error = th
        .app
        .container(&board.id)
        .await
        .expect_err("the board is gone");
    assert!(error.is_not_found(), "not found: {error:?}");

    // Verified on the original: deleting it again is a 404.
    th.client
        .delete(&format!("{TEST_WS}/boards/{}", board.id))
        .await
        .check_not_found();
    th.teardown().await;
}

// -- TestUndeleteBoard ----------------------------------------------------------------

pub(crate) async fn undelete_board(client: &Client, board_id: &str) -> ApiResponse {
    client
        .post_empty(&format!("{TEST_WS}/boards/{board_id}/undelete"))
        .await
}

async fn assert_board_is_gone(th: &TestServer, board_id: &str) {
    let error = th
        .app
        .container(board_id)
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
        new_board(TEST_WORKSPACE_ID, ContainerType::Open, "title"),
        &th.stranger().await,
    )
    .await;
    th.app
        .delete_container(&board.id, &th.stranger().await)
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
        new_board(TEST_WORKSPACE_ID, ContainerType::Open, "title"),
        &th.stranger().await,
    )
    .await;
    th.app
        .delete_container(&board.id, &th.stranger().await)
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
    let stranger = th.stranger().await;
    let board = board_without_members(
        &th,
        new_board(TEST_WORKSPACE_ID, ContainerType::Open, "title"),
        &stranger,
    )
    .await;
    add_member(&th, &editor(&board.id, &stranger)).await;
    add_member(&th, &editor(&board.id, &user1.id)).await;
    th.app
        .delete_container(&board.id, &stranger)
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

    for board_id in [UNKNOWN_ID, "non-existing-board"] {
        let response = undelete_board(&th.client, board_id).await;
        response.check_error(StatusCode::NOT_FOUND, "board not found");
    }
    th.teardown().await;
}

/// `TestUndeleteBoard/"an existing deleted board should be correctly undeleted"`
#[tokio::test]
async fn undelete_board_an_existing_deleted_board_should_be_correctly_undeleted() {
    let th = Setup::native().start().await.init_basic().await;
    let user1 = th.client.me().await;
    let board = board_with_admin(
        &th,
        new_board(TEST_WORKSPACE_ID, ContainerType::Open, "title"),
        &user1.id,
    )
    .await;

    // A board that is not deleted is not in the trash: there is nothing to restore.
    undelete_board(&th.client, &board.id)
        .await
        .check_error(StatusCode::NOT_FOUND, "board not found");

    th.app
        .delete_container(&board.id, &th.stranger().await)
        .await
        .expect("the board is deleted");
    assert_board_is_gone(&th, &board.id).await;

    let response = undelete_board(&th.client, &board.id).await;

    let answered: Container = response.ok_json();
    assert_eq!(answered.id, board.id, "the answer is the board");
    assert_eq!(answered.delete_at, 0, "as it is after the restoring");
    let restored = stored_board(&th, &board.id).await;
    assert_eq!(restored, answered, "the board is back, as answered");
    assert_eq!(restored.title, board.title, "with the title it had");
    th.teardown().await;
}

// -- TestGetMembersForBoard -----------------------------------------------------------

/// `createBoardWithUsers`: an open board with user1 as admin and user2 as editor.
async fn board_with_users(th: &TestServer) -> Container {
    let user1 = th.client.me().await;
    let user2 = th.client2.me().await;
    let board = board_with_admin(
        th,
        new_board(TEST_WORKSPACE_ID, ContainerType::Open, "title"),
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
        .delete_container_member(&board.id, &user2.id)
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

    for board_id in [UNKNOWN_ID, "non-existing-board"] {
        let response = get_members(&th.client, board_id).await;
        response.check_error(StatusCode::NOT_FOUND, "board not found");
    }
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
    let mut members: Vec<ContainerMember> = response.ok_json();
    members.sort_by_key(|member| member.user_id != user1.id);

    assert_eq!(
        members,
        vec![
            ContainerMember {
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
            "schemeAdmin": false,
            "schemeEditor": true,
            "schemeCommenter": false,
            "schemeViewer": false,
        }),
        "the members of a membership on the wire"
    );
    th.teardown().await;
}

/// The "-base case" and "-team check false" cases of `TestGetMembersForUser` in
/// `server/app/boards_test.go`, through `GET /users/me/memberships`: memberships are
/// answered as stored. They go to the trash with their board and come back with it.
#[tokio::test]
async fn get_members_for_user_returns_the_memberships_as_stored() {
    let th = Setup::native().start().await.init_basic().await;
    let user2 = th.client2.me().await;
    let board = board_with_users(&th).await;

    let memberships: Vec<ContainerMember> = th.client2.get("/users/me/memberships").await.ok_json();
    assert_eq!(
        memberships,
        vec![editor(&board.id, &user2.id)],
        "an editor, not an admin"
    );

    th.app
        .delete_container(&board.id, &user2.id)
        .await
        .expect("the board is deleted");
    let memberships: Vec<ContainerMember> = th.client2.get("/users/me/memberships").await.ok_json();
    assert_eq!(memberships, [], "a deleted board has no members");

    th.app
        .undelete_container(&board.id, &user2.id)
        .await
        .expect("the board is restored");
    let memberships: Vec<ContainerMember> = th.client2.get("/users/me/memberships").await.ok_json();
    assert_eq!(
        memberships,
        vec![editor(&board.id, &user2.id)],
        "the membership is back with the board, as it was"
    );

    th.teardown().await;
}

// -- TestAddMember --------------------------------------------------------------------

/// `TestAddMember/"a non authenticated user should be rejected"`
#[tokio::test]
async fn add_member_a_non_authenticated_user_should_be_rejected() {
    let th = Setup::native().start().await.init_basic().await;
    let user1 = th.client.me().await;
    let board = board_without_members(
        &th,
        new_board(TEST_WORKSPACE_ID, ContainerType::Open, "title"),
        &th.stranger().await,
    )
    .await;

    let response = th
        .anonymous()
        .add_member(&editor(&board.id, &user1.id))
        .await;

    response.check_unauthorized();
    th.teardown().await;
}

/// `TestAddMember/"a user without permissions should be rejected"`
#[tokio::test]
async fn add_member_a_user_without_permissions_should_be_rejected() {
    let th = Setup::native().start().await.init_basic().await;
    let user1 = th.client.me().await;
    let board = board_without_members(
        &th,
        new_board(TEST_WORKSPACE_ID, ContainerType::Private, "title"),
        &th.stranger().await,
    )
    .await;

    let response = th.client.add_member(&editor(&board.id, &user1.id)).await;

    // A private board the user is no member of does not exist for them.
    response.check_error(StatusCode::NOT_FOUND, "board not found");
    th.teardown().await;
}

/// `TestAddMember/"non existing board"`
#[tokio::test]
async fn add_member_non_existing_board() {
    let th = Setup::native().start().await.init_basic().await;
    let user1 = th.client.me().await;

    for board_id in [UNKNOWN_ID, "non-existing-board-id"] {
        let response = th.client.add_member(&editor(board_id, &user1.id)).await;
        response.check_error(StatusCode::NOT_FOUND, "board not found");
    }
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
        new_board(TEST_WORKSPACE_ID, ContainerType::Private, "title"),
        &user1.id,
    )
    .await;
    let new_member = editor(&board.id, &user2.id);

    let added: ContainerMember = th.client.add_member(&new_member).await.ok_json();

    assert_eq!(added, new_member, "the membership as asked for");
    assert_eq!(
        category_name_of(&th, &user2.id, TEST_WORKSPACE_ID, &board.id).await,
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
        new_board(TEST_WORKSPACE_ID, ContainerType::Open, "title"),
        &user1.id,
    )
    .await;
    let new_member = editor(&board.id, &user2.id);

    th.client2.add_member(&new_member).await.check_forbidden();
    get_members(&th.client2, &board.id).await.check_forbidden();

    // Join board - will become an editor
    let joined: ContainerMember = join_board(&th.client2, &board.id).await.ok_json();
    assert_eq!(joined, new_member, "joined as an editor");

    let added: ContainerMember = th.client2.add_member(&new_member).await.ok_json();
    assert_eq!(added, new_member, "the existing membership is answered");

    let members: Vec<ContainerMember> = get_members(&th.client2, &board.id).await.ok_json();
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
        new_board(TEST_WORKSPACE_ID, ContainerType::Private, "title"),
        &user1.id,
    )
    .await;
    let new_member = ContainerMember {
        scheme_commenter: true,
        ..member(&board.id, &user2.id)
    };

    let added: ContainerMember = th.client.add_member(&new_member).await.ok_json();

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
        new_board(TEST_WORKSPACE_ID, ContainerType::Private, "title"),
        &user1.id,
    )
    .await;
    let members = members_of(&th, &board.id).await;
    assert_eq!(members.len(), 1, "one member: {members:?}");
    assert!(
        members[0].scheme_admin && members[0].scheme_editor,
        "an admin and editor"
    );

    let answered: ContainerMember = th
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
        new_board(TEST_WORKSPACE_ID, ContainerType::Open, "title"),
        &user1.id,
    )
    .await;
    let path = format!("{TEST_WS}/boards/{}/members", board.id);

    for body in ["{bad", "[]", "null"] {
        th.client
            .send_raw_body(Method::POST, &path, body)
            .await
            .check_bad_request();
    }
    let response = th.client.post(&path, &json!({"schemeEditor": true})).await;
    response.check_bad_request();
    assert_eq!(response.error_message(), "empty userId", "no user named");

    // Only the user and the four role flags are taken from the body; the board comes from
    // the path.
    let added: ContainerMember = th
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
        ContainerMember {
            scheme_commenter: true,
            ..member(&board.id, &user2.id)
        },
        "the rest of the body is ignored"
    );

    // The user must exist. (The original stores a membership of nobody.)
    for no_user in [UNKNOWN_ID, "no-such-user"] {
        let response = th
            .client
            .post(&path, &json!({"userId": no_user, "schemeEditor": true}))
            .await;
        response.check_error(StatusCode::BAD_REQUEST, "no such user");
        assert_eq!(
            response.refused_at(),
            "/userId",
            "the member at fault: {no_user}"
        );
    }

    // A membership without any role is stored as such.
    let user3 = th.add_user("user3").await.me().await;
    let added: ContainerMember = th
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
        new_board(TEST_WORKSPACE_ID, ContainerType::Open, "victim"),
        &user1.id,
    )
    .await;

    // user2 joins the open board, which makes them an editor.
    join_board(&th.client2, &board.id).await.check_ok();

    // The attack: the editor adds an accomplice as admin.
    let response = th
        .client2
        .add_member(&ContainerMember {
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
        .delete(&format!("{TEST_WS}/boards/{}", board.id))
        .await
        .check_forbidden();
    delete_member(&client3, &member(&board.id, &user1.id))
        .await
        .check_forbidden();

    // Naming an existing member with the admin flag is refused as well, not answered
    // with that membership.
    th.client2
        .add_member(&ContainerMember {
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
        new_board(TEST_WORKSPACE_ID, ContainerType::Open, "open"),
        &user1.id,
    )
    .await;
    join_board(&th.client2, &board.id).await.check_ok();

    let response = th
        .client2
        .add_member(&ContainerMember {
            scheme_viewer: true,
            ..member(&board.id, &user3.id)
        })
        .await;
    let added: ContainerMember = response.ok_json();
    assert_eq!(added.user_id, user3.id, "added: {added:?}");

    // An admin still adds admins.
    let client4 = th.add_user("user4").await;
    let user4 = client4.me().await;
    let added: ContainerMember = th
        .client
        .add_member(&ContainerMember {
            scheme_admin: true,
            ..member(&board.id, &user4.id)
        })
        .await
        .ok_json();
    assert!(added.scheme_admin, "added as an admin: {added:?}");

    th.teardown().await;
}

/// The role flags of a member, in the order admin, editor, commenter, viewer.
fn role_flags(member: &ContainerMember) -> [bool; 4] {
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
/// a member without any role. Such an adder gives the board's minimum role, which every
/// board has: editor unless the board says otherwise. The answer and the stored
/// membership say what was given.
#[tokio::test]
async fn add_member_without_managing_roles_gives_the_minimum_role_of_the_board() {
    let th = Setup::native().start().await.init_basic().await;
    let user1 = th.client.me().await;
    let editor_request = |board_id: &str, user_id: &str| ContainerMember {
        scheme_editor: true,
        scheme_commenter: true,
        scheme_viewer: true,
        ..member(board_id, user_id)
    };

    // The empty one is the board that says nothing about its minimum role.
    let cases = [
        ("", ONLY_EDITOR),
        ("viewer", ONLY_VIEWER),
        ("commenter", ONLY_COMMENTER),
        ("editor", ONLY_EDITOR),
    ];
    for (index, (minimum_role, expected)) in cases.into_iter().enumerate() {
        let board = board_with_admin(
            &th,
            new_board(TEST_WORKSPACE_ID, ContainerType::Open, "open"),
            &user1.id,
        )
        .await;
        // The adder is an editor of the board whatever its minimum role.
        add_member(&th, &editor(&board.id, &th.client2.me().await.id)).await;
        if !minimum_role.is_empty() {
            th.client
                .patch(
                    &format!("{TEST_WS}/boards/{}", board.id),
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
            let added: ContainerMember = th.client2.add_member(&request).await.ok_json();
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
                insert.check_created();
            } else {
                insert.check_forbidden();
            }
        }
    }
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
        new_board(TEST_WORKSPACE_ID, ContainerType::Open, "open"),
        &user1.id,
    )
    .await;
    th.client
        .patch(
            &format!("{TEST_WS}/boards/{}", board.id),
            &json!({"minimumRole": "commenter"}),
        )
        .await
        .check_ok();

    let user3 = th.add_user("user3").await.me().await;
    let added: ContainerMember = th
        .client
        .add_member(&editor(&board.id, &user3.id))
        .await
        .ok_json();
    assert_eq!(role_flags(&added), ONLY_EDITOR, "above the minimum role");

    let user4 = th.add_user("user4").await.me().await;
    let added: ContainerMember = th
        .client
        .add_member(&ContainerMember {
            scheme_viewer: true,
            ..member(&board.id, &user4.id)
        })
        .await
        .ok_json();
    assert_eq!(role_flags(&added), ONLY_VIEWER, "below the minimum role");

    // Preserved quirk: a request without a role makes a member without one.
    let user5 = th.add_user("user5").await.me().await;
    let added: ContainerMember = th
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
        new_board(TEST_WORKSPACE_ID, ContainerType::Open, "open"),
        &user1.id,
    )
    .await;
    let private = board_with_admin(
        &th,
        new_board(TEST_WORKSPACE_ID, ContainerType::Private, "private"),
        &user1.id,
    )
    .await;

    // A commenter of an open board.
    add_member(
        &th,
        &ContainerMember {
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
        .update_container_member(&editor(&open.id, &user2.id))
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
    let creator = th.stranger().await;
    let user = stored_user(&th, "member").await;
    let board = board_without_members(
        &th,
        new_board(TEST_WORKSPACE_ID, ContainerType::Open, ""),
        &creator,
    )
    .await;
    let new_member = editor(&board.id, &user);

    let added = add_member(&th, &new_member).await;

    assert_eq!(added, new_member, "the membership as asked for");
    assert_eq!(
        members_of(&th, &board.id).await,
        vec![new_member],
        "and stored"
    );
    assert_eq!(
        category_name_of(&th, &user, TEST_WORKSPACE_ID, &board.id).await,
        Some("Boards".to_owned()),
        "the board is in the member's default category"
    );

    // A board that does not exist has no members to add to.
    for board_id in [UNKNOWN_ID, "no-such-board"] {
        let nothing = th
            .app
            .add_member_to_container(&editor(board_id, &user))
            .await
            .expect("not an error");
        assert_eq!(nothing, None, "nothing is answered for {board_id}");
    }

    th.teardown().await;
}

/// `TestAddMemberToBoard/"return existing non-synthetic membership if any"` in
/// `server/app/boards_test.go`, through the real store.
#[tokio::test]
async fn add_member_to_board_return_existing_non_synthetic_membership_if_any() {
    let th = Setup::native().start().await;
    let creator = th.stranger().await;
    let user = stored_user(&th, "member").await;
    let board = board_without_members(
        &th,
        new_board(TEST_WORKSPACE_ID, ContainerType::Open, ""),
        &creator,
    )
    .await;
    let existing = add_member(&th, &editor(&board.id, &user)).await;

    let answered = add_member(
        &th,
        &ContainerMember {
            scheme_admin: true,
            ..member(&board.id, &user)
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
        new_board(TEST_WORKSPACE_ID, ContainerType::Open, "title"),
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
        new_board(TEST_WORKSPACE_ID, ContainerType::Open, "title"),
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

    for board_id in [UNKNOWN_ID, "non-existent-board-id"] {
        let response = update_member(&th.client, &editor(board_id, &user1.id)).await;
        response.check_error(StatusCode::NOT_FOUND, "board not found");
    }
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
        new_board(TEST_WORKSPACE_ID, ContainerType::Open, "title"),
        &user1.id,
    )
    .await;
    let added = add_member(&th, &editor(&board.id, &user2.id)).await;
    assert!(!added.scheme_admin && added.scheme_editor, "an editor");
    let update = ContainerMember {
        scheme_admin: true,
        scheme_editor: true,
        ..member(&board.id, &user2.id)
    };

    let updated: ContainerMember = update_member(&th.client, &update).await.ok_json();

    assert_eq!(updated, update, "the new roles are answered");
    let stored = th
        .app
        .store()
        .get_member_for_container(&board.id, &user2.id);
    assert_eq!(
        stored.await.expect("the membership is read"),
        update,
        "and stored"
    );
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
        new_board(TEST_WORKSPACE_ID, ContainerType::Open, "title"),
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
        new_board(TEST_WORKSPACE_ID, ContainerType::Private, "title"),
        &user1.id,
    )
    .await;
    let path = format!("{TEST_WS}/boards/{}/members/{}", board.id, user2.id);

    // Who is no member of the private board learns nothing, whatever the body and
    // whoever the path names. (The original decodes the body and looks the user up
    // first, for anyone.)
    for target in [user2.id.as_str(), UNKNOWN_ID, "no-such-user"] {
        for body in ["{bad", "{\"schemeEditor\":true}"] {
            let response = th
                .client2
                .send_raw_body(
                    Method::PUT,
                    &format!("{TEST_WS}/boards/{}/members/{target}", board.id),
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
    for target in [user2.id.as_str(), UNKNOWN_ID, "no-such-user"] {
        let response = th
            .client
            .put(
                &format!("{TEST_WS}/boards/{}/members/{target}", board.id),
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
    let updated: ContainerMember = th
        .client
        .put(
            &path,
            &json!({"userId": "ignored", "boardId": "ignored", "schemeAdmin": true}),
        )
        .await
        .ok_json();
    assert_eq!(
        updated,
        ContainerMember {
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

// -- TestDeleteMember -----------------------------------------------------------------

/// `TestDeleteMember/"a non authenticated user should be rejected"`
#[tokio::test]
async fn delete_member_a_non_authenticated_user_should_be_rejected() {
    let th = Setup::native().start().await.init_basic().await;
    let user1 = th.client.me().await;
    let board = board_with_admin(
        &th,
        new_board(TEST_WORKSPACE_ID, ContainerType::Open, "title"),
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
        new_board(TEST_WORKSPACE_ID, ContainerType::Open, "title"),
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

    for board_id in [UNKNOWN_ID, "non-existent-board-id"] {
        let response = delete_member(&th.client, &member(board_id, &user1.id)).await;
        response.check_error(StatusCode::NOT_FOUND, "board not found");
    }
    th.teardown().await;
}

/// A private board with user1 as admin and user2 as editor, as the subtests of
/// `TestDeleteMember/"should correctly delete a member for a valid board"` set up.
async fn private_board_with_users(th: &TestServer) -> Container {
    let user1 = th.client.me().await;
    let user2 = th.client2.me().await;
    let board = board_with_admin(
        th,
        new_board(TEST_WORKSPACE_ID, ContainerType::Private, "title"),
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

    response.check_no_content();
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
        new_board(TEST_WORKSPACE_ID, ContainerType::Private, "title"),
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

    // Verified on the original: removing somebody who is not a member succeeds, whether
    // or not the path names anybody.
    for no_member in [UNKNOWN_ID, "no-such-user"] {
        delete_member(&th.client, &member(&board.id, no_member))
            .await
            .check_no_content();
    }
    th.teardown().await;
}

// -- TestJoinBoard --------------------------------------------------------------------

/// `TestJoinBoard/"create and join public board"`
#[tokio::test]
async fn join_board_create_and_join_public_board() {
    let th = Setup::native().start().await.init_basic().await;
    let me = th.client.me().await;
    let user2 = th.client2.me().await;
    let board: Container = th
        .client
        .create_board(&new_board(
            TEST_WORKSPACE_ID,
            ContainerType::Open,
            "Test Public board",
        ))
        .await
        .created_json();
    assert_eq!(board.created_by, me.id, "created by the session's user");
    assert_eq!(
        board.minimum_role,
        MinimumRole::Editor,
        "a board that names none starts with editor"
    );

    let joined: ContainerMember = join_board(&th.client2, &board.id).await.ok_json();

    assert_eq!(joined.container_id, board.id, "of the board");
    assert_eq!(joined.user_id, user2.id, "the joining user");
    assert_eq!(
        category_name_of(&th, &user2.id, TEST_WORKSPACE_ID, &board.id).await,
        Some("Boards".to_owned()),
        "the board is filed under the new member's default category"
    );

    // Verified on the original: joining again, or as the admin, answers the membership
    // there is.
    let again: ContainerMember = join_board(&th.client2, &board.id).await.ok_json();
    assert_eq!(again, joined, "the same membership");
    let admin: ContainerMember = join_board(&th.client, &board.id).await.ok_json();
    assert!(admin.scheme_admin, "the admin stays an admin: {admin:?}");
    th.teardown().await;
}

/// `TestJoinBoard/"create and join public board should match the minimumRole in the
/// membership"`, for every minimum role a board can have.
#[tokio::test]
async fn join_board_should_match_the_minimum_role_in_the_membership() {
    let th = Setup::native().start().await.init_basic().await;
    let user2 = th.client2.me().await;

    for (minimum_role, expected) in [
        (MinimumRole::Commenter, ONLY_COMMENTER),
        (MinimumRole::Viewer, ONLY_VIEWER),
        (MinimumRole::Editor, ONLY_EDITOR),
    ] {
        let board: Container = th
            .client
            .create_board(&Container {
                minimum_role,
                ..new_board(
                    TEST_WORKSPACE_ID,
                    ContainerType::Open,
                    "Public board with a role",
                )
            })
            .await
            .created_json();

        let joined: ContainerMember = join_board(&th.client2, &board.id).await.ok_json();

        assert_eq!(
            (joined.container_id.as_str(), joined.user_id.as_str()),
            (board.id.as_str(), user2.id.as_str()),
            "the membership of the user on the board"
        );
        assert_eq!(
            role_flags(&joined),
            expected,
            "joining a board whose minimum role is {minimum_role:?}"
        );
    }
    th.teardown().await;
}

/// `TestJoinBoard/"create and join public board should match editor role in the
/// membership when MinimumRole is empty"`: a board that says nothing about its minimum
/// role has editor.
#[tokio::test]
async fn join_board_should_match_editor_role_when_the_board_names_no_minimum_role() {
    let th = Setup::native().start().await.init_basic().await;
    let user2 = th.client2.me().await;
    let board: Container = th
        .client
        .create_board(&new_board(
            TEST_WORKSPACE_ID,
            ContainerType::Open,
            "Public board for editors",
        ))
        .await
        .created_json();

    let joined: ContainerMember = join_board(&th.client2, &board.id).await.ok_json();

    assert_eq!(
        role_flags(&joined),
        ONLY_EDITOR,
        "new member should be editor and nothing else"
    );
    assert_eq!(joined.user_id, user2.id, "the user who joined");
    th.teardown().await;
}

/// A member below the board's minimum role who joins again is raised to it. Joining
/// never lowers anybody: a member with the minimum role or a higher one stays as they
/// are, the administrator included.
#[tokio::test]
async fn joining_again_raises_to_the_minimum_role_and_never_lowers() {
    let th = Setup::native().start().await.init_basic().await;
    let user1 = th.client.me().await;
    let user2 = th.client2.me().await;
    let board = board_with_admin(
        &th,
        Container {
            minimum_role: MinimumRole::Commenter,
            ..new_board(TEST_WORKSPACE_ID, ContainerType::Open, "open")
        },
        &user1.id,
    )
    .await;
    let stored_role = async || {
        let members = members_of(&th, &board.id).await;
        let member = members
            .iter()
            .find(|member| member.user_id == user2.id)
            .expect("user2 is a member");
        role_flags(member)
    };

    // The administrator made user2 a viewer: below the board's minimum role.
    add_member(&th, &viewer(&board.id, &user2.id)).await;
    assert_eq!(
        stored_role().await,
        ONLY_VIEWER,
        "as the administrator said"
    );
    let joined: ContainerMember = join_board(&th.client2, &board.id).await.ok_json();
    assert_eq!(
        role_flags(&joined),
        ONLY_COMMENTER,
        "raised by joining again"
    );
    assert_eq!(stored_role().await, ONLY_COMMENTER, "and stored so");

    // With the minimum role: joining again changes nothing.
    let again: ContainerMember = join_board(&th.client2, &board.id).await.ok_json();
    assert_eq!(role_flags(&again), ONLY_COMMENTER, "as before");

    // Above it: an editor stays an editor.
    th.client
        .put(
            &format!("{TEST_WS}/boards/{}/members/{}", board.id, user2.id),
            &editor(&board.id, &user2.id),
        )
        .await
        .check_ok();
    let as_editor: ContainerMember = join_board(&th.client2, &board.id).await.ok_json();
    assert_eq!(role_flags(&as_editor), ONLY_EDITOR, "not lowered");
    assert_eq!(stored_role().await, ONLY_EDITOR, "nor in the store");

    // The minimum role goes up: members below it stay where they are until they join
    // again, and then are raised.
    patch_board(&th.client, &board.id, &json!({"minimumRole": "viewer"}))
        .await
        .check_ok();
    th.client
        .put(
            &format!("{TEST_WS}/boards/{}/members/{}", board.id, user2.id),
            &viewer(&board.id, &user2.id),
        )
        .await
        .check_ok();
    patch_board(&th.client, &board.id, &json!({"minimumRole": "editor"}))
        .await
        .check_ok();
    assert_eq!(
        stored_role().await,
        ONLY_VIEWER,
        "raising the minimum raises nobody"
    );
    th.client2
        .insert_blocks(&board.id, &[card_block(&board.id)])
        .await
        .check_forbidden();
    let raised: ContainerMember = join_board(&th.client2, &board.id).await.ok_json();
    assert_eq!(role_flags(&raised), ONLY_EDITOR, "joining again does");
    th.client2
        .insert_blocks(&board.id, &[card_block(&board.id)])
        .await
        .check_created();

    // The administrator joins: still the administrator.
    let admin: ContainerMember = join_board(&th.client, &board.id).await.ok_json();
    assert!(
        admin.scheme_admin,
        "the administrator is not lowered: {admin:?}"
    );

    th.teardown().await;
}

/// The role that counts is the role a member has. An administrator can give a member
/// less than the board's minimum role, and the member then has less: the minimum role
/// is what joining gives, not something every member has anyway.
#[tokio::test]
async fn a_member_below_the_minimum_role_has_the_role_they_were_given() {
    let th = Setup::native().start().await.init_basic().await;
    let user1 = th.client.me().await;
    let user2 = th.client2.me().await;
    // A private board cannot be joined, so the member cannot raise themself.
    let board = board_with_admin(
        &th,
        new_board(TEST_WORKSPACE_ID, ContainerType::Private, "private"),
        &user1.id,
    )
    .await;
    assert_eq!(board.minimum_role, MinimumRole::Editor, "the default");

    add_member(&th, &viewer(&board.id, &user2.id)).await;

    th.client2
        .get(&format!("{TEST_WS}/boards/{}/blocks", board.id))
        .await
        .check_ok();
    th.client2
        .insert_blocks(&board.id, &[card_block(&board.id)])
        .await
        .check_forbidden();
    join_board(&th.client2, &board.id)
        .await
        .check_error(StatusCode::FORBIDDEN, "cannot join a non Open board");

    th.teardown().await;
}

/// A template is not joined. An open template is there for every user of its workspace to
/// copy; joining made anyone its editor, and the built-in templates are open boards of
/// the workspace every user is in. (The original lets anyone join them.)
#[tokio::test]
async fn join_board_refuses_a_template() {
    let th = Setup::native().start().await.init_basic().await;
    let user1 = th.client.me().await;
    let template = board_with_admin(
        &th,
        Container {
            is_template: true,
            ..new_board(TEST_WORKSPACE_ID, ContainerType::Open, "an open template")
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
    let board: Container = th
        .client
        .create_board(&new_board(
            TEST_WORKSPACE_ID,
            ContainerType::Private,
            "Private board",
        ))
        .await
        .created_json();

    // Verified on the original: `allow_admin` is for workspace administrators, which a
    // standalone server does not have.
    for query in ["", "?allow_admin", "?allow_admin=true"] {
        let response = th
            .client2
            .post_empty(&format!("{TEST_WS}/boards/{}/join{query}", board.id))
            .await;
        // A private board does not exist for who is no member, so there is nothing
        // to join.
        response.check_error(StatusCode::NOT_FOUND, "board not found");
        assert_eq!(response.problem_type(), "/problems/not-found", "{query}");
    }
    assert_eq!(members_of(&th, &board.id).await.len(), 1, "nobody joined");
    th.teardown().await;
}

/// `TestJoinBoard/"join invalid board"`
#[tokio::test]
async fn join_board_join_invalid_board() {
    let th = Setup::native().start().await.init_basic().await;

    for board_id in [UNKNOWN_ID, "nonexistent-board-ID"] {
        let response = join_board(&th.client2, board_id).await;
        response.check_error(StatusCode::NOT_FOUND, "board not found");
    }
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
    for board_id in [UNKNOWN_ID, "nonexistent"] {
        let response = leave_board(&th.client, board_id).await;
        response.check_error(StatusCode::NOT_FOUND, "board not found");
    }
    let outsider = th.add_user("user3").await;
    leave_board(&outsider, &board.id).await.check_forbidden();

    let response = leave_board(&th.client2, &board.id).await;
    response.check_no_content();
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
        .delete(&format!("{TEST_WS}/boards/{}", board.id))
        .await
        .check_no_content();
    leave_board(&th.client2, &board.id).await.check_not_found();

    th.teardown().await;
}

// -- TestPatchBoard (server/app/boards_test.go) ---------------------------------------

/// `TestPatchBoard/"base case, title patch"` in `server/app/boards_test.go`, through the
/// real store.
#[tokio::test]
async fn app_patch_board_base_case_title_patch() {
    let th = Setup::native().start().await;
    let user = th.stranger_in(TEST_WORKSPACE_ID).await;
    let board = board_with_admin(
        &th,
        new_board(TEST_WORKSPACE_ID, ContainerType::Open, "Title"),
        &user,
    )
    .await;

    let patched = th
        .app
        .patch_container(&title_patch("Patched Title"), &board.id, &user)
        .await
        .expect("the board is patched");

    assert_eq!(patched.title, "Patched Title", "the new title");
    assert_eq!(stored_board(&th, &board.id).await, patched, "and stored");
    th.teardown().await;
}

// -- TestBoardCategory (server/app/boards_test.go) ------------------------------------

/// `TestBoardCategory/"no boards default category exists"` in
/// `server/app/boards_test.go`, through the real store: a user who has categories but
/// not the default one gets it when a board needs filing.
#[tokio::test]
async fn board_category_no_boards_default_category_exists() {
    let th = Setup::native().start().await;
    let user = th.stranger_in(TEST_WORKSPACE_ID).await;
    for name in ["Category 1", "Category 2", "Category 3"] {
        th.app
            .create_category(Category {
                name: name.to_owned(),
                user_id: user.clone(),
                workspace_id: TEST_WORKSPACE_ID.to_owned(),
                ..Category::default()
            })
            .await
            .expect("the category is created");
    }

    let mut board_ids = Vec::new();
    for title in ["board 1", "board 2", "board 3"] {
        let board = board_with_admin(
            &th,
            new_board(TEST_WORKSPACE_ID, ContainerType::Open, title),
            &user,
        )
        .await;
        board_ids.push(board.id);
    }

    let categories = categories_of(&th, &user, TEST_WORKSPACE_ID).await;
    assert_eq!(categories.len(), 4, "the default category was added");
    let default = categories
        .iter()
        .find(|entry| entry.category.name == "Boards")
        .expect("the default category exists");
    let mut filed: Vec<String> = default
        .container_metadata
        .iter()
        .map(|metadata| metadata.container_id.clone())
        .collect();
    filed.sort();
    board_ids.sort();
    assert_eq!(filed, board_ids, "the boards are in the default category");
    th.teardown().await;
}
