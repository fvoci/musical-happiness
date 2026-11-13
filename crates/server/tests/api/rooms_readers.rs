//! A reader by a shared board's link is kept out of the rooms, and what the rooms
//! write to a card reaches the reader naming nobody.

use serde_json::{Value, json};

use crate::common::{Setup, TEST_WORKSPACE_ID};
use crate::rooms::{Peer, table};
use crate::websocket::{PATIENCE, Socket};

/// The message of `socket` that tells of `card_id` with `text` in its body.
async fn told_of(socket: &mut Socket, card_id: &str, text: &str) -> Value {
    let deadline = tokio::time::Instant::now() + PATIENCE;
    loop {
        let wait = deadline.saturating_duration_since(tokio::time::Instant::now());
        let message = socket
            .next_json(wait)
            .await
            .unwrap_or_else(|| panic!("the reader is told of the body with {text:?}"));
        if message["action"] == "UPDATE_BLOCK"
            && message["block"]["id"] == card_id
            && message["block"]["body"].to_string().contains(text)
        {
            return message;
        }
    }
}

/// The connection of a shared board's reader, with or without a session, is refused
/// the card's room as if the card were not there. What a member types in the room
/// reaches the reader as the card's body (over the subscription and over HTTP) with
/// its mention naming nobody.
#[tokio::test]
async fn a_reader_by_link_is_kept_out_of_the_room_and_told_its_body_naming_nobody() {
    let th = Setup::local_mode().start().await.init_basic().await;
    let me = th.client.me().await.id;
    let body = json!({"type": "doc", "content": [{"type": "paragraph", "content": [
        {"type": "text", "text": "ask "},
        {"type": "mention", "attrs": {"userId": me}},
    ]}]});
    let t = table(&th, &body).await;
    let token = th.share(&t.board_id).await;

    let mut readers = [
        Socket::connect(&th).await,
        Socket::connect_as(&th.client2).await,
    ];
    for reader in &mut readers {
        reader
            .send(json!({
                "action": "SUBSCRIBE_BLOCKS", "workspaceId": TEST_WORKSPACE_ID,
                "readToken": token, "blockIds": [t.card_id],
            }))
            .await;
        reader
            .send(json!({
                "action": "JOIN_DOC", "workspaceId": TEST_WORKSPACE_ID,
                "cardId": t.card_id, "readToken": token,
            }))
            .await;
        // Answered after the subscription: the reader follows the card from here on.
        assert_eq!(
            reader.next_json(PATIENCE).await,
            Some(json!({
                "action": "DOC_REFUSED", "workspaceId": TEST_WORKSPACE_ID,
                "cardId": t.card_id, "reason": "not_found",
            }))
        );
    }

    let mut writer = Peer::connect(&th.client, 101).await;
    writer.joined(TEST_WORKSPACE_ID, &t.card_id).await;
    writer.write_acked("typed in the room").await;
    assert_eq!(th.app.collab().room_members(&th.app, &t.card_id).await, 1);
    th.app
        .collab()
        .write_body(&th.app, &t.card_id)
        .await
        .expect("the room's body is written");

    let mention = "/block/body/content/0/content/1/attrs/userId";
    for reader in &mut readers {
        let told = told_of(reader, &t.card_id, "typed in the room").await;
        assert_eq!(told.pointer(mention), Some(&json!("")), "{told}");
        assert!(!told.to_string().contains(&me), "nobody is named: {told}");
    }
    let read: Vec<Value> = th
        .anonymous()
        .get(&format!(
            "/shared/{}/blocks?blockId={}&readToken={token}",
            t.board_id, t.card_id
        ))
        .await
        .ok_json();
    let shown = read[0]["body"].to_string();
    assert!(shown.contains("typed in the room"), "{shown}");
    assert!(!shown.contains(&me), "nobody is named: {shown}");
    assert_eq!(
        read[0]["body"]["content"][0]["content"][1]["attrs"]["userId"],
        json!("")
    );
    th.teardown().await;
}
