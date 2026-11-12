//! The JSON messages of the bodies edited together, as the server sends them over a
//! real connection, against `contracts/websocket.json`.

use std::collections::BTreeSet;

use fvoci_server::collab::Outgoing;
use serde_json::{Value, json};

use crate::common::{Setup, TEST_WORKSPACE_ID};
use crate::rooms::{Peer, UPDATE, frame, give, paragraphs, table};
use crate::websocket::{check_message, read_contract};

/// Every message the server sends about a body edited together has exactly the
/// members `contracts/websocket.json` lists for it.
#[tokio::test]
async fn the_messages_of_the_rooms_match_the_websocket_contract() {
    let contract = read_contract("websocket.json");
    let openapi = read_contract("openapi.json");
    let th = Setup::native().start().await.init_basic().await;
    let t = table(&th, &paragraphs(&["first"])).await;
    let user2 = th.client2.me().await.id;
    give(&th, &t.board_id, &user2, "editor").await;
    let mut heard = BTreeSet::new();
    let mut check = |message: &Value| {
        check_message(&contract, &openapi, message);
        heard.insert(message["action"].as_str().expect("an action").to_owned());
    };

    let mut one = Peer::connect(&th.client, 101).await;
    let mut two = Peer::connect(&th.client2, 102).await;
    check(&one.joined(TEST_WORKSPACE_ID, &t.card_id).await);
    check(&one.join(TEST_WORKSPACE_ID, "no card").await);
    one.write("typed").await;
    check(&one.text("DOC_ACK").await);
    one.send_binary(frame(one.doc, one.epoch, UPDATE, b"no update"))
        .await;
    check(&one.text("DOC_NACK").await);
    one.settle().await;

    // The card's body is written whole over HTTP, which makes its document again;
    // who comes in next is sent the new one, and so is who was there.
    crate::rooms::write_whole(&th.client, &t.card_id, &paragraphs(&["written whole"])).await;
    two.joined(TEST_WORKSPACE_ID, &t.card_id).await;
    let reset = one.text("DOC_RESET").await;
    check(&reset);
    assert_eq!(reset["epoch"], json!(two.epoch), "{reset}");
    assert_ne!(two.epoch, 1, "another document");
    one.settle().await;
    assert_eq!(one.lines(), ["written whole"]);
    assert_eq!(one.body(), two.body());

    // A connection that reads nothing of what it is sent: its place among the rooms
    // is taken from the server itself, and what waits for it is not asked for. With
    // as many messages waiting as may, it is put out, and the others go on.
    let collab = th.app.collab();
    let join = json!({
        "action": "JOIN_DOC", "workspaceId": TEST_WORKSPACE_ID, "cardId": t.card_id,
    })
    .to_string();
    let (mut slow, waiting) = collab.seat(&th.app, &user2);
    assert!(slow.command(&join).await, "a command of the rooms");
    // `DOC_JOINED` and the document wait already: 510 more, and the next is one too many.
    for n in 0..511 {
        one.write_acked(&format!("{n}")).await;
    }
    let Some(Outgoing::Text(left)) = waiting.next().await else {
        panic!("what waited is dropped for the notice");
    };
    let left: Value = serde_json::from_str(&left).expect("JSON");
    check(&left);
    assert_eq!(
        left,
        json!({"action": "DOC_LEFT", "doc": 1, "reason": "connection_too_slow"})
    );
    assert!(!slow.overrun().is_cancelled(), "the connection stays");
    two.settle().await;
    assert_eq!(two.lines().len(), 512, "the others were sent all of it");
    assert_eq!(one.body(), two.body());
    // It comes in again, and is sent the whole document.
    assert!(slow.command(&join).await);
    let Some(Outgoing::Text(joined)) = waiting.next().await else {
        panic!("the answer to JOIN_DOC");
    };
    let joined: Value = serde_json::from_str(&joined).expect("JSON");
    assert_eq!(
        (&joined["action"], &joined["doc"]),
        (&json!("DOC_JOINED"), &json!(2))
    );
    assert!(matches!(waiting.next().await, Some(Outgoing::Binary(_))));
    slow.leave_all().await;

    let listed: BTreeSet<String> = contract["messages"]
        .as_object()
        .expect("the messages are an object")
        .keys()
        .filter(|action| action.starts_with("DOC_"))
        .cloned()
        .collect();
    assert_eq!(
        heard, listed,
        "the test hears the messages of the rooms the contract lists"
    );
    th.teardown().await;
}
