//! What the references of a body name to whoever reads a board by its link: the cards
//! of that board, and nothing else. A reference to anything else of the workspace goes
//! out without its identifier, whatever it named, so that the reader learns neither
//! what is elsewhere nor whether anything is.

use std::time::Duration;

use serde_json::{Value, json};

use crate::common::{Client, Setup, TEST_WORKSPACE_ID, TEST_WS, UNKNOWN_ID};
use crate::references::{HIDDEN, made};
use crate::references_to::{body_naming, card_names, card_of, comment_naming};
use crate::websocket::Socket;

/// The identifiers the references below `node` hold, in the order they stand.
pub(crate) fn references_in(node: &Value) -> Vec<String> {
    let mut held = Vec::new();
    if node["type"] == "reference" {
        held.push(node["attrs"]["id"].as_str().expect("an id").to_owned());
    }
    for child in node["content"].as_array().into_iter().flatten() {
        held.extend(references_in(child));
    }
    held
}

/// The block with the identifier among the blocks of an answer.
fn block_of<'a>(blocks: &'a Value, id: &str) -> &'a Value {
    blocks
        .as_array()
        .expect("a list")
        .iter()
        .find(|block| block["id"] == id)
        .unwrap_or_else(|| panic!("no block {id} in {blocks}"))
}

/// A card of a shared board names a card beside it, itself, and much that is not of
/// the board: a card and a page elsewhere, a card of the board in the trash, and
/// nothing. Whoever reads by the link, with a session or without, gets the first two
/// and an empty identifier for each of the rest, by HTTP and when the card changes;
/// a member on the board's own routes gets them all.
#[tokio::test]
async fn a_reader_by_a_link_is_told_of_the_cards_of_the_board_and_of_nothing_else() {
    let th = Setup::local_mode().start().await.init_basic().await;
    let owner = &th.client;
    let shared = made(owner, TEST_WS, "P", "shared").await;
    let elsewhere = made(owner, TEST_WS, "P", HIDDEN).await;
    let board = shared.board.as_str();
    let beside = card_of(owner, TEST_WS, board, "beside").await;
    let gone = card_of(owner, TEST_WS, board, HIDDEN).await;
    let source = card_of(owner, TEST_WS, board, "source").await;
    let outside = [
        elsewhere.card.as_str(),
        elsewhere.page.as_str(),
        shared.page.as_str(),
        gone.as_str(),
        UNKNOWN_ID,
    ];
    let mut named = vec![beside.as_str()];
    named.extend(outside);
    named.push(&source);
    card_names(owner, TEST_WS, &source, &named).await;
    let comment = comment_naming(owner, board, &source, &[&elsewhere.card, &beside]).await;
    owner
        .delete(&format!("{TEST_WS}/boards/{board}/blocks/{gone}"))
        .await
        .check_no_content();
    let token = th.share(board).await;

    let kept = [beside.as_str(), "", "", "", "", "", source.as_str()];
    let tells_nothing = |text: &str, what: &str| {
        for id in outside {
            assert!(!text.contains(id), "{what} tells of {id}: {text}");
        }
        assert!(!text.contains(HIDDEN), "{what}: {text}");
    };
    let anonymous = th.anonymous();
    for reader in [&anonymous, &th.client2, owner] {
        let all = reader
            .get(&format!(
                "/shared/{board}/blocks?all=true&readToken={token}"
            ))
            .await;
        all.check_ok();
        tells_nothing(&all.text(), "every block");
        let blocks = all.value();
        assert_eq!(references_in(&block_of(&blocks, &source)["body"]), kept);
        assert_eq!(
            references_in(&block_of(&blocks, &comment)["body"]),
            ["", beside.as_str()]
        );
        for rest in [
            format!("blocks?blockId={source}"),
            format!("blocks?parentId={source}&type=comment"),
            "cards?".to_owned(),
            "cards?preview=true".to_owned(),
        ] {
            let answer = reader
                .get(&format!("/shared/{board}/{rest}&readToken={token}"))
                .await;
            answer.check_ok();
            tells_nothing(&answer.text(), &rest);
        }
    }
    // A member on the board's own routes is told every identifier, as it was written.
    let own = owner
        .get(&format!("{TEST_WS}/boards/{board}/blocks?all=true"))
        .await
        .value();
    assert_eq!(references_in(&block_of(&own, &source)["body"]), named);

    told_of_a_change(&th.anonymous(), owner, &token, &source, &named, &kept).await;
    th.teardown().await;
}

/// The card is written again while a connection follows it by the link: what that
/// connection is told of the change names what `kept` says and nothing of `named`
/// besides.
async fn told_of_a_change(
    reader: &Client,
    owner: &Client,
    token: &str,
    source: &str,
    named: &[&str],
    kept: &[&str],
) {
    let mut socket = Socket::connect_as(&reader.as_browser()).await;
    socket
        .send(json!({
            "action": "SUBSCRIBE_BLOCKS", "workspaceId": TEST_WORKSPACE_ID,
            "readToken": token, "blockIds": [source],
        }))
        .await;
    // No acknowledgement: the card is written until the subscription shows.
    let path = format!("{TEST_WS}/cards/{source}");
    let mut told = None;
    for round in 0..50 {
        let read_at = owner.get(&path).await.value()["updateAt"].clone();
        let body = body_naming(&format!("round {round}"), named);
        owner
            .patch(&path, &json!({"readAt": read_at, "body": body}))
            .await
            .check_ok();
        if let Some(message) = socket.next_json(Duration::from_millis(100)).await {
            told = Some(message);
            break;
        }
    }
    let told = told.expect("the reader is told of the card");
    assert_eq!(told["action"], json!("UPDATE_BLOCK"), "{told}");
    assert_eq!(references_in(&told["block"]["body"]), kept, "{told}");
    let text = told.to_string();
    for id in named.iter().filter(|id| !kept.contains(id)) {
        assert!(!text.contains(id), "the change tells of {id}: {text}");
    }
}
