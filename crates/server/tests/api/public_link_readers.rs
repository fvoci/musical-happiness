//! How many connections may read one shared board by its public link, in all and from
//! one address: one more is told `UNSUBSCRIBED_BLOCKS` and is not counted among the
//! board's anonymous readers; the members' own connections are not held to it.

use serde_json::{Value, json};

use crate::common::{Setup, TEST_WORKSPACE_ID, TestServer};
use crate::rooms::{paragraphs, table};
use crate::websocket::{PATIENCE, QUIET, Socket};

/// A server that believes the addresses its local proxy names, with these limits.
async fn server(limits: fn(&mut fvoci_server::config::Configuration)) -> TestServer {
    // Local mode: public links are on.
    Setup::local_mode()
        .configure(|config| {
            config.trusted_proxies = vec!["127.0.0.1".to_owned(), "::1".to_owned()];
        })
        .configure(limits)
        .start()
        .await
        .init_basic()
        .await
}

/// A board with a card, shared: the board, the card and the link's read token.
async fn shared(th: &TestServer) -> (String, String, String) {
    let t = table(th, &paragraphs(&["hello"])).await;
    let token = th.share(&t.board_id).await;
    (t.board_id, t.card_id, token)
}

/// A connection from `address` that reads the board by its link: follows the card and
/// visits the board. What it is told of its subscription within a while, if anything.
async fn reader(
    th: &TestServer,
    address: &str,
    (board_id, card_id, token): &(String, String, String),
) -> (Socket, Option<Value>) {
    let mut socket = Socket::connect_from(th, address).await;
    socket
        .send(json!({
            "action": "SUBSCRIBE_BLOCKS", "workspaceId": TEST_WORKSPACE_ID,
            "blockIds": [card_id], "readToken": token,
        }))
        .await;
    socket
        .send(json!({
            "action": "SET_PRESENCE", "workspaceId": TEST_WORKSPACE_ID,
            "boardId": board_id, "readToken": token,
        }))
        .await;
    let told = socket.unsubscribed_message(QUIET).await;
    (socket, told)
}

/// The board's member, on the board: what they are told of its anonymous readers.
async fn member_on(th: &TestServer, board_id: &str) -> Socket {
    let mut member = Socket::connect_as(&th.client).await;
    member
        .send(json!({
            "action": "SET_PRESENCE", "workspaceId": TEST_WORKSPACE_ID, "boardId": board_id,
        }))
        .await;
    member
}

/// The anonymous count of the latest roster the member is sent, once it is `count`.
async fn anonymous_reaches(member: &mut Socket, count: u64) {
    let mut last = Value::Null;
    for _ in 0..20 {
        let Some(message) = member.next_json(PATIENCE).await else {
            break;
        };
        if message["action"] == "UPDATE_PRESENCE" {
            if message["anonymousCount"] == count {
                return;
            }
            last = message;
        }
    }
    panic!("the anonymous count never reached {count}; last roster {last}");
}

fn is_refusal(told: Option<&Value>) -> bool {
    told.is_some_and(|told| {
        told["action"] == "UNSUBSCRIBED_BLOCKS" && told["reason"] == "public link connections full"
    })
}

#[tokio::test]
async fn one_address_reads_a_board_up_to_its_limit_and_others_go_on() {
    let th = server(|config| {
        config.public_link_connections_per_board = 5;
        config.public_link_connections_per_address = 2;
    })
    .await;
    let board = shared(&th).await;
    let mut member = member_on(&th, &board.0).await;
    let mut readers = Vec::new();
    for _ in 0..2 {
        let (socket, told) = reader(&th, "203.0.113.7", &board).await;
        assert_eq!(told, None, "a reader within the limits is taken");
        readers.push(socket);
    }
    anonymous_reaches(&mut member, 2).await;
    let (_third, told) = reader(&th, "203.0.113.7", &board).await;
    assert!(
        is_refusal(told.as_ref()),
        "the third from one address: {told:?}"
    );
    assert!(
        member.next_json(QUIET).await.is_none(),
        "the refused reader is not counted"
    );
    let (_elsewhere, told) = reader(&th, "198.51.100.1", &board).await;
    assert_eq!(told, None, "another address is taken");
    anonymous_reaches(&mut member, 3).await;
    th.teardown().await;
}

#[tokio::test]
async fn a_board_reads_up_to_its_limit_and_takes_one_again_when_one_leaves() {
    let th = server(|config| {
        config.public_link_connections_per_board = 2;
        config.public_link_connections_per_address = 10;
    })
    .await;
    let board = shared(&th).await;
    let mut member = member_on(&th, &board.0).await;
    let (first, told) = reader(&th, "192.0.2.1", &board).await;
    assert_eq!(told, None);
    let (_second, told) = reader(&th, "192.0.2.2", &board).await;
    assert_eq!(told, None);
    anonymous_reaches(&mut member, 2).await;
    let (_third, told) = reader(&th, "192.0.2.3", &board).await;
    assert!(is_refusal(told.as_ref()), "the board's third: {told:?}");
    // The member's own connection is no reader by the link: still on the board.
    let mut again = member_on(&th, &board.0).await;
    anonymous_reaches(&mut again, 2).await;
    drop(first);
    anonymous_reaches(&mut member, 1).await;
    let (_fourth, told) = reader(&th, "192.0.2.4", &board).await;
    assert_eq!(told, None, "a place freed is taken again");
    anonymous_reaches(&mut member, 2).await;
    th.teardown().await;
}
