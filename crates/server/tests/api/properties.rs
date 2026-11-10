//! What card property definitions and values must look like to be stored.
//!
//! Not in the original, which stores whatever it is sent: a definition without an
//! identifier, a number as a property value, a string where the properties of a card
//! should be (all answered 200 by the original and, until S6, by this server). Clients
//! render these values as they are, so one member could break a board for the others,
//! and a card with malformed properties broke the board's card list. Every way of
//! writing them now answers 400 and stores nothing.

use std::io::{Cursor, Write as _};

use fvoci_model::{Block, Board, Card};
use reqwest::StatusCode;
use serde_json::{Value, json};

use crate::common::{ApiResponse, Client, Setup, TEST_TEAM_ID, TestServer};
use crate::export::import;

/// Card property definitions that are malformed, each with what is wrong with it.
fn malformed_definitions() -> Vec<(&'static str, Value)> {
    vec![
        ("no id", json!({"name": "Status", "type": "select"})),
        ("an empty id", json!({"id": "", "name": "Status"})),
        ("a number as id", json!({"id": 7, "name": "Status"})),
        ("a number as name", json!({"id": "p1", "name": 7})),
        ("a list as type", json!({"id": "p1", "type": ["select"]})),
        (
            "options that are no list",
            json!({"id": "p1", "options": "none"}),
        ),
        (
            "an option that is no object",
            json!({"id": "p1", "options": ["a"]}),
        ),
        (
            "an option with a number as value",
            json!({"id": "p1", "options": [{"id": "o1", "value": 1, "color": "red"}]}),
        ),
    ]
}

/// Card property values that are malformed.
fn malformed_values() -> Vec<(&'static str, Value)> {
    vec![
        ("an object", json!({"nested": "x"})),
        ("a number", json!(3)),
        ("a boolean", json!(true)),
        ("a list with a number", json!(["a", 1])),
        ("a list with a list", json!([["a"]])),
    ]
}

async fn new_board(client: &Client, card_properties: Value) -> ApiResponse {
    client
        .post(
            "/boards",
            &json!({
                "teamId": TEST_TEAM_ID, "type": "P", "title": "Properties",
                "cardProperties": card_properties,
            }),
        )
        .await
}

fn card_block(board_id: &str, properties: Value) -> Value {
    let mut block = json!({
        "id": "c1", "boardId": board_id, "parentId": board_id, "type": "card", "title": "Card",
        "schema": 1, "createAt": 1, "updateAt": 1, "fields": {},
    });
    block["fields"]["properties"] = properties;
    block
}

/// Checks that a request was refused as a bad request that says what is wrong.
#[track_caller]
fn check_refused(response: &ApiResponse, what: &str) {
    assert_eq!(
        response.status,
        StatusCode::BAD_REQUEST,
        "{what}: {}",
        response.text()
    );
    assert!(
        response.error_message().contains("card propert"),
        "{what}: the refusal names the property data: {}",
        response.text()
    );
}

/// A private board of the first user with one well-formed definition and one card that
/// has a value for it.
async fn board_with_card(th: &TestServer) -> (Board, Card) {
    let board: Board = new_board(
        &th.client,
        json!([{"id": "p1", "name": "Status", "type": "select", "options": []}]),
    )
    .await
    .created_json();
    let card: Card = th
        .client
        .post(
            &format!("/boards/{}/cards", board.id),
            &json!({"title": "Card", "properties": {"p1": "open"}}),
        )
        .await
        .created_json();
    (board, card)
}

async fn board_count(client: &Client) -> usize {
    let boards: Vec<Board> = client
        .get(&format!("/teams/{TEST_TEAM_ID}/boards"))
        .await
        .ok_json();
    boards.len()
}

async fn blocks_of(client: &Client, board_id: &str) -> Vec<Block> {
    client
        .get(&format!("/boards/{board_id}/blocks"))
        .await
        .ok_json()
}

// -- definitions --------------------------------------------------------------------------

#[tokio::test]
async fn a_board_with_a_malformed_card_property_definition_is_not_created() {
    let th = Setup::native().start().await.init_basic().await;

    for (what, definition) in malformed_definitions() {
        // Among well-formed ones, too.
        let response = new_board(&th.client, json!([{"id": "ok"}, definition])).await;
        check_refused(&response, what);
    }
    let not_objects = new_board(&th.client, json!(["p1"])).await;
    assert_eq!(
        not_objects.status,
        StatusCode::BAD_REQUEST,
        "a definition that is no object: {}",
        not_objects.text()
    );
    assert_eq!(board_count(&th.client).await, 0, "no board was created");
    th.teardown().await;
}

#[tokio::test]
async fn a_board_patch_with_a_malformed_card_property_definition_changes_nothing() {
    let th = Setup::native().start().await.init_basic().await;
    let (board, _card) = board_with_card(&th).await;
    let path = format!("/boards/{}", board.id);

    for (what, definition) in malformed_definitions() {
        let response = th
            .client
            .patch(
                &path,
                &json!({"title": "renamed", "updatedCardProperties": [{"id": "p2"}, definition]}),
            )
            .await;
        check_refused(&response, what);
    }
    let stored: Board = th.client.get(&path).await.ok_json();
    assert_eq!(
        stored.title, "Properties",
        "the title was not changed either"
    );
    assert_eq!(
        stored.card_properties, board.card_properties,
        "the definitions are as they were"
    );
    th.teardown().await;
}

#[tokio::test]
async fn well_formed_card_property_definitions_are_stored_as_sent() {
    let th = Setup::native().start().await.init_basic().await;
    let definitions = json!([
        {"id": "p1"},
        {"id": "p2", "name": "Status", "type": "select", "options": [
            {"id": "o1", "value": "Open", "color": "propColorRed"},
            {"id": "o2", "value": ""},
        ]},
        {"id": "p3", "name": "", "type": "text", "options": null, "extra": {"kept": [1, true]}},
    ]);

    let board: Value = new_board(&th.client, definitions.clone())
        .await
        .created_json();
    assert_eq!(board["cardProperties"], definitions, "created as sent");

    let added = json!({"id": "p4", "name": "Owner", "type": "person", "options": []});
    let patched: Value = th
        .client
        .patch(
            &format!("/boards/{}", board["id"].as_str().expect("an id")),
            &json!({"updatedCardProperties": [added]}),
        )
        .await
        .ok_json();
    assert_eq!(
        patched["cardProperties"][3], added,
        "a well-formed definition is added by a patch"
    );
    th.teardown().await;
}

// -- values -------------------------------------------------------------------------------

#[tokio::test]
async fn a_block_with_malformed_properties_is_not_inserted() {
    let th = Setup::native().start().await.init_basic().await;
    let board: Board = new_board(&th.client, json!([])).await.created_json();
    let path = format!("/boards/{}/blocks", board.id);

    for (what, value) in malformed_values() {
        let response = th
            .client
            .post(&path, &json!([card_block(&board.id, json!({"p1": value}))]))
            .await;
        check_refused(&response, what);
    }
    for properties in [json!("a string"), json!(["a"]), json!(42), json!(null)] {
        let response = th
            .client
            .post(&path, &json!([card_block(&board.id, properties.clone())]))
            .await;
        check_refused(&response, &format!("properties that are {properties}"));
    }
    // `properties` means property values on a block of any type.
    let mut text = card_block(&board.id, json!({"p1": 1}));
    text["type"] = json!("text");
    check_refused(&th.client.post(&path, &json!([text])).await, "a text block");
    assert_eq!(blocks_of(&th.client, &board.id).await, [], "nothing stored");
    th.teardown().await;
}

#[tokio::test]
async fn a_block_patch_with_malformed_properties_changes_nothing() {
    let th = Setup::native().start().await.init_basic().await;
    let (board, card) = board_with_card(&th).await;
    let block_path = format!("/boards/{}/blocks/{}", board.id, card.id);
    let second: Card = th
        .client
        .post(
            &format!("/boards/{}/cards", board.id),
            &json!({"title": "Second"}),
        )
        .await
        .created_json();

    for (what, value) in malformed_values() {
        let response = th
            .client
            .patch(
                &block_path,
                &json!({"title": "renamed", "updatedFields": {"properties": {"p1": value}}}),
            )
            .await;
        check_refused(&response, what);

        // In a batch, the well-formed patch beside it is not applied either.
        let response = th
            .client
            .patch(
                &format!("/boards/{}/blocks", board.id),
                &json!({
                    "blockIds": [second.id, card.id],
                    "blockPatches": [
                        {"title": "renamed"},
                        {"updatedFields": {"properties": {"p1": value}}},
                    ],
                }),
            )
            .await;
        check_refused(&response, &format!("a batch with {what}"));
    }
    let response = th
        .client
        .patch(
            &block_path,
            &json!({"updatedFields": {"properties": "text"}}),
        )
        .await;
    check_refused(&response, "properties that are a string");

    let stored: Card = th
        .client
        .get(&format!("/cards/{}", card.id))
        .await
        .ok_json();
    assert_eq!(stored.title, "Card", "the title was not changed either");
    assert_eq!(
        stored.properties, card.properties,
        "the values are as they were"
    );
    let other: Card = th
        .client
        .get(&format!("/cards/{}", second.id))
        .await
        .ok_json();
    assert_eq!(
        other.title, "Second",
        "nor was the other block of the batch"
    );
    th.teardown().await;
}

#[tokio::test]
async fn a_card_with_malformed_property_values_is_neither_created_nor_patched() {
    let th = Setup::native().start().await.init_basic().await;
    let (board, card) = board_with_card(&th).await;

    for (what, value) in malformed_values() {
        let created = th
            .client
            .post(
                &format!("/boards/{}/cards", board.id),
                &json!({"title": "Another", "properties": {"p1": value}}),
            )
            .await;
        check_refused(&created, what);
        let patched = th
            .client
            .patch(
                &format!("/cards/{}", card.id),
                &json!({"title": "renamed", "updatedProperties": {"p1": value}}),
            )
            .await;
        check_refused(&patched, what);
    }
    let cards: Vec<Card> = th
        .client
        .get(&format!("/boards/{}/cards", board.id))
        .await
        .ok_json();
    assert_eq!(cards.len(), 1, "no card was created");
    assert_eq!(cards[0].title, "Card", "and the card was not changed");
    assert_eq!(cards[0].properties, card.properties);
    th.teardown().await;
}

#[tokio::test]
async fn well_formed_property_values_are_stored_as_sent() {
    let th = Setup::native().start().await.init_basic().await;
    let (board, card) = board_with_card(&th).await;
    let values = json!({
        "text": "hello", "empty": "", "many": ["a", "b"], "none": [], "unset": null,
    });

    let created: Value = th
        .client
        .post(
            &format!("/boards/{}/cards", board.id),
            &json!({"title": "Another", "properties": values}),
        )
        .await
        .created_json();
    assert_eq!(created["properties"], values, "a card is created with them");

    let patched: Value = th
        .client
        .patch(
            &format!("/cards/{}", card.id),
            &json!({"updatedProperties": values}),
        )
        .await
        .ok_json();
    assert_eq!(patched["properties"], values, "and patched with them");

    let inserted = th
        .client
        .post(
            &format!("/boards/{}/blocks", board.id),
            &json!([
                card_block(&board.id, values.clone()),
                card_block(&board.id, json!({})),
            ]),
        )
        .await;
    inserted.check_created();
    th.client
        .patch(
            &format!("/boards/{}/blocks/{}", board.id, card.id),
            &json!({"updatedFields": {"properties": {"p1": ["x"]}}}),
        )
        .await
        .check_no_content();
    th.teardown().await;
}

// -- the combined routes and the import ---------------------------------------------------

#[tokio::test]
async fn boards_and_blocks_with_malformed_property_data_are_refused_as_a_whole() {
    let th = Setup::native().start().await.init_basic().await;
    let request = |card_properties: Value, values: Value| {
        json!({
            "boards": [{
                "id": "b1", "teamId": TEST_TEAM_ID, "type": "P", "title": "Both",
                "cardProperties": card_properties,
            }],
            "blocks": [card_block("b1", values)],
        })
    };

    let response = th
        .client
        .post(
            "/boards-and-blocks",
            &request(json!([{"name": "no id"}]), json!({})),
        )
        .await;
    check_refused(&response, "a definition without id");
    let response = th
        .client
        .post(
            "/boards-and-blocks",
            &request(json!([{"id": "p1"}]), json!({"p1": 3})),
        )
        .await;
    check_refused(&response, "a number as value");
    assert_eq!(board_count(&th.client).await, 0, "neither made a board");

    let created: Value = th
        .client
        .post(
            "/boards-and-blocks",
            &request(json!([{"id": "p1"}]), json!({"p1": "v"})),
        )
        .await
        .created_json();
    let board_id = created["boards"][0]["id"].as_str().expect("a board id");
    let block_id = created["blocks"][0]["id"].as_str().expect("a block id");

    for (what, board_patch, block_patch) in [
        (
            "a definition without id",
            json!({"title": "renamed", "updatedCardProperties": [{"name": "x"}]}),
            json!({"title": "renamed"}),
        ),
        (
            "a number as value",
            json!({"title": "renamed"}),
            json!({"updatedFields": {"properties": {"p1": 3}}}),
        ),
    ] {
        let response = th
            .client
            .patch(
                "/boards-and-blocks",
                &json!({
                    "boardIds": [board_id], "boardPatches": [board_patch],
                    "blockIds": [block_id], "blockPatches": [block_patch],
                }),
            )
            .await;
        check_refused(&response, what);
    }
    let board: Board = th
        .client
        .get(&format!("/boards/{board_id}"))
        .await
        .ok_json();
    assert_eq!(board.title, "Both", "the board is as it was");
    let blocks = blocks_of(&th.client, board_id).await;
    assert_eq!(blocks[0].title, "Card", "and so is the block");
    th.teardown().await;
}

/// An archive of the previous version with one board and one card on it.
fn archive(card_properties: &Value, values: Value) -> Vec<u8> {
    let board = json!({"type": "board", "data": {
        "id": "b1", "teamId": TEST_TEAM_ID, "type": "P", "title": "Imported",
        "cardProperties": card_properties, "createAt": 1, "updateAt": 1,
    }});
    let block = json!({"type": "block", "data": card_block("b1", values)});
    let options = zip::write::SimpleFileOptions::default();
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    writer.start_file("version.json", options).expect("start");
    writer
        .write_all(br#"{"version":2,"date":1}"#)
        .expect("write");
    writer.start_file("b1/board.jsonl", options).expect("start");
    writer
        .write_all(format!("{board}\n{block}\n").as_bytes())
        .expect("write");
    writer.finish().expect("finish").into_inner()
}

/// An archive is another way in, open to every signed-in user.
#[tokio::test]
async fn an_archive_with_malformed_property_data_is_refused() {
    let th = Setup::native().start().await.init_basic().await;

    let response = import(
        &th.client,
        TEST_TEAM_ID,
        &archive(&json!([{"name": "no id"}]), json!({})),
    )
    .await;
    check_refused(&response, "an archive with a definition without id");
    assert_eq!(board_count(&th.client).await, 0, "no board was imported");

    let response = import(
        &th.client,
        TEST_TEAM_ID,
        &archive(&json!([{"id": "p1"}]), json!({"p1": 3})),
    )
    .await;
    check_refused(&response, "an archive with a number as value");
    assert_eq!(
        board_count(&th.client).await,
        0,
        "the board of the archive is not left behind without its card"
    );

    // The same archive, well-formed, is imported.
    import(
        &th.client,
        TEST_TEAM_ID,
        &archive(&json!([{"id": "p1"}]), json!({"p1": "v"})),
    )
    .await
    .check_no_content();
    th.teardown().await;
}

/// A date property holds a date value (`docs/s6-design.md` 5.7): a calendar date, a
/// range of them or a time, or `null`. Text in its place is refused, and so is a date
/// value in another property, on the routes of cards and of blocks.
#[tokio::test]
async fn date_properties_hold_date_values() {
    let th = Setup::native().start().await.init_basic().await;
    let board: Board = new_board(
        &th.client,
        json!([
            {"id": "due", "name": "Due", "type": "date"},
            {"id": "note", "name": "Note", "type": "text"},
        ]),
    )
    .await
    .created_json();
    let cards = format!("/boards/{}/cards", board.id);

    for due in [
        json!({"from": "2026-03-05"}),
        json!({"from": "2026-03-05", "to": "2026-03-09"}),
        json!({"from": 1_772_703_000_000_i64}),
        Value::Null,
    ] {
        let created: Value = th
            .client
            .post(
                &cards,
                &json!({"title": "Card", "properties": {"due": due}}),
            )
            .await
            .created_json();
        assert_eq!(created["properties"]["due"], due, "{created}");
    }
    let card: Card = th
        .client
        .post(
            &cards,
            &json!({"title": "Card", "properties": {"note": "x"}}),
        )
        .await
        .created_json();

    for (what, properties) in [
        ("a date as text", json!({"due": "{\"from\":1772679600000}"})),
        ("no calendar day", json!({"due": {"from": "2026-02-30"}})),
        (
            "a time zone",
            json!({"due": {"from": 1, "timeZone": "UTC"}}),
        ),
        (
            "to before from",
            json!({"due": {"from": "2026-03-05", "to": "2026-03-01"}}),
        ),
        (
            "a date in a text property",
            json!({"note": {"from": "2026-03-05"}}),
        ),
        (
            "a date in no property",
            json!({"other": {"from": "2026-03-05"}}),
        ),
    ] {
        check_refused(
            &th.client
                .post(&cards, &json!({"title": "Card", "properties": properties}))
                .await,
            what,
        );
        check_refused(
            &th.client
                .patch(
                    &format!("/cards/{}", card.id),
                    &json!({"updatedProperties": properties}),
                )
                .await,
            what,
        );
        check_refused(
            &th.client
                .post(
                    &format!("/boards/{}/blocks", board.id),
                    &json!([card_block(&board.id, properties)]),
                )
                .await,
            what,
        );
    }
    let stored: Card = th
        .client
        .get(&format!("/cards/{}", card.id))
        .await
        .ok_json();
    assert_eq!(
        stored.properties, card.properties,
        "the refused patches changed nothing"
    );

    th.teardown().await;
}

/// A change of the board's definitions leaves the cards' values as they are: a card
/// whose value no longer fits is still written, copied and exported, and the value is
/// checked when it is written again. A copy clears such a value.
#[tokio::test]
async fn values_left_behind_by_a_change_of_definitions_stay_until_written() {
    let th = Setup::native().start().await.init_basic().await;
    let board: Board = new_board(
        &th.client,
        json!([
            {"id": "due", "name": "Due", "type": "date"},
            {"id": "note", "name": "Note", "type": "text"},
        ]),
    )
    .await
    .created_json();
    let card: Card = th
        .client
        .post(
            &format!("/boards/{}/cards", board.id),
            &json!({"title": "Card", "properties": {"due": {"from": "2026-03-05"}, "note": "x"}}),
        )
        .await
        .created_json();
    th.client
        .patch(
            &format!("/boards/{}", board.id),
            &json!({"deletedCardProperties": ["due"],
                    "updatedCardProperties": [{"id": "note", "name": "Note", "type": "date"}]}),
        )
        .await
        .check_ok();

    let path = format!("/cards/{}", card.id);
    let renamed: Card = th
        .client
        .patch(&path, &json!({"title": "Renamed"}))
        .await
        .ok_json();
    assert_eq!(renamed.properties, card.properties, "the values stay");
    check_refused(
        &th.client
            .patch(&path, &json!({"updatedProperties": {"note": "y"}}))
            .await,
        "a text written to what is now a date",
    );
    th.client
        .patch(
            &path,
            &json!({"updatedProperties": {"note": {"from": "2026-03-06"}}}),
        )
        .await
        .check_ok();

    let copies: Vec<Value> = th
        .client
        .post_empty(&format!(
            "/boards/{}/blocks/{}/duplicate",
            board.id, card.id
        ))
        .await
        .created_json();
    let copy = copies
        .iter()
        .find(|block| block["type"] == json!("card"))
        .expect("the copy");
    let copy_values = &copy["fields"]["properties"];
    assert!(
        copy_values["due"].is_null(),
        "the date of the deleted property is cleared in the copy: {copy}"
    );
    assert_eq!(copy_values["note"], json!({"from": "2026-03-06"}), "{copy}");
    th.client
        .post_empty(&format!("/boards/{}/duplicate", board.id))
        .await
        .check_status(StatusCode::CREATED);

    th.teardown().await;
}

/// The routes of block patches and the archive take date values as the card routes do,
/// and an archive of the previous version has its date values rewritten.
#[tokio::test]
async fn date_values_on_the_patch_routes_and_in_an_archive() {
    let th = Setup::native().start().await.init_basic().await;
    let (board, card) = board_with_card(&th).await;
    th.client
        .patch(
            &format!("/boards/{}", board.id),
            &json!({"updatedCardProperties": [{"id": "due", "name": "Due", "type": "date"}]}),
        )
        .await
        .check_ok();
    let block_path = format!("/boards/{}/blocks/{}", board.id, card.id);
    let text = json!({"updatedFields": {"properties": {"p1": "open", "due": "soon"}}});
    check_refused(&th.client.patch(&block_path, &text).await, "patchBlock");
    let request = json!({
        "boardIds": [board.id], "boardPatches": [{"title": "n"}],
        "blockIds": [card.id], "blockPatches": [text],
    });
    check_refused(
        &th.client.patch("/boards-and-blocks", &request).await,
        "patchBoardsAndBlocks",
    );
    let date =
        json!({"updatedFields": {"properties": {"p1": "open", "due": {"from": "2026-03-05"}}}});
    th.client.patch(&block_path, &date).await.check_no_content();

    import(
        &th.client,
        TEST_TEAM_ID,
        &archive(
            &json!([{"id": "due", "name": "Due", "type": "date"}]),
            json!({"due": "{\"from\":1772679600000}"}),
        ),
    )
    .await
    .check_no_content();
    let boards: Vec<Board> = th
        .client
        .get(&format!("/teams/{TEST_TEAM_ID}/boards"))
        .await
        .ok_json();
    let imported = boards
        .iter()
        .find(|board| board.title == "Imported")
        .expect("the imported board");
    let blocks = blocks_of(&th.client, &imported.id).await;
    assert_eq!(
        blocks[0].fields["properties"],
        json!({"due": {"from": "2026-03-05"}}),
        "the previous shape became the new one"
    );

    th.teardown().await;
}
