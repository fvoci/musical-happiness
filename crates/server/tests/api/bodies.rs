//! The bodies of cards: one document per card, written and read through the card
//! routes, checked before it is stored, and guarded against overwriting a change the
//! writer has not seen (`docs/s6-design.md` 3.7, 5.7).

use fvoci_model::{BoardType, Card};
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

    // `null` takes the body away; a title alone needs no `readAt`.
    let cleared: Card = th
        .client
        .patch(&path, &json!({"body": null, "readAt": patched.update_at}))
        .await
        .ok_json();
    assert_eq!(cleared.body, None);
    assert!(!cleared.has_body);
    assert_eq!((cleared.tasks_total, cleared.tasks_done), (0, 0));
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
    let copy = card(&th.client, copy["id"].as_str().expect("an id")).await;
    assert_eq!((copy.tasks_total, copy.tasks_done), (2, 1));

    th.teardown().await;
}
