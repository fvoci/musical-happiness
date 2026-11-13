//! What the board routes tell over websocket connections, and the socket the tests listen with.

use std::time::Duration;

use futures_util::{SinkExt as _, StreamExt as _};
use fvoci_model::{Board, BoardMember, BoardType, BoardsAndBlocks, Category, new_id};
use serde_json::{Value, json};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

use crate::blocks::title_patch;
use crate::boards::{
    board_with_admin, delete_member, duplicate_board, editor, insert_block, member, new_board,
    patch_board, undelete_board, update_member, view_block,
};
use crate::common::TEST_WS;
use crate::common::{Client, Setup, TEST_WORKSPACE_ID, TestServer};

/// How long to wait for a message that should arrive.
const PATIENCE: Duration = Duration::from_secs(5);
/// How long to wait before deciding that no more messages arrive.
const QUIET: Duration = Duration::from_millis(300);

/// A websocket connection of one user, subscribed to one workspace.
pub(crate) struct WorkspaceSocket {
    stream: WebSocketStream<MaybeTlsStream<TcpStream>>,
}

impl WorkspaceSocket {
    /// Connects as the client and subscribes to the workspace.
    pub(crate) async fn subscribe(
        th: &TestServer,
        client: &Client,
        workspace_id: &str,
        user_id: &str,
    ) -> Self {
        let stream = client.connect_websocket().await;
        let mut socket = Self { stream };
        socket
            .send(json!({"action": "SUBSCRIBE_WORKSPACE", "workspaceId": workspace_id}))
            .await;

        // Commands have no acknowledgement: wait until a broadcast comes through.
        let probe = Category {
            id: new_id(),
            name: "subscription probe".to_owned(),
            user_id: user_id.to_owned(),
            workspace_id: workspace_id.to_owned(),
            ..Category::default()
        };
        for _ in 0..50 {
            th.app.ws().broadcast_category_change(&probe).await;
            if socket.next(Duration::from_millis(100)).await.is_some() {
                socket.drain().await;
                return socket;
            }
        }
        panic!("the socket was never subscribed to workspace {workspace_id}");
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
    let mut socket1 =
        WorkspaceSocket::subscribe(&th, &th.client, TEST_WORKSPACE_ID, &user1.id).await;
    let mut socket2 =
        WorkspaceSocket::subscribe(&th, &th.client2, TEST_WORKSPACE_ID, &user2.id).await;

    let board: Board = th
        .client
        .create_board(&new_board(TEST_WORKSPACE_ID, BoardType::Open, "Roadmap"))
        .await
        .created_json();
    let message = socket1.expect("UPDATE_BOARD", "/board/id", &board.id).await;
    assert_eq!(
        message["workspaceId"],
        json!(TEST_WORKSPACE_ID),
        "{message}"
    );
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
        .delete(&format!("{TEST_WS}/boards/{}", board.id))
        .await
        .check_no_content();
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
        new_board(TEST_WORKSPACE_ID, BoardType::Private, "title"),
        &user1.id,
    )
    .await;
    let mut socket1 =
        WorkspaceSocket::subscribe(&th, &th.client, TEST_WORKSPACE_ID, &user1.id).await;
    let mut socket2 =
        WorkspaceSocket::subscribe(&th, &th.client2, TEST_WORKSPACE_ID, &user2.id).await;

    th.client
        .add_member(&editor(&board.id, &user2.id))
        .await
        .check_ok();
    for socket in [&mut socket1, &mut socket2] {
        let message = socket
            .expect("UPDATE_MEMBER", "/member/userId", &user2.id)
            .await;
        assert_eq!(
            message["workspaceId"],
            json!(TEST_WORKSPACE_ID),
            "{message}"
        );
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
        .check_no_content();
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
/// private, the workspace's users who are not members of it are told that they lost or gained
/// access (`server/app/boards.go:402-432`).
#[tokio::test]
async fn patch_board_type_of_a_template_is_broadcast_to_the_workspace_users() {
    let th = Setup::native().start().await.init_basic().await;
    let user1 = th.client.me().await;
    let user2 = th.client2.me().await;
    let template = board_with_admin(
        &th,
        Board {
            is_template: true,
            ..new_board(TEST_WORKSPACE_ID, BoardType::Open, "a template")
        },
        &user1.id,
    )
    .await;
    let mut socket1 =
        WorkspaceSocket::subscribe(&th, &th.client, TEST_WORKSPACE_ID, &user1.id).await;
    let mut socket2 =
        WorkspaceSocket::subscribe(&th, &th.client2, TEST_WORKSPACE_ID, &user2.id).await;

    // Private: every user who is not a member is told that their access ended. The
    // message goes to the members and to that user.
    patch_board(&th.client, &template.id, &json!({"type": "P"}))
        .await
        .check_ok();
    for socket in [&mut socket1, &mut socket2] {
        let message = socket
            .expect("DELETE_MEMBER", "/member/userId", &user2.id)
            .await;
        assert_eq!(
            message["workspaceId"],
            json!(TEST_WORKSPACE_ID),
            "{message}"
        );
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

    // Open: the members are told about a viewer membership of every other user.
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
            "schemeAdmin": false,
            "schemeEditor": false,
            "schemeCommenter": false,
            "schemeViewer": true,
        }),
        "a viewer"
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
        new_board(TEST_WORKSPACE_ID, BoardType::Open, "a board"),
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
        new_board(TEST_WORKSPACE_ID, BoardType::Open, "Source"),
        &user1.id,
    )
    .await;
    insert_block(&th, view_block(&board.id), &user1.id).await;
    let mut socket =
        WorkspaceSocket::subscribe(&th, &th.client, TEST_WORKSPACE_ID, &user1.id).await;

    let copy: BoardsAndBlocks = duplicate_board(&th.client, &board.id, "")
        .await
        .created_json();

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
