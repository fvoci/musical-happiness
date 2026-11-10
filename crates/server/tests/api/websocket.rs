//! The websocket endpoint over a real connection: authentication, subscriptions, who is
//! told about which change, and pings. The original has no tests at this level; its unit
//! tests of the listener table (`server/ws/server_test.go`) are ported next to the hub.

use std::time::Duration;

use futures_util::{SinkExt as _, StreamExt as _};
use fvoci_model::{Block, BlockType, Board, BoardType, Category, new_id};
use serde_json::{Value, json};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

use crate::common::{Client, ROOT_TEAM_ID, Setup, TestServer};

/// How long to wait for a message that should arrive.
const PATIENCE: Duration = Duration::from_secs(5);
/// How long to wait before deciding that a message does not arrive.
const QUIET: Duration = Duration::from_millis(300);

struct Socket {
    stream: WebSocketStream<MaybeTlsStream<TcpStream>>,
}

/// What a socket yields next.
#[derive(Debug, PartialEq)]
enum Event {
    Json(Value),
    Ping,
    Closed,
    Nothing,
}

impl Socket {
    /// A connection without a session, as a page opens one that nobody is signed in
    /// on.
    async fn connect(th: &TestServer) -> Self {
        let stream = th.anonymous().as_browser().connect_websocket().await;
        Self { stream }
    }

    /// A connection of the client's session: whose it is is settled by the handshake.
    async fn connect_as(client: &Client) -> Self {
        let stream = client.connect_websocket().await;
        Self { stream }
    }

    async fn send(&mut self, command: Value) {
        self.stream
            .send(Message::Text(command.to_string().into()))
            .await
            .expect("the command is sent");
    }

    async fn next(&mut self, wait: Duration) -> Event {
        loop {
            match tokio::time::timeout(wait, self.stream.next()).await {
                Err(_) => return Event::Nothing,
                Ok(None | Some(Err(_) | Ok(Message::Close(_)))) => return Event::Closed,
                Ok(Some(Ok(Message::Text(text)))) => {
                    return Event::Json(serde_json::from_str(&text).expect("messages are JSON"));
                }
                Ok(Some(Ok(Message::Ping(_)))) => return Event::Ping,
                Ok(Some(Ok(_))) => {}
            }
        }
    }

    /// The next JSON message, skipping pings.
    async fn next_json(&mut self, wait: Duration) -> Option<Value> {
        loop {
            match self.next(wait).await {
                Event::Json(value) => return Some(value),
                Event::Ping => {}
                Event::Closed | Event::Nothing => return None,
            }
        }
    }
}

fn category(team_id: &str, user_id: &str, name: &str) -> Category {
    Category {
        id: new_id(),
        name: name.to_owned(),
        user_id: user_id.to_owned(),
        team_id: team_id.to_owned(),
        ..Category::default()
    }
}

/// Subscribes an authenticated socket to a team and waits until the server has done it.
/// Commands have no acknowledgement, so the wait is for a broadcast to come through.
async fn subscribe_to_team(th: &TestServer, socket: &mut Socket, team_id: &str, user_id: &str) {
    socket
        .send(json!({"action": "SUBSCRIBE_TEAM", "teamId": team_id}))
        .await;
    let probe = category(team_id, user_id, "subscription probe");
    for _ in 0..50 {
        th.app.ws().broadcast_category_change(&probe);
        if let Some(message) = socket.next_json(Duration::from_millis(100)).await {
            assert_eq!(message["action"], json!("UPDATE_CATEGORY"), "{message}");
            // Later probes may still be queued; let them drain.
            while socket.next_json(Duration::from_millis(150)).await.is_some() {}
            return;
        }
    }
    panic!("the socket was never subscribed to team {team_id}");
}

/// A connection of a session subscribes to a team and is told about the user's category
/// changes; on a connection without a session the subscription is ignored.
#[tokio::test]
async fn team_subscription_needs_authentication() {
    let th = Setup::native().start().await.init_basic().await;
    let user1 = th.client.me().await;

    // No session: the subscription is ignored, and so is an unknown action.
    let mut anonymous = Socket::connect(&th).await;
    anonymous
        .send(json!({"action": "SUBSCRIBE_TEAM", "teamId": ROOT_TEAM_ID}))
        .await;
    anonymous.send(json!({"action": "NO_SUCH_ACTION"})).await;
    anonymous.send(json!("not a command")).await;

    let mut socket = Socket::connect_as(&th.client).await;
    subscribe_to_team(&th, &mut socket, ROOT_TEAM_ID, &user1.id).await;

    let change = category(ROOT_TEAM_ID, &user1.id, "Projects");
    th.app.ws().broadcast_category_change(&change);
    let message = socket
        .next_json(PATIENCE)
        .await
        .expect("the change arrives");
    assert_eq!(message["action"], json!("UPDATE_CATEGORY"), "{message}");
    assert_eq!(message["teamId"], json!(ROOT_TEAM_ID), "{message}");
    assert_eq!(message["category"]["name"], json!("Projects"), "{message}");
    assert_eq!(message["category"]["userID"], json!(user1.id), "{message}");
    assert!(message.get("blockCategories").is_none(), "{message}");

    assert_eq!(
        anonymous.next(QUIET).await,
        Event::Nothing,
        "the anonymous socket hears nothing"
    );

    // After unsubscribing, nothing arrives any more. The second subscription to another
    // team shows that the server has processed the commands before it.
    socket
        .send(json!({"action": "UNSUBSCRIBE_TEAM", "teamId": ROOT_TEAM_ID}))
        .await;
    subscribe_to_team(&th, &mut socket, "other-team", &user1.id).await;
    th.app.ws().broadcast_category_change(&change);
    assert_eq!(
        socket.next(QUIET).await,
        Event::Nothing,
        "unsubscribed from the team"
    );

    th.teardown().await;
}

/// There is no message to sign in with: a connection is whose its handshake says, and
/// stays that. `AUTH`, which used to carry the token, is an action like any unknown one.
#[tokio::test]
async fn a_message_does_not_sign_a_connection_in() {
    let th = Setup::native().start().await.init_basic().await;
    let user1 = th.client.me().await;
    let mut socket = Socket::connect(&th).await;

    socket
        .send(json!({"action": "AUTH", "token": th.client.token}))
        .await;
    socket
        .send(json!({"action": "SUBSCRIBE_TEAM", "teamId": ROOT_TEAM_ID}))
        .await;
    th.app
        .ws()
        .broadcast_category_change(&category(ROOT_TEAM_ID, &user1.id, "Projects"));

    assert_eq!(
        socket.next(QUIET).await,
        Event::Nothing,
        "the connection is nobody's: it hears nothing, and it is not closed either"
    );
    th.teardown().await;
}

/// Board and block changes go to the team's subscribers who are members of the board.
#[tokio::test]
async fn board_changes_go_to_subscribed_members() {
    let th = Setup::native().start().await.init_basic().await;
    let user1 = th.client.me().await;
    let user2 = th.client2.me().await;

    let mut board = Board {
        id: new_id(),
        team_id: ROOT_TEAM_ID.to_owned(),
        board_type: BoardType::Private,
        title: "Roadmap".to_owned(),
        ..Board::default()
    };
    th.app
        .store()
        .insert_board_with_admin(&mut board, &user1.id)
        .await
        .expect("the board is stored with user1 as admin");

    let mut member = Socket::connect_as(&th.client).await;
    subscribe_to_team(&th, &mut member, ROOT_TEAM_ID, &user1.id).await;
    let mut outsider = Socket::connect_as(&th.client2).await;
    subscribe_to_team(&th, &mut outsider, ROOT_TEAM_ID, &user2.id).await;

    th.app
        .ws()
        .broadcast_board_change(ROOT_TEAM_ID, &board)
        .await;
    let message = member
        .next_json(PATIENCE)
        .await
        .expect("the member is told");
    assert_eq!(message["action"], json!("UPDATE_BOARD"), "{message}");
    assert_eq!(message["teamId"], json!(ROOT_TEAM_ID), "{message}");
    assert_eq!(message["board"]["id"], json!(board.id), "{message}");
    assert_eq!(message["board"]["title"], json!("Roadmap"), "{message}");

    let mut block = Block {
        id: new_id(),
        board_id: board.id.clone(),
        block_type: BlockType::Card,
        title: "A card".to_owned(),
        ..Block::default()
    };
    th.app
        .store()
        .insert_block(&mut block, &user1.id)
        .await
        .expect("the block is stored");
    th.app
        .ws()
        .broadcast_block_change(ROOT_TEAM_ID, &block)
        .await;
    let message = member
        .next_json(PATIENCE)
        .await
        .expect("the member is told");
    assert_eq!(message["action"], json!("UPDATE_BLOCK"), "{message}");
    assert_eq!(message["block"]["id"], json!(block.id), "{message}");
    assert_eq!(message["block"]["title"], json!("A card"), "{message}");

    th.app
        .ws()
        .broadcast_block_delete(ROOT_TEAM_ID, &block.id, &board.id)
        .await;
    let message = member
        .next_json(PATIENCE)
        .await
        .expect("the member is told");
    assert_eq!(message["action"], json!("UPDATE_BLOCK"), "{message}");
    assert_eq!(message["block"]["id"], json!(block.id), "{message}");
    assert_ne!(
        message["block"]["deleteAt"],
        json!(0),
        "a deletion carries its time: {message}"
    );

    // Removing a member tells the members and the removed user.
    th.app
        .ws()
        .broadcast_member_delete(ROOT_TEAM_ID, &board.id, &user2.id)
        .await;
    let message = member
        .next_json(PATIENCE)
        .await
        .expect("the member is told");
    assert_eq!(message["action"], json!("DELETE_MEMBER"), "{message}");
    assert_eq!(message["member"]["userId"], json!(user2.id), "{message}");
    let message = outsider
        .next_json(PATIENCE)
        .await
        .expect("the removed user is told");
    assert_eq!(message["action"], json!("DELETE_MEMBER"), "{message}");

    // Everything before that passed the user who is no member by.
    assert_eq!(
        outsider.next(QUIET).await,
        Event::Nothing,
        "nothing else for the outsider"
    );

    th.teardown().await;
}

/// Anonymous readers of a shared board subscribe to its blocks with the read token.
#[tokio::test]
async fn block_subscription_needs_the_read_token() {
    let th = Setup::local_mode().start().await.init_basic().await;
    let user1 = th.client.me().await;

    let mut board = Board {
        id: new_id(),
        team_id: ROOT_TEAM_ID.to_owned(),
        board_type: BoardType::Private,
        ..Board::default()
    };
    th.app
        .store()
        .insert_board_with_admin(&mut board, &user1.id)
        .await
        .expect("the board is stored");
    let mut block = Block {
        id: new_id(),
        board_id: board.id.clone(),
        block_type: BlockType::Card,
        ..Block::default()
    };
    th.app
        .store()
        .insert_block(&mut block, &user1.id)
        .await
        .expect("the block is stored");
    let token = th.share(&board.id).await;

    let subscribe = |read_token: &str| {
        json!({
            "action": "SUBSCRIBE_BLOCKS",
            "teamId": ROOT_TEAM_ID,
            "readToken": read_token,
            "blockIds": [block.id],
        })
    };

    let mut wrong = Socket::connect(&th).await;
    wrong.send(subscribe("not-the-read-token")).await;
    let mut reader = Socket::connect(&th).await;
    reader.send(subscribe(&token)).await;

    // No acknowledgement: broadcast until the reader's subscription shows.
    let mut received = None;
    for _ in 0..50 {
        th.app
            .ws()
            .broadcast_block_change(ROOT_TEAM_ID, &block)
            .await;
        if let Some(message) = reader.next_json(Duration::from_millis(100)).await {
            received = Some(message);
            break;
        }
    }
    let message = received.expect("the reader is told about the block");
    assert_eq!(message["action"], json!("UPDATE_BLOCK"), "{message}");
    assert_eq!(message["block"]["id"], json!(block.id), "{message}");

    assert_eq!(
        wrong.next(QUIET).await,
        Event::Nothing,
        "a wrong read token subscribes to nothing"
    );

    th.teardown().await;
}

/// The server pings every connection at the configured interval, so that proxies in
/// between do not close it as idle. A deliberate difference: the original never pings.
#[tokio::test]
async fn the_server_pings_at_the_configured_interval() {
    let th = Setup::native()
        .configure(|config| config.websocket_ping_seconds = 1)
        .start()
        .await;
    let mut socket = Socket::connect(&th).await;

    // The first tick of the interval is immediate; the second comes a second later.
    assert_eq!(
        socket.next(PATIENCE).await,
        Event::Ping,
        "a ping on connect"
    );
    let started = std::time::Instant::now();
    assert_eq!(socket.next(PATIENCE).await, Event::Ping, "and another one");
    let waited = started.elapsed();
    assert!(
        waited >= Duration::from_millis(500) && waited < Duration::from_secs(3),
        "about a second apart, was {waited:?}"
    );

    th.teardown().await;
}

/// Without a ping interval nothing is sent to an idle connection, as in the original.
#[tokio::test]
async fn no_pings_when_turned_off() {
    let th = Setup::native().start().await;
    let mut socket = Socket::connect(&th).await;

    assert_eq!(
        socket.next(QUIET).await,
        Event::Nothing,
        "an idle connection stays quiet"
    );
    th.teardown().await;
}

/// Commands whose members have the wrong JSON type are dropped and the connection goes
/// on. The plugin repository that descends from the original had a crash of this kind in
/// its own adapter (its PR #211); the standalone server decodes commands into a typed
/// structure, as the port does, so a mismatch is a decoding error and nothing more.
#[tokio::test]
async fn commands_with_members_of_the_wrong_type_are_ignored() {
    let th = Setup::native().start().await.init_basic().await;
    let user1 = th.client.me().await;
    let mut socket = Socket::connect_as(&th.client).await;

    let malformed = [
        json!({"action": "SUBSCRIBE_TEAM", "teamId": 5}),
        json!({"action": "SUBSCRIBE_BLOCKS", "teamId": ROOT_TEAM_ID, "blockIds": "not-a-list"}),
        json!({"action": "SUBSCRIBE_BLOCKS", "teamId": ROOT_TEAM_ID, "blockIds": [1, 2, 3]}),
        json!({"action": "SUBSCRIBE_BLOCKS", "teamId": ROOT_TEAM_ID, "readToken": {"a": 1}, "blockIds": []}),
        json!({"action": 7}),
        json!([1, 2, 3]),
        json!(null),
    ];
    for command in malformed {
        socket.send(command).await;
    }

    // The same connection still subscribes and is told about changes.
    subscribe_to_team(&th, &mut socket, ROOT_TEAM_ID, &user1.id).await;
    let change = category(ROOT_TEAM_ID, &user1.id, "After the noise");
    th.app.ws().broadcast_category_change(&change);
    let message = socket
        .next_json(PATIENCE)
        .await
        .expect("the change arrives");
    assert_eq!(
        message["category"]["name"],
        json!("After the noise"),
        "{message}"
    );

    th.teardown().await;
}

/// A password that an administrator sets signs the user out everywhere, live websocket
/// connections included: a connection checks its token only once, at its handshake.
/// Connections of other users stay.
#[tokio::test]
async fn resetting_a_password_closes_the_websocket_connections_of_that_user() {
    let th = Setup::native().start().await.init_basic().await;
    let user1 = th.client.me().await;
    let user2 = th.client2.me().await;
    let mut socket1 = Socket::connect_as(&th.client).await;
    subscribe_to_team(&th, &mut socket1, ROOT_TEAM_ID, &user1.id).await;
    let mut socket2 = Socket::connect_as(&th.client2).await;
    subscribe_to_team(&th, &mut socket2, ROOT_TEAM_ID, &user2.id).await;

    th.app
        .reset_user_password(&user1.username, "another-Pa$$word")
        .await
        .expect("the password is set");

    assert_eq!(
        socket1.next(PATIENCE).await,
        Event::Closed,
        "the connection of the user whose password was set"
    );
    th.app
        .ws()
        .broadcast_category_change(&category(ROOT_TEAM_ID, &user2.id, "still listening"));
    let message = socket2
        .next_json(PATIENCE)
        .await
        .expect("the other user's connection still gets messages");
    assert_eq!(message["action"], json!("UPDATE_CATEGORY"), "{message}");
    th.teardown().await;
}
