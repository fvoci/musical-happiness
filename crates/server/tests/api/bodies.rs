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

    // The routes of blocks check `readAt` too.
    let block_path = format!("/boards/{}/blocks/{}", board.id, created.id);
    let stale = th
        .client
        .patch(
            &block_path,
            &json!({"title": "Stale", "readAt": created.update_at}),
        )
        .await;
    stale.check_status(StatusCode::CONFLICT);
    let stale = th
        .client
        .patch(
            &block_path,
            &json!({"body": null, "readAt": created.update_at}),
        )
        .await;
    stale.check_status(StatusCode::CONFLICT);
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

/// A card made through `createBlocks` with a body; `body` is that of the block.
fn card_block(board_id: &str, body: &Value) -> Value {
    json!({"id": "b1", "boardId": board_id, "type": "card", "createAt": 1, "updateAt": 1,
           "body": body})
}

/// A document with an image of the file of that stored name.
fn with_image(file_id: &str) -> Value {
    json!({"type": "doc", "content": [
        paragraph("Picture"),
        {"type": "image", "attrs": {"fileId": file_id}},
    ]})
}

/// The routes of blocks take the body of a card as the routes of cards do: checked,
/// refused as `body`, changed only with `readAt`, and refused on other blocks.
#[tokio::test]
async fn the_routes_of_blocks_take_the_body_of_a_card() {
    let th = Setup::native().start().await.init_basic().await;
    let board = th.create_board(TEST_TEAM_ID, BoardType::Private).await;
    let blocks = format!("/boards/{}/blocks", board.id);

    // Made with a body, which is checked and counted.
    let made: Vec<Value> = th
        .client
        .post(&blocks, &[card_block(&board.id, &document())])
        .await
        .created_json();
    let card_id = made[0]["id"].as_str().expect("an id").to_owned();
    assert_eq!(made[0]["body"], document(), "{made:?}");
    assert_eq!(listed(&th.client, &board.id, &card_id).await, (true, 2, 1));
    // What may not be stored, and a body on a block that is no card.
    for (block, field) in [
        (card_block(&board.id, &json!("text")), "body"),
        (
            card_block(
                &board.id,
                &json!({"type": "doc", "content": [{"type": "table"}]}),
            ),
            "body",
        ),
        (
            json!({"id": "t1", "boardId": board.id, "parentId": card_id, "type": "text",
                   "createAt": 1, "updateAt": 1, "body": document()}),
            "body",
        ),
    ] {
        let response = th.client.post(&blocks, &[block]).await;
        check_refused_member(&response, field);
    }

    // Changed with the `updateAt` it was read at, through each route of patches.
    let path = format!("{blocks}/{card_id}");
    let read = card(&th.client, &card_id).await;
    let response = th.client.patch(&path, &json!({"body": null})).await;
    check_refused_member(&response, "readAt");
    let response = th
        .client
        .patch(
            &path,
            &json!({"body": {"type": "doc"}, "readAt": read.update_at}),
        )
        .await;
    check_refused_member(&response, "body");
    th.client
        .patch(&path, &json!({"body": null, "readAt": read.update_at}))
        .await
        .check_no_content();
    assert_eq!(listed(&th.client, &board.id, &card_id).await, (false, 0, 0));
    let stale = th
        .client
        .patch(
            &path,
            &json!({"body": document(), "readAt": read.update_at}),
        )
        .await;
    stale.check_status(StatusCode::CONFLICT);

    let read = card(&th.client, &card_id).await;
    let batch = json!({"blockIds": [card_id],
                       "blockPatches": [{"body": document(), "readAt": read.update_at}]});
    th.client.patch(&blocks, &batch).await.check_no_content();
    assert_eq!(card(&th.client, &card_id).await.body, Some(document_body()));
    let batch = json!({"blockIds": [card_id], "blockPatches": [{"body": null}]});
    check_refused_member(&th.client.patch(&blocks, &batch).await, "readAt");

    let read = card(&th.client, &card_id).await;
    let request = json!({
        "boardIds": [board.id], "boardPatches": [{"title": "n"}],
        "blockIds": [card_id], "blockPatches": [{"body": null, "readAt": read.update_at}],
    });
    th.client
        .patch("/boards-and-blocks", &request)
        .await
        .check_ok();
    assert_eq!(card(&th.client, &card_id).await.body, None);

    // A new board and its cards, with a body.
    let made: Value = th
        .client
        .post(
            "/boards-and-blocks",
            &json!({
                "boards": [{"id": "bd", "teamId": TEST_TEAM_ID, "type": "P"}],
                "blocks": [card_block("bd", &document())],
            }),
        )
        .await
        .created_json();
    assert_eq!(made["blocks"][0]["body"], document(), "{made}");

    th.teardown().await;
}

fn document_body() -> fvoci_model::Body {
    fvoci_model::Body::parse(document())
        .expect("allowed")
        .expect("not empty")
}

/// An image of a body shows a file of the card's board; a copy of the card shows a copy
/// of the file.
#[tokio::test]
async fn the_images_of_a_body_are_files_of_its_board() {
    let th = Setup::native().start().await.init_basic().await;
    let board = th.create_board(TEST_TEAM_ID, BoardType::Private).await;
    let other = th.create_board(TEST_TEAM_ID, BoardType::Private).await;
    let file_id = crate::files::uploaded_id(
        &crate::files::upload(&th.client, &board.id, "picture.png", b"png").await,
    );
    let elsewhere = crate::files::uploaded_id(
        &crate::files::upload(&th.client, &other.id, "other.png", b"png").await,
    );

    // A file of another board, or none, is refused on every route that takes a body.
    for missing in [elsewhere.as_str(), "7nofile.png"] {
        let response = new_card(
            &th.client,
            &board.id,
            &json!({"title": "Card", "body": with_image(missing)}),
        )
        .await;
        check_refused_member(&response, "body");
        let response = th
            .client
            .post(
                &format!("/boards/{}/blocks", board.id),
                &[card_block(&board.id, &with_image(missing))],
            )
            .await;
        check_refused_member(&response, "body");
    }
    let created: Card = new_card(
        &th.client,
        &board.id,
        &json!({"title": "Card", "body": with_image(&file_id)}),
    )
    .await
    .created_json();
    let response = th
        .client
        .patch(
            &format!("/cards/{}", created.id),
            &json!({"body": with_image(&elsewhere), "readAt": created.update_at}),
        )
        .await;
    check_refused_member(&response, "body");

    // The copy of the card shows a copy of the file, a file of the same board.
    let copies: Vec<Value> = th
        .client
        .post_empty(&format!(
            "/boards/{}/blocks/{}/duplicate",
            board.id, created.id
        ))
        .await
        .created_json();
    let copy = copies
        .iter()
        .find(|block| block["type"] == json!("card"))
        .expect("the copy of the card");
    let copied_file = copy["body"]["content"][1]["attrs"]["fileId"]
        .as_str()
        .expect("the image of the copy names a file")
        .to_owned();
    assert_ne!(copied_file, file_id, "the copy shows a file of its own");
    let stored = card(&th.client, copy["id"].as_str().expect("an id")).await;
    assert_eq!(
        stored.body.as_ref().map(fvoci_model::Body::file_ids),
        Some(vec![copied_file.as_str()]),
        "the stored copy is what was answered"
    );
    th.client
        .get(&format!("/boards/{}/files/{copied_file}", board.id))
        .await
        .check_ok();

    th.teardown().await;
}
