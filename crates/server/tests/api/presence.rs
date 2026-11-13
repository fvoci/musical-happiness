//! Real sockets: board visits, open cards, deduplication, token readers and boundaries.

use fvoci_model::Sharing;
use serde_json::{Value, json};

use crate::common::{Setup, TEST_WORKSPACE_ID, TEST_WS};
use crate::rooms::{give, paragraphs, table};
use crate::websocket::{PATIENCE, QUIET, Socket, check_message, read_contract};

async fn visit(socket: &mut Socket, board: &str, card: &str) {
    socket
        .send(
            json!({"action":"SET_PRESENCE", "workspaceId":TEST_WORKSPACE_ID,
        "boardId":board, "cardId":card}),
        )
        .await;
}

async fn people(socket: &mut Socket, count: usize) -> Value {
    for _ in 0..20 {
        let message = socket.next_json(PATIENCE).await.expect("presence arrives");
        assert_eq!(
            message["action"], "UPDATE_PRESENCE",
            "only presence is subscribed"
        );
        if message["people"].as_array().expect("people").len() == count {
            return message;
        }
    }
    panic!("the expected people never arrived");
}

#[tokio::test]
async fn visits_show_cards_deduplicate_tabs_and_leave_on_disconnect() {
    let th = Setup::native().start().await.init_basic().await;
    let t = table(&th, &paragraphs(&["hello"])).await;
    let other = th.client2.me().await.id;
    give(&th, &t.board_id, &other, "viewer").await;
    let mut one = Socket::connect_as(&th.client).await;
    visit(&mut one, &t.board_id, "").await;
    let initial = people(&mut one, 1).await;
    check_message(
        &read_contract("websocket.json"),
        &read_contract("openapi.json"),
        &initial,
    );
    let mut two = Socket::connect_as(&th.client2).await;
    visit(&mut two, &t.board_id, &t.card_id).await;
    let shown = people(&mut one, 2).await;
    let person = shown["people"]
        .as_array()
        .expect("people")
        .iter()
        .find(|person| person["userId"] == other)
        .expect("the other reader");
    assert_eq!(
        person["cardIds"],
        json!([t.card_id]),
        "the open card is named"
    );
    let mut duplicate = Socket::connect_as(&th.client2).await;
    visit(&mut duplicate, &t.board_id, &t.card_id).await;
    let _ = people(&mut duplicate, 2).await;
    assert!(
        one.next_json(QUIET).await.is_none(),
        "a duplicate tab changes no roster"
    );
    drop(duplicate);
    assert!(
        one.next_json(QUIET).await.is_none(),
        "the user's other tab remains"
    );
    visit(&mut two, &t.board_id, "").await;
    let closed = people(&mut one, 2).await;
    assert!(
        closed["people"]
            .as_array()
            .expect("people")
            .iter()
            .all(|person| person["cardIds"] == json!([])),
        "closing the card clears its readers"
    );
    drop(two);
    let _ = people(&mut one, 1).await;
    assert!(
        one.next_json(QUIET).await.is_none(),
        "unchanged rosters are quiet"
    );
    th.teardown().await;
}

#[tokio::test]
async fn token_visits_count_anonymously_and_receive_nothing() {
    let th = Setup::local_mode().start().await.init_basic().await;
    let t = table(&th, &paragraphs(&["hello"])).await;
    let sharing: Sharing = th
        .client
        .post(
            &format!("{TEST_WS}/boards/{}/sharing", t.board_id),
            &json!({"enabled":true}),
        )
        .await
        .ok_json();
    let mut member = Socket::connect_as(&th.client).await;
    visit(&mut member, &t.board_id, "").await;
    let _ = people(&mut member, 1).await;
    // Even the owner's signed-in socket is anonymous when it explicitly uses a link.
    let mut reader = Socket::connect_as(&th.client).await;
    reader
        .send(json!({"action":"SET_PRESENCE", "boardId":t.board_id,
        "readToken":sharing.token}))
        .await;
    let roster = people(&mut member, 1).await;
    assert_eq!(roster["anonymousCount"], 1, "one anonymous connection");
    assert!(
        reader.next_json(QUIET).await.is_none(),
        "a reader hears no names or identifiers"
    );
    drop(reader);
    assert_eq!(
        people(&mut member, 1).await["anonymousCount"],
        0,
        "the reader left"
    );
    th.teardown().await;
}

#[tokio::test]
async fn no_membership_no_presence_and_revoked_membership_expires() {
    let th = Setup::native()
        .configure(|c| c.collab.collab_recheck_seconds = 1)
        .start()
        .await
        .init_basic()
        .await;
    let t = table(&th, &paragraphs(&["hello"])).await;
    let mut one = Socket::connect_as(&th.client).await;
    let mut two = Socket::connect_as(&th.client2).await;
    visit(&mut one, &t.board_id, "").await;
    let _ = people(&mut one, 1).await;
    visit(&mut two, &t.board_id, "").await;
    assert!(
        two.next_json(QUIET).await.is_none(),
        "an uninvited user hears nothing"
    );
    assert!(
        one.next_json(QUIET).await.is_none(),
        "the uninvited user is absent"
    );
    let other = th.client2.me().await.id;
    give(&th, &t.board_id, &other, "viewer").await;
    visit(&mut two, &t.board_id, "").await;
    let _ = people(&mut one, 2).await;
    let _ = people(&mut two, 2).await;
    th.client
        .delete(&format!("{TEST_WS}/boards/{}/members/{other}", t.board_id))
        .await
        .check_no_content();
    let _ = people(&mut one, 1).await;
    assert!(
        two.next_json(QUIET).await.is_none(),
        "the removed member receives nothing"
    );
    th.teardown().await;
}

#[tokio::test]
async fn leaving_and_reentering_within_one_batch_gets_a_new_roster() {
    let th = Setup::native().start().await.init_basic().await;
    let t = table(&th, &paragraphs(&["hello"])).await;
    let mut socket = Socket::connect_as(&th.client).await;
    visit(&mut socket, &t.board_id, "").await;
    let before = people(&mut socket, 1).await;
    visit(&mut socket, "", "").await;
    visit(&mut socket, &t.board_id, "").await;
    assert_eq!(
        people(&mut socket, 1).await,
        before,
        "reentry receives the roster again"
    );
    th.teardown().await;
}
