//! The websocket endpoint over a real connection: authentication, subscriptions, who is
//! told about which change, and pings. The original has no tests at this level; its unit
//! tests of the listener table (`server/ws/server_test.go`) are ported next to the hub.

use std::collections::BTreeSet;
use std::path::Path;
use std::time::Duration;

use futures_util::{SinkExt as _, StreamExt as _};
use fvoci_model::{Block, BlockType, Board, BoardPatch, BoardType, Category, new_id};
use serde_json::{Value, json};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

use crate::common::{Client, HIDDEN_TEAM_ID, ROOT_TEAM_ID, Setup, TEST_TEAM_ID, TestServer};

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

/// The category messages go to every connection of the user that follows the team:
/// another tab of the user hears them too, another user does not.
#[tokio::test]
async fn category_messages_reach_every_tab_of_the_user() {
    let th = Setup::native().start().await.init_basic().await;
    let user1 = th.client.me().await;
    let mut first = Socket::connect_as(&th.client).await;
    subscribe_to_team(&th, &mut first, ROOT_TEAM_ID, &user1.id).await;
    let mut second = Socket::connect_as(&th.client).await;
    subscribe_to_team(&th, &mut second, ROOT_TEAM_ID, &user1.id).await;
    while first.next_json(Duration::from_millis(150)).await.is_some() {}
    let user2 = th.client2.me().await;
    let mut other = Socket::connect_as(&th.client2).await;
    subscribe_to_team(&th, &mut other, ROOT_TEAM_ID, &user2.id).await;

    th.client
        .post(
            &format!("/teams/{ROOT_TEAM_ID}/categories"),
            &json!({"name": "both tabs", "userId": user1.id, "teamId": ROOT_TEAM_ID}),
        )
        .await
        .check_created();
    for socket in [&mut first, &mut second] {
        let message = socket.next_json(PATIENCE).await.expect("told");
        assert_eq!(message["action"], json!("UPDATE_CATEGORY"), "{message}");
        assert_eq!(message["category"]["name"], json!("both tabs"));
    }
    assert_eq!(other.next(QUIET).await, Event::Nothing, "another user");
    th.teardown().await;
}

/// A connection follows the teams its user works in. The subscription to a team the
/// user is not in is ignored, as is that of a guest of the workspace: neither is told
/// what goes to who follows the team.
#[tokio::test]
async fn a_team_is_followed_by_who_works_in_it() {
    let th = Setup::native().start().await.init_basic().await;
    let user1 = th.client.me().await;
    let user2 = th.client2.me().await;
    th.app
        .store()
        .execute_unchecked(&format!(
            "UPDATE workspace_members SET role = 'guest' \
             WHERE user_id = '{}' AND workspace_id = '{ROOT_TEAM_ID}'",
            user2.id
        ))
        .await
        .expect("the second user becomes a guest of the root team");

    // The subscription that works comes last on each connection: once it is seen to
    // work, the server has gone through the one before it.
    let mut outsider = Socket::connect_as(&th.client).await;
    outsider
        .send(json!({"action": "SUBSCRIBE_TEAM", "teamId": HIDDEN_TEAM_ID}))
        .await;
    subscribe_to_team(&th, &mut outsider, ROOT_TEAM_ID, &user1.id).await;
    let mut guest = Socket::connect_as(&th.client2).await;
    guest
        .send(json!({"action": "SUBSCRIBE_TEAM", "teamId": ROOT_TEAM_ID}))
        .await;
    subscribe_to_team(&th, &mut guest, TEST_TEAM_ID, &user2.id).await;

    th.app
        .ws()
        .broadcast_category_change(&category(HIDDEN_TEAM_ID, &user1.id, "hidden"));
    th.app
        .ws()
        .broadcast_category_change(&category(ROOT_TEAM_ID, &user2.id, "a guest's"));
    assert_eq!(
        outsider.next(QUIET).await,
        Event::Nothing,
        "not in the team"
    );
    assert_eq!(
        guest.next(QUIET).await,
        Event::Nothing,
        "a guest of the team"
    );
    th.teardown().await;
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
    subscribe_to_team(&th, &mut socket, TEST_TEAM_ID, &user1.id).await;
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
    follow_block_on(th, Socket::connect(th).await, block, read_token).await
}

/// [`follow_block`] on the given connection.
async fn follow_block_on(
    th: &TestServer,
    mut reader: Socket,
    block: &Block,
    read_token: &str,
) -> (Socket, Value) {
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

/// Why a connection ended is counted in the metrics: here the read token stopped
/// opening its board, and a message over the limit broke a connection off.
#[tokio::test]
async fn why_a_connection_ended_is_counted() {
    use crate::metrics::{sample, scrape_until};

    let th = Setup::local_mode()
        .metrics_at("127.0.0.1:0")
        .start()
        .await
        .init_basic()
        .await;
    let user1 = th.client.me().await;
    let (board, block, token) = shared_board_with_a_card(&th, &user1.id).await;
    let (mut reader, _) = follow_block(&th, &block, &token).await;
    th.app.ws().close_readers_of_board(&board.id);
    assert_eq!(
        reader.next(PATIENCE).await,
        Event::Closed,
        "the share ended"
    );

    let mut talker = Socket::connect(&th).await;
    // The server may close before the whole message is written.
    let _ = talker
        .stream
        .send(Message::Text("x".repeat(64 * 1024 + 1).into()))
        .await;
    assert_eq!(talker.next(PATIENCE).await, Event::Closed, "over the limit");

    let ended = |body: &str, reason: &str| {
        sample(
            body,
            "fvoci_websocket_disconnections_total",
            &[("reason", reason)],
        )
    };
    let body = scrape_until(&th, |body| {
        ended(body, "share_ended") == Some(1.0) && ended(body, "read_error") == Some(1.0)
    })
    .await;
    assert_eq!(ended(&body, "share_ended"), Some(1.0), "{body}");
    assert_eq!(ended(&body, "read_error"), Some(1.0), "{body}");
    assert_eq!(
        ended(&body, "slow_consumer"),
        Some(0.0),
        "there from the start: {body}"
    );
    th.teardown().await;
}

/// Whoever follows a board by its read token alone is not told who made or changed a
/// block, over the websocket as over HTTP. A member is.
#[tokio::test]
async fn a_reader_by_read_token_is_not_told_who_wrote_a_block() {
    let th = Setup::local_mode().start().await.init_basic().await;
    let user1 = th.client.me().await;
    let (_board, mut block, token) = shared_board_with_a_card(&th, &user1.id).await;
    assert_eq!(block.created_by, user1.id, "the block has an author");
    block.body = fvoci_model::Body::parse(json!({"type": "doc", "content": [
        {"type": "paragraph", "content": [{"type": "mention", "attrs": {"userId": user1.id}}]},
    ]}))
    .expect("a body");
    let mention = "/block/body/content/0/content/0/attrs/userId";

    let (mut reader, message) = follow_block(&th, &block, &token).await;
    assert_eq!(message["block"]["id"], json!(block.id), "{message}");
    assert_eq!(message["block"]["createdBy"], json!(""), "{message}");
    assert_eq!(message["block"]["modifiedBy"], json!(""), "{message}");
    assert_eq!(
        message.pointer(mention),
        Some(&json!("")),
        "nor who is mentioned"
    );

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
    assert_eq!(
        message.pointer(mention),
        Some(&json!(user1.id)),
        "{message}"
    );
    // The same broadcast, as the reader has it: what the member is told does not reach
    // the reader, nor the reverse.
    let message = reader
        .next_json(PATIENCE)
        .await
        .expect("the reader is told too");
    assert_eq!(message["block"]["createdBy"], json!(""), "{message}");
    assert_eq!(message.pointer(mention), Some(&json!("")), "{message}");

    // A signed-in user who is no member of the board follows it as the token's reader:
    // nor are they told who the values of the board's person properties name.
    let definitions = json!([
        {"id": "owner", "name": "Owner", "type": "person"},
        {"id": "team", "name": "Team", "type": "multiPerson"},
    ]);
    let patch: BoardPatch = serde_json::from_value(json!({"updatedCardProperties": definitions}))
        .expect("a board patch");
    th.app
        .store()
        .patch_board(&block.board_id, &patch, &user1.id)
        .await
        .expect("the board is stored");
    block.fields.insert(
        "properties".to_owned(),
        json!({"owner": user1.id, "team": [user1.id, user1.id]}),
    );
    let stranger = Socket::connect_as(&th.client2).await;
    let (_stranger, message) = follow_block_on(&th, stranger, &block, &token).await;
    assert_eq!(message["block"]["createdBy"], json!(""), "{message}");
    assert_eq!(message.pointer(mention), Some(&json!("")), "{message}");
    assert_eq!(
        message["block"]["fields"]["properties"],
        json!({"owner": "", "team": ["", ""]}),
        "{message}"
    );

    // A member who follows the block by the token, with no team subscription, is told
    // all of it.
    let member = Socket::connect_as(&th.client).await;
    let (_member, message) = follow_block_on(&th, member, &block, &token).await;
    assert_eq!(message["block"]["createdBy"], json!(user1.id), "{message}");
    assert_eq!(
        message.pointer(mention),
        Some(&json!(user1.id)),
        "{message}"
    );
    assert_eq!(
        message["block"]["fields"]["properties"],
        json!({"owner": user1.id, "team": [user1.id, user1.id]}),
        "{message}"
    );

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

    // The board deleted with others at once, as `deleteBoardsAndBlocks` does.
    let (board, block, token) = shared_board_with_a_card(&th, &user1.id).await;
    let (mut reader, _) = follow_block(&th, &block, &token).await;
    th.client
        .delete_json(
            "/boards-and-blocks",
            &json!({"boards": [board.id], "blocks": [block.id]}),
        )
        .await
        .check_no_content();
    assert_eq!(
        reader.next(PATIENCE).await,
        Event::Closed,
        "the board is gone with others"
    );

    th.teardown().await;
}

/// A session that ends by its idle limit, which nobody announces, takes its websocket
/// connection with it at the next ping (the hourly clean-up of ended sessions is not
/// waited for); a session in use keeps its connection.
#[tokio::test]
async fn a_session_that_runs_out_closes_its_websocket_connection() {
    let th = Setup::native()
        .configure(|config| {
            config.websocket_ping_seconds = 1;
            config.session_idle_days = 3;
        })
        .start()
        .await
        .init_basic()
        .await;
    let user1 = th.client.me().await;
    let elsewhere = th
        .login(crate::common::USER1_USERNAME, crate::common::PASSWORD)
        .await;
    let mut here = Socket::connect_as(&th.client).await;
    subscribe_to_team(&th, &mut here, ROOT_TEAM_ID, &user1.id).await;
    let mut there = Socket::connect_as(&elsewhere).await;
    let session = th
        .app
        .store()
        .get_session(
            &fvoci_server::auth::secret::token_digest(&th.client.token),
            0,
            i64::MIN,
        )
        .await
        .expect("the session is stored");
    let three_days = 3 * 24 * 60 * 60 * 1000;
    th.app
        .store()
        .set_session_last_used(
            &session.id,
            fvoci_server::auth::now_millis() - three_days - 60_000,
        )
        .await
        .expect("the last use is moved back");

    let until = std::time::Instant::now() + PATIENCE;
    let mut ended = Event::Nothing;
    while std::time::Instant::now() < until {
        ended = here.next(PATIENCE).await;
        if ended != Event::Ping {
            break;
        }
    }
    assert_eq!(ended, Event::Closed, "the session ran out");
    let until = std::time::Instant::now() + Duration::from_millis(2500);
    while std::time::Instant::now() < until {
        assert_eq!(
            there.next(PATIENCE).await,
            Event::Ping,
            "the user's other session stays connected"
        );
    }
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

/// A private board of the root team with the text properties `a`, `b` and `c`, in that
/// order, and `admin` as its admin.
async fn board_with_three_properties(th: &TestServer, admin: &str) -> Board {
    let property = |id: &str| {
        json!({"id": id, "name": id, "type": "text", "options": []})
            .as_object()
            .expect("an object")
            .clone()
    };
    let mut board = Board {
        id: new_id(),
        team_id: ROOT_TEAM_ID.to_owned(),
        board_type: BoardType::Private,
        card_properties: vec![property("a"), property("b"), property("c")],
        ..Board::default()
    };
    th.app
        .store()
        .insert_board_with_admin(&mut board, admin)
        .await
        .expect("the board is stored with the admin");
    board
}

/// Not in the original (user decision 2026-11-11 14:25 KST). The card properties of a
/// board are put in a new order by naming all of them: the answer is the order, the
/// board has it, and the listeners hear `UPDATE_BOARD`. An order that leaves one out,
/// names one twice or names one the board does not have is 400 and changes nothing; a
/// commenter may not, nor a stranger.
#[tokio::test]
async fn card_properties_are_put_in_order_and_the_board_is_told() {
    let th = Setup::native().start().await.init_basic().await;
    let user1 = th.client.me().await;
    let user2 = th.client2.me().await;
    let editor_client = th.add_user("editor").await;
    let editor = editor_client.me().await;
    let board = board_with_three_properties(&th, &user1.id).await;
    for (user, role) in [(&user2.id, "commenter"), (&editor.id, "editor")] {
        th.client
            .add_member(&fvoci_model::BoardMember {
                board_id: board.id.clone(),
                user_id: user.clone(),
                scheme_commenter: true,
                scheme_editor: role == "editor",
                ..fvoci_model::BoardMember::default()
            })
            .await
            .check_ok();
    }
    let path = format!("/boards/{}/property-order", board.id);
    let ids = |board: &Value| -> Vec<Value> {
        board["cardProperties"]
            .as_array()
            .expect("properties")
            .iter()
            .map(|property| property["id"].clone())
            .collect()
    };
    let mut socket = Socket::connect_as(&th.client).await;
    subscribe_to_team(&th, &mut socket, ROOT_TEAM_ID, &user1.id).await;

    let answered: Vec<String> = th.client.put(&path, &["c", "a", "b"]).await.ok_json();
    assert_eq!(answered, ["c", "a", "b"]);
    let message = socket
        .next_json(PATIENCE)
        .await
        .expect("the board's listeners are told");
    assert_eq!(message["action"], json!("UPDATE_BOARD"), "{message}");
    assert_eq!(ids(&message["board"]), [json!("c"), json!("a"), json!("b")]);
    let stored = th
        .client
        .get(&format!("/boards/{}", board.id))
        .await
        .value();
    assert_eq!(ids(&stored), [json!("c"), json!("a"), json!("b")]);
    assert_eq!(stored["modifiedBy"], json!(user1.id), "{stored}");

    // An editor may, as with patchBoard; the board is marked as changed by them.
    let answered: Vec<String> = editor_client.put(&path, &["b", "c", "a"]).await.ok_json();
    assert_eq!(answered, ["b", "c", "a"]);
    let by_editor = th
        .client
        .get(&format!("/boards/{}", board.id))
        .await
        .value();
    assert_eq!(by_editor["modifiedBy"], json!(editor.id), "{by_editor}");
    assert!(
        by_editor["updateAt"].as_i64() >= stored["updateAt"].as_i64(),
        "{by_editor}"
    );
    socket.next_json(PATIENCE).await.expect("told again");

    for order in [
        json!(["c", "a"]),
        json!(["c", "a", "b", "b"]),
        json!(["c", "a", "b", "d"]),
        json!(["c", "a", "d"]),
        json!({"order": ["a"]}),
    ] {
        th.client.put(&path, &order).await.check_bad_request();
    }
    th.client2
        .put(&path, &["a", "b", "c"])
        .await
        .check_status(reqwest::StatusCode::FORBIDDEN);
    let stranger = th.add_user("stranger").await;
    stranger
        .put(&path, &["a", "b", "c"])
        .await
        .check_status(reqwest::StatusCode::NOT_FOUND);
    let stored = th
        .client
        .get(&format!("/boards/{}", board.id))
        .await
        .value();
    assert_eq!(
        ids(&stored),
        [json!("b"), json!("c"), json!("a")],
        "nothing refused changed it"
    );
}
