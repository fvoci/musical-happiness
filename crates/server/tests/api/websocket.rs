//! The websocket endpoint over a real connection: authentication, subscriptions, who is
//! told about which change, and pings. The original has no tests at this level; its unit
//! tests of the listener table (`server/ws/server_test.go`) are ported next to the hub.

use std::collections::BTreeSet;
use std::path::Path;
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
    assert_eq!(message["category"]["userId"], json!(user1.id), "{message}");
    assert!(message.get("boardCategories").is_none(), "{message}");

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
    // The server reads the two messages in its own time, and a change that is sent
    // before it has read them would be missed by any connection. So the change is sent
    // again and again, and a connection of the user, which subscribes after the other
    // one sent its messages, shows that the changes do arrive where they belong.
    let mut signed_in = Socket::connect_as(&th.client).await;
    subscribe_to_team(&th, &mut signed_in, ROOT_TEAM_ID, &user1.id).await;
    let change = category(ROOT_TEAM_ID, &user1.id, "Projects");
    for round in 0..5 {
        th.app.ws().broadcast_category_change(&change);
        let heard = signed_in.next_json(Duration::from_secs(5)).await;
        assert_eq!(
            heard.expect("the user's connection hears the change")["action"],
            json!("UPDATE_CATEGORY"),
            "round {round}"
        );
        assert_eq!(
            socket.next(Duration::from_millis(50)).await,
            Event::Nothing,
            "round {round}: the connection is nobody's: it hears nothing, and it is not \
             closed either"
        );
    }
    assert_eq!(socket.next(QUIET).await, Event::Nothing);
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

/// A board, a card on it, and the read token the board is shared under.
async fn shared_board_with_a_card(th: &TestServer, user_id: &str) -> (Board, Block, String) {
    let mut board = Board {
        id: new_id(),
        team_id: ROOT_TEAM_ID.to_owned(),
        board_type: BoardType::Private,
        ..Board::default()
    };
    th.app
        .store()
        .insert_board_with_admin(&mut board, user_id)
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
        .insert_block(&mut block, user_id)
        .await
        .expect("the block is stored");
    let token = th.share(&board.id).await;
    (board, block, token)
}

/// Subscribes a connection without a session to a block with a read token and waits
/// until a change of the block comes through; returns that message.
async fn follow_block(th: &TestServer, block: &Block, read_token: &str) -> (Socket, Value) {
    let mut reader = Socket::connect(th).await;
    reader
        .send(json!({
            "action": "SUBSCRIBE_BLOCKS",
            "teamId": ROOT_TEAM_ID,
            "readToken": read_token,
            "blockIds": [block.id],
        }))
        .await;
    for _ in 0..50 {
        th.app
            .ws()
            .broadcast_block_change(ROOT_TEAM_ID, block)
            .await;
        if let Some(message) = reader.next_json(Duration::from_millis(100)).await {
            // Later broadcasts may still be queued; let them drain.
            while reader.next_json(Duration::from_millis(150)).await.is_some() {}
            return (reader, message);
        }
    }
    panic!("the reader was never told about the block");
}

/// Whoever follows a board by its read token alone is not told who made or changed a
/// block, over the websocket as over HTTP. A member is.
#[tokio::test]
async fn a_reader_by_read_token_is_not_told_who_wrote_a_block() {
    let th = Setup::local_mode().start().await.init_basic().await;
    let user1 = th.client.me().await;
    let (_board, block, token) = shared_board_with_a_card(&th, &user1.id).await;
    assert_eq!(block.created_by, user1.id, "the block has an author");

    let (_reader, message) = follow_block(&th, &block, &token).await;
    assert_eq!(message["block"]["id"], json!(block.id), "{message}");
    assert_eq!(message["block"]["createdBy"], json!(""), "{message}");
    assert_eq!(message["block"]["modifiedBy"], json!(""), "{message}");

    let mut member = Socket::connect_as(&th.client).await;
    subscribe_to_team(&th, &mut member, ROOT_TEAM_ID, &user1.id).await;
    th.app
        .ws()
        .broadcast_block_change(ROOT_TEAM_ID, &block)
        .await;
    let message = member
        .next_json(PATIENCE)
        .await
        .expect("the member is told");
    assert_eq!(message["block"]["createdBy"], json!(user1.id), "{message}");

    th.teardown().await;
}

/// A read token that stops opening its board stops feeding connections: turning
/// sharing off, sharing under a new token and deleting the board each end the
/// connections that followed the board with the old link.
#[tokio::test]
async fn a_read_token_that_no_longer_opens_its_board_no_longer_feeds_a_connection() {
    let th = Setup::local_mode().start().await.init_basic().await;
    let user1 = th.client.me().await;
    let sharing = |board_id: &str| format!("/boards/{board_id}/sharing");

    // Sharing turned off.
    let (board, block, token) = shared_board_with_a_card(&th, &user1.id).await;
    let (mut reader, _) = follow_block(&th, &block, &token).await;
    // Sharing what is shared changes nothing for who follows it.
    th.client
        .post(&sharing(&board.id), &json!({"enabled": true}))
        .await
        .check_ok();
    assert_eq!(reader.next(QUIET).await, Event::Nothing, "still connected");
    th.client
        .post(&sharing(&board.id), &json!({"enabled": false}))
        .await
        .check_ok();
    assert_eq!(reader.next(PATIENCE).await, Event::Closed, "sharing off");

    // A new token: the old link is dead, the new one is followed.
    let (board, block, token) = shared_board_with_a_card(&th, &user1.id).await;
    let (mut reader, _) = follow_block(&th, &block, &token).await;
    let renewed = th
        .client
        .post(
            &sharing(&board.id),
            &json!({"enabled": true, "regenerate": true}),
        )
        .await
        .value()["token"]
        .as_str()
        .expect("the new token is answered")
        .to_owned();
    assert_eq!(reader.next(PATIENCE).await, Event::Closed, "a new token");
    let (mut with_new_link, _) = follow_block(&th, &block, &renewed).await;

    // The board deleted: its sharing went with it.
    th.client
        .delete(&format!("/boards/{}", board.id))
        .await
        .check_no_content();
    assert_eq!(
        with_new_link.next(PATIENCE).await,
        Event::Closed,
        "the board is gone"
    );

    th.teardown().await;
}

/// A session that ends takes its websocket connections with it: logging out closes the
/// connection of that session and leaves the user's other session connected.
#[tokio::test]
async fn logging_out_closes_the_websocket_connections_of_that_session() {
    let th = Setup::native().start().await.init_basic().await;
    let user1 = th.client.me().await;
    let elsewhere = th
        .login(crate::common::USER1_USERNAME, crate::common::PASSWORD)
        .await;
    let mut here = Socket::connect_as(&th.client).await;
    subscribe_to_team(&th, &mut here, ROOT_TEAM_ID, &user1.id).await;
    let mut there = Socket::connect_as(&elsewhere).await;

    th.client.post_empty("/logout").await.check_no_content();

    assert_eq!(
        here.next(PATIENCE).await,
        Event::Closed,
        "the session ended"
    );
    assert_eq!(
        there.next(QUIET).await,
        Event::Nothing,
        "the user's other session stays connected"
    );
    th.teardown().await;
}

/// Checks a value against a member's description in `contracts/websocket.json`: a JSON
/// type, a schema of `contracts/openapi.json` by name, an inline object, or a list.
fn check_against(openapi: &Value, what: &Value, value: &Value, at: &str) {
    match what {
        Value::String(reference) if let Some(name) = reference.strip_prefix('#') => {
            let schema = &openapi["components"]["schemas"][name];
            let properties = schema["properties"]
                .as_object()
                .unwrap_or_else(|| panic!("{at}: openapi.json has the schema {name}"));
            let object = value
                .as_object()
                .unwrap_or_else(|| panic!("{at}: an object, not {value}"));
            for key in object.keys() {
                assert!(
                    properties.contains_key(key),
                    "{at}: `{key}` is no member of {name} in openapi.json: {value}"
                );
            }
            for key in schema["required"].as_array().into_iter().flatten() {
                let key = key.as_str().expect("required names are strings");
                assert!(
                    object.contains_key(key),
                    "{at}: {name} has `{key}`: {value}"
                );
            }
        }
        Value::String(kind) => {
            let matches = match kind.as_str() {
                "string" => value.is_string(),
                "boolean" => value.is_boolean(),
                "integer" => value.is_i64() || value.is_u64(),
                other => panic!("{at}: unknown type {other} in websocket.json"),
            };
            assert!(matches, "{at}: a {kind}, not {value}");
        }
        Value::Array(items) => {
            let [item] = items.as_slice() else {
                panic!("{at}: a list is described by one item");
            };
            let list = value
                .as_array()
                .unwrap_or_else(|| panic!("{at}: a list, not {value}"));
            for (index, element) in list.iter().enumerate() {
                check_against(openapi, item, element, &format!("{at}[{index}]"));
            }
        }
        Value::Object(members) => {
            let object = value
                .as_object()
                .unwrap_or_else(|| panic!("{at}: an object, not {value}"));
            let expected: BTreeSet<&String> = members.keys().collect();
            let actual: BTreeSet<&String> = object.keys().collect();
            assert_eq!(actual, expected, "{at}: the members of {value}");
            for (key, member) in members {
                check_against(openapi, member, &object[key], &format!("{at}.{key}"));
            }
        }
        other => panic!("{at}: cannot read {other} in websocket.json"),
    }
}

/// Checks one message the server sent against `contracts/websocket.json`.
fn check_message(contract: &Value, openapi: &Value, message: &Value) {
    let action = message["action"].as_str().expect("a message has an action");
    let members = &contract["messages"][action]["members"];
    assert!(
        members.is_object(),
        "contracts/websocket.json lists {action}"
    );
    check_against(openapi, members, message, action);
}

fn read_contract(name: &str) -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../contracts")
        .join(name);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("reading {}: {error}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

/// Checks the messages a socket hears against the websocket contract, and remembers
/// which kinds it heard.
struct ContractCheck<'contract> {
    contract: &'contract Value,
    openapi: &'contract Value,
    heard: BTreeSet<String>,
}

impl ContractCheck<'_> {
    /// Waits for the next message, which must be an `action`, and checks it.
    async fn hear(&mut self, socket: &mut Socket, action: &str) {
        let message = socket.next_json(PATIENCE).await.expect("the message comes");
        assert_eq!(message["action"], json!(action), "{message}");
        check_message(self.contract, self.openapi, &message);
        self.heard.insert(action.to_owned());
    }
}

/// A private board of the user's with one card, stored.
async fn board_with_a_card(th: &TestServer, user_id: &str) -> (Board, Block) {
    let mut board = Board {
        id: new_id(),
        team_id: ROOT_TEAM_ID.to_owned(),
        board_type: BoardType::Private,
        title: "Roadmap".to_owned(),
        ..Board::default()
    };
    th.app
        .store()
        .insert_board_with_admin(&mut board, user_id)
        .await
        .expect("the board is stored");
    let mut block = Block {
        id: new_id(),
        board_id: board.id.clone(),
        block_type: BlockType::Card,
        title: "A card".to_owned(),
        ..Block::default()
    };
    th.app
        .store()
        .insert_block(&mut block, user_id)
        .await
        .expect("the block is stored");
    (board, block)
}

/// Every message the server sends has exactly the members `contracts/websocket.json`
/// lists for it, and what it carries has the members of its schema in
/// `contracts/openapi.json`. The messages come from the hub's broadcasts over a real
/// connection, with stored boards, blocks and categories.
#[tokio::test]
async fn messages_match_the_websocket_contract() {
    let contract = read_contract("websocket.json");
    let openapi = read_contract("openapi.json");
    let th = Setup::native().start().await.init_basic().await;
    let user1 = th.client.me().await;

    let (board, block) = board_with_a_card(&th, &user1.id).await;
    let members = th
        .app
        .store()
        .get_members_for_board(&board.id)
        .await
        .expect("the members are read");

    let mut socket = Socket::connect_as(&th.client).await;
    subscribe_to_team(&th, &mut socket, ROOT_TEAM_ID, &user1.id).await;

    let ws = th.app.ws();
    let category = category(ROOT_TEAM_ID, &user1.id, "Work");
    let mut contract_check = ContractCheck {
        contract: &contract,
        openapi: &openapi,
        heard: BTreeSet::new(),
    };

    ws.broadcast_board_change(ROOT_TEAM_ID, &board).await;
    contract_check.hear(&mut socket, "UPDATE_BOARD").await;
    ws.broadcast_block_change(ROOT_TEAM_ID, &block).await;
    contract_check.hear(&mut socket, "UPDATE_BLOCK").await;
    ws.broadcast_block_delete(ROOT_TEAM_ID, &block.id, &board.id)
        .await;
    contract_check.hear(&mut socket, "UPDATE_BLOCK").await;
    let admin = members
        .first()
        .expect("the board has its admin as a member");
    ws.broadcast_member_change(ROOT_TEAM_ID, &board.id, admin)
        .await;
    contract_check.hear(&mut socket, "UPDATE_MEMBER").await;
    ws.broadcast_member_delete(ROOT_TEAM_ID, &board.id, &user1.id)
        .await;
    contract_check.hear(&mut socket, "DELETE_MEMBER").await;
    ws.broadcast_category_change(&category);
    contract_check.hear(&mut socket, "UPDATE_CATEGORY").await;
    ws.broadcast_category_board_change(
        ROOT_TEAM_ID,
        &user1.id,
        &[fvoci_model::BoardCategoryWebsocketData {
            board_id: board.id.clone(),
            category_id: category.id.clone(),
            hidden: false,
        }],
    );
    contract_check
        .hear(&mut socket, "UPDATE_BOARD_CATEGORY")
        .await;
    ws.broadcast_category_reorder(ROOT_TEAM_ID, &user1.id, std::slice::from_ref(&category.id));
    contract_check.hear(&mut socket, "REORDER_CATEGORIES").await;
    ws.broadcast_category_boards_reorder(
        ROOT_TEAM_ID,
        &user1.id,
        &category.id,
        std::slice::from_ref(&board.id),
    );
    contract_check
        .hear(&mut socket, "REORDER_CATEGORY_BOARDS")
        .await;
    ws.broadcast_config_change(&th.app.client_config());
    contract_check
        .hear(&mut socket, "UPDATE_CLIENT_CONFIG")
        .await;
    // The board deletion goes to the former members, with the board's identifiers only.
    ws.broadcast_board_delete(ROOT_TEAM_ID, &board.id, &members);
    contract_check.hear(&mut socket, "UPDATE_BOARD").await;

    // Every message of the contract was heard.
    let listed: BTreeSet<&String> = contract["messages"]
        .as_object()
        .expect("the messages are an object")
        .keys()
        .collect();
    let heard: BTreeSet<&String> = contract_check.heard.iter().collect();
    assert_eq!(
        heard, listed,
        "the test hears every message the contract lists"
    );

    th.teardown().await;
}
