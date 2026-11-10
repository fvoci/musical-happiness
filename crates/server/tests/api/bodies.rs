//! The bodies of cards: one document per card, written and read through the card
//! routes, checked before it is stored, and guarded against overwriting a change the
//! writer has not seen (`docs/s6-design.md` 3.7, 5.7).

use fvoci_model::{BLOCK_TITLE_MAX_CHARS, BoardType, Card};
use reqwest::StatusCode;
use serde_json::{Value, json};

use crate::common::{ApiResponse, Client, Setup, TEST_TEAM_ID};

fn paragraph(text: &str) -> Value {
    json!({"type": "paragraph", "content": [{"type": "text", "text": text}]})
}

/// A task item; `checked` is the JSON of its attribute.
fn task(text: &str, checked: &Value) -> Value {
    json!({"type": "taskItem", "attrs": {"checked": checked}, "content": [paragraph(text)]})
}

/// A document with a paragraph and a task list of two items, one checked.
fn document() -> Value {
    json!({"type": "doc", "content": [
        paragraph("Plan"),
        {"type": "taskList", "content": [task("one", &json!(true)), task("two", &json!(false))]},
    ]})
}

/// Checks that the answer is a 400 that names the member it refuses.
#[track_caller]
fn check_refused_member(response: &ApiResponse, field: &str) {
    response.check_status(StatusCode::BAD_REQUEST);
    assert_eq!(
        response.value()["field"],
        json!(field),
        "{}",
        response.text()
    );
}

async fn new_card(client: &Client, board_id: &str, card: &Value) -> ApiResponse {
    client
        .post(&format!("/boards/{board_id}/cards"), card)
        .await
}

async fn card(client: &Client, card_id: &str) -> Card {
    client.get(&format!("/cards/{card_id}")).await.ok_json()
}

/// What the list of cards says of a card's body: whether it has one, and its tasks,
/// done and all. The list reads them from what was stored with the body.
async fn listed(client: &Client, board_id: &str, card_id: &str) -> (bool, u64, u64) {
    let cards = client
        .get(&format!("/boards/{board_id}/cards"))
        .await
        .value();
    let Value::Array(cards) = cards else {
        panic!("a list: {cards}");
    };
    let card = cards
        .into_iter()
        .find(|card| card["id"] == json!(card_id))
        .unwrap_or_else(|| panic!("{card_id} is listed"));
    let count = |name: &str| {
        card[name]
            .as_u64()
            .unwrap_or_else(|| panic!("{name}: {card}"))
    };
    (
        card["hasBody"].as_bool().expect("hasBody"),
        count("tasksTotal"),
        count("tasksDone"),
    )
}

/// A card made with a body is read back with it; the list of cards says that it has
/// one and counts its tasks, without the body.
#[tokio::test]
async fn a_card_is_made_with_its_body_and_listed_without_it() {
    let th = Setup::native().start().await.init_basic().await;
    let board = th.create_board(TEST_TEAM_ID, BoardType::Private).await;

    let created: Card = new_card(
        &th.client,
        &board.id,
        &json!({"title": "With a body", "body": document()}),
    )
    .await
    .created_json();
    assert_eq!(
        created.body.as_ref().map(fvoci_model::Body::document),
        Some(&document())
    );
    assert!(created.has_body, "{created:?}");
    assert_eq!((created.tasks_total, created.tasks_done), (2, 1));
    let read = card(&th.client, &created.id).await;
    assert_eq!(read, created, "the card is stored as answered");

    let empty: Card = new_card(
        &th.client,
        &board.id,
        &json!({"title": "Empty", "body": {"type": "doc", "content": [{"type": "paragraph"}]}}),
    )
    .await
    .created_json();
    assert_eq!(empty.body, None, "an empty document is no body");
    assert!(!empty.has_body);

    let listed = th
        .client
        .get(&format!("/boards/{}/cards", board.id))
        .await
        .value();
    let listed = listed.as_array().expect("a list");
    let with_body = listed
        .iter()
        .find(|card| card["id"] == json!(created.id))
        .expect("the card is listed");
    assert_eq!(with_body["body"], Value::Null, "{with_body}");
    assert_eq!(with_body["hasBody"], json!(true), "{with_body}");
    assert_eq!(with_body["tasksTotal"], json!(2), "{with_body}");
    assert_eq!(with_body["tasksDone"], json!(1), "{with_body}");

    th.teardown().await;
}

/// A body is changed with the `updateAt` of the card as the writer read it; with
/// another one the patch is refused and changes nothing.
#[tokio::test]
async fn a_body_is_changed_only_from_the_card_as_it_is() {
    let th = Setup::native().start().await.init_basic().await;
    let board = th.create_board(TEST_TEAM_ID, BoardType::Private).await;
    let created: Card = new_card(&th.client, &board.id, &json!({"title": "Card"}))
        .await
        .created_json();
    let path = format!("/cards/{}", created.id);

    let response = th.client.patch(&path, &json!({"body": document()})).await;
    check_refused_member(&response, "readAt");

    let patched: Card = th
        .client
        .patch(
            &path,
            &json!({"body": document(), "readAt": created.update_at}),
        )
        .await
        .ok_json();
    assert!(patched.has_body, "{patched:?}");
    assert_eq!(
        patched.body.as_ref().map(fvoci_model::Body::document),
        Some(&document())
    );
    assert_eq!(
        listed(&th.client, &board.id, &created.id).await,
        (true, 2, 1),
        "the list reads what the patch stored"
    );

    // The first version is no longer the card's.
    let stale = th
        .client
        .patch(
            &path,
            &json!({"body": null, "readAt": created.update_at, "title": "Stale"}),
        )
        .await;
    stale.check_status(StatusCode::CONFLICT);
    assert_eq!(stale.error_code(), "conflict");
    assert_eq!(
        card(&th.client, &created.id).await,
        patched,
        "a refused patch changes nothing"
    );
    // A patch that could not be stored anyway is refused for that first.
    let too_long = "a".repeat(BLOCK_TITLE_MAX_CHARS + 1);
    let invalid = th
        .client
        .patch(
            &path,
            &json!({"title": too_long, "readAt": created.update_at}),
        )
        .await;
    invalid.check_status(StatusCode::BAD_REQUEST);

    // `null` takes the body away; a title alone needs no `readAt`.
    let cleared: Card = th
        .client
        .patch(&path, &json!({"body": null, "readAt": patched.update_at}))
        .await
        .ok_json();
    assert_eq!(cleared.body, None);
    assert!(!cleared.has_body);
    assert_eq!((cleared.tasks_total, cleared.tasks_done), (0, 0));
    assert_eq!(
        listed(&th.client, &board.id, &created.id).await,
        (false, 0, 0)
    );
    th.client
        .patch(&path, &json!({"title": "Renamed"}))
        .await
        .check_ok();

    th.teardown().await;
}

/// A body that may not be stored is a 400 that names `body`, on both routes.
#[tokio::test]
async fn a_body_that_is_no_document_is_refused() {
    let th = Setup::native().start().await.init_basic().await;
    let board = th.create_board(TEST_TEAM_ID, BoardType::Private).await;
    let created: Card = new_card(&th.client, &board.id, &json!({"title": "Card"}))
        .await
        .created_json();

    for body in [
        json!("text"),
        json!({"type": "doc", "content": []}),
        json!({"type": "doc", "content": [{"type": "paragraph", "extra": true}]}),
    ] {
        let response = new_card(&th.client, &board.id, &json!({"body": body})).await;
        check_refused_member(&response, "body");
        let response = th
            .client
            .patch(
                &format!("/cards/{}", created.id),
                &json!({"body": body, "readAt": created.update_at}),
            )
            .await;
        check_refused_member(&response, "body");
    }
    assert_eq!(
        card(&th.client, &created.id).await,
        created,
        "nothing was stored"
    );

    th.teardown().await;
}

/// Every change of a card moves its `updateAt` on, so a `readAt` from before a change
/// is refused even when the change came in the same millisecond, and also when the
/// patch carries no body.
#[tokio::test]
async fn a_read_at_from_before_a_change_is_refused() {
    let th = Setup::native().start().await.init_basic().await;
    let board = th.create_board(TEST_TEAM_ID, BoardType::Private).await;
    let created: Card = new_card(
        &th.client,
        &board.id,
        &json!({"title": "Card", "body": null}),
    )
    .await
    .created_json();
    assert_eq!(created.body, None, "`null` is no body");
    let path = format!("/cards/{}", created.id);

    // Two patches made from the same read, one right after the other.
    let first: Card = th
        .client
        .patch(
            &path,
            &json!({"body": document(), "readAt": created.update_at}),
        )
        .await
        .ok_json();
    assert!(
        first.update_at > created.update_at,
        "{} after {}",
        first.update_at,
        created.update_at
    );
    let second = th
        .client
        .patch(
            &path,
            &json!({"title": "Second", "readAt": created.update_at}),
        )
        .await;
    second.check_status(StatusCode::CONFLICT);
    assert_eq!(card(&th.client, &created.id).await, first, "unchanged");

    // The routes of blocks check `readAt` too, and take no body yet.
    let block_path = format!("/boards/{}/blocks/{}", board.id, created.id);
    let stale = th
        .client
        .patch(
            &block_path,
            &json!({"title": "Stale", "readAt": created.update_at}),
        )
        .await;
    stale.check_status(StatusCode::CONFLICT);
    let response = th
        .client
        .patch(
            &block_path,
            &json!({"body": document(), "readAt": first.update_at}),
        )
        .await;
    check_refused_member(&response, "body");
    let response = th
        .client
        .post(
            &format!("/boards/{}/blocks", board.id),
            &json!([{"type": "card", "boardId": board.id, "createAt": 1, "updateAt": 1,
                     "body": document()}]),
        )
        .await;
    check_refused_member(&response, "body");
    assert_eq!(card(&th.client, &created.id).await, first, "unchanged");

    th.teardown().await;
}

/// A body goes to the trash with its card and comes back with it, and a copy of the
/// card has it too.
#[tokio::test]
async fn a_body_comes_back_from_the_trash_and_is_copied() {
    let th = Setup::native().start().await.init_basic().await;
    let board = th.create_board(TEST_TEAM_ID, BoardType::Private).await;
    let created: Card = new_card(
        &th.client,
        &board.id,
        &json!({"title": "Card", "body": document()}),
    )
    .await
    .created_json();
    let block_path = format!("/boards/{}/blocks/{}", board.id, created.id);

    th.client.delete(&block_path).await.check_no_content();
    th.client
        .post_empty(&format!("{block_path}/undelete"))
        .await
        .check_ok();
    let restored = card(&th.client, &created.id).await;
    assert_eq!(restored.body, created.body, "{restored:?}");
    assert_eq!((restored.tasks_total, restored.tasks_done), (2, 1));
    assert_eq!(
        listed(&th.client, &board.id, &created.id).await,
        (true, 2, 1),
        "the list reads what was restored"
    );

    let copies: Vec<Value> = th
        .client
        .post_empty(&format!("{block_path}/duplicate"))
        .await
        .created_json();
    let copy = copies
        .iter()
        .find(|block| block["type"] == json!("card"))
        .expect("the copy of the card");
    assert_eq!(copy["body"], document(), "{copy}");
    let copy_id = copy["id"].as_str().expect("an id");
    let copy = card(&th.client, copy_id).await;
    assert_eq!((copy.tasks_total, copy.tasks_done), (2, 1));
    assert_eq!(
        listed(&th.client, &board.id, copy_id).await,
        (true, 2, 1),
        "the list reads what was copied"
    );

    th.teardown().await;
}

/// The routes of blocks take no body, whatever it is, until they check one the way
/// the routes of cards do: each refusal names `body` and stores nothing.
#[tokio::test]
async fn the_routes_of_blocks_refuse_a_body() {
    let th = Setup::native().start().await.init_basic().await;
    let board = th.create_board(TEST_TEAM_ID, BoardType::Private).await;
    let created: Card = new_card(&th.client, &board.id, &json!({"title": "Card"}))
        .await
        .created_json();
    let blocks = format!("/boards/{}/blocks", board.id);
    let block = |body: &Value| {
        json!({"id": "b1", "boardId": board.id, "type": "card", "createAt": 1, "updateAt": 1,
               "body": body})
    };
    let empty = json!({"type": "doc", "content": [{"type": "paragraph"}]});
    let teams_boards = format!("/teams/{TEST_TEAM_ID}/boards");
    let boards_before = th.client.get(&teams_boards).await.value();

    // A document, an empty one and something that is none: all refused as a body.
    for body in [document(), empty, json!("text")] {
        let response = th.client.post(&blocks, &[block(&body)]).await;
        check_refused_member(&response, "body");
        let response = th
            .client
            .post(
                "/boards-and-blocks",
                &json!({
                    "boards": [{"id": "bd", "teamId": TEST_TEAM_ID, "type": "P"}],
                    "blocks": [{"id": "b1", "boardId": "bd", "type": "card",
                                "createAt": 1, "updateAt": 1, "body": body}],
                }),
            )
            .await;
        check_refused_member(&response, "body");
    }
    // A patch may not even take the body away.
    for body in [document(), Value::Null, json!("text")] {
        let patch = json!({"title": "Patched", "body": body});
        let response = th
            .client
            .patch(&format!("{blocks}/{}", created.id), &patch)
            .await;
        check_refused_member(&response, "body");
        let response = th
            .client
            .patch(
                &blocks,
                &json!({"blockIds": [created.id], "blockPatches": [patch]}),
            )
            .await;
        check_refused_member(&response, "body");
        let response = th
            .client
            .patch(
                "/boards-and-blocks",
                &json!({
                    "boardIds": [board.id], "boardPatches": [{"title": "n"}],
                    "blockIds": [created.id], "blockPatches": [patch],
                }),
            )
            .await;
        check_refused_member(&response, "body");
    }
    // Nothing of the refused requests was stored: no block, no board, no new title.
    let stored = th.client.get(&blocks).await.value();
    let stored = stored.as_array().expect("a list of blocks");
    assert_eq!(stored.len(), 1, "only the card: {stored:?}");
    assert_eq!(
        th.client.get(&teams_boards).await.value(),
        boards_before,
        "no board was made or renamed"
    );
    // A block without a body, or with `null`, is made as before.
    th.client
        .post(&blocks, &[block(&Value::Null)])
        .await
        .check_status(StatusCode::CREATED);
    assert_eq!(
        card(&th.client, &created.id).await,
        created,
        "no refused patch changed the card"
    );

    th.teardown().await;
}
