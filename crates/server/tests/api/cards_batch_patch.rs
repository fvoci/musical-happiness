//! Several cards of a board changed alike in one request (`batchPatchCards`): all or
//! none, each as `patchCard` changes one, and nothing of a card that is not one of the
//! board's, nor of a template of its cards.

use fvoci_model::{Card, CardPropertyDefinition, Container, ContainerVisibility, PropertyType};
use reqwest::StatusCode;
use serde_json::{Value, json};

use crate::boards::new_board;
use crate::common::{Client, Setup, TEST_WORKSPACE_ID, TEST_WS};

/// The path of the batch patch of the cards of `board_id`.
pub(crate) fn batch_patch_of(board_id: &str) -> String {
    format!("{TEST_WS}/boards/{board_id}/cards/batch-patch")
}

/// A board with a text property `note` and a number property `size`.
async fn board_with_values(client: &Client) -> Container {
    client
        .create_board(&Container {
            card_properties: vec![
                CardPropertyDefinition::new("note", "Note", PropertyType::Text),
                CardPropertyDefinition::new("size", "Size", PropertyType::Number),
            ],
            ..new_board(TEST_WORKSPACE_ID, ContainerVisibility::Open, "Batched")
        })
        .await
        .created_json()
}

/// A card of `board_id` made from `card`.
async fn card_of(client: &Client, board_id: &str, card: &Value) -> Card {
    client
        .post(&format!("{TEST_WS}/boards/{board_id}/cards"), card)
        .await
        .created_json()
}

/// The card as `client` reads it.
async fn read_card(client: &Client, card_id: &str) -> Card {
    client
        .get(&format!("{TEST_WS}/cards/{card_id}"))
        .await
        .ok_json()
}

/// The cards are changed alike, each keeping its other values, and answered in the
/// order of the request.
#[tokio::test]
async fn the_cards_are_changed_alike_and_answered_in_the_order_asked() {
    let ts = Setup::native().start().await.init_basic().await;
    let board = board_with_values(&ts.client).await;
    let first = card_of(
        &ts.client,
        &board.id,
        &json!({"title": "first", "properties": {"note": "mine", "size": "1"}}),
    )
    .await;
    let second = card_of(
        &ts.client,
        &board.id,
        &json!({"title": "second", "properties": {"size": "2"}}),
    )
    .await;
    let untouched = card_of(&ts.client, &board.id, &json!({"title": "third"})).await;

    let patched: Vec<Card> = ts
        .client
        .post(
            &batch_patch_of(&board.id),
            &json!({
                "cardIds": [second.id, first.id],
                "updatedProperties": {"note": "shared"}, "deletedProperties": ["size"],
            }),
        )
        .await
        .ok_json();
    let ids: Vec<&str> = patched.iter().map(|card| card.id.as_str()).collect();
    assert_eq!(ids, [second.id.as_str(), first.id.as_str()], "as asked");
    for card in &patched {
        assert_eq!(
            Value::Object(card.properties.clone()),
            json!({"note": "shared"}),
            "{card:?}"
        );
        assert_eq!(&read_card(&ts.client, &card.id).await, card, "as stored");
    }
    assert_eq!(read_card(&ts.client, &untouched.id).await, untouched);

    ts.teardown().await;
}

/// What the request refuses of itself is named at once: no card, too many, a card named
/// twice, a value that is none.
#[tokio::test]
async fn a_batch_names_what_it_refuses_of_itself() {
    let ts = Setup::native().start().await.init_basic().await;
    let board = board_with_values(&ts.client).await;
    let card = card_of(&ts.client, &board.id, &json!({"title": "card"})).await;
    let path = batch_patch_of(&board.id);

    let response = ts.client.post(&path, &json!({"cardIds": []})).await;
    response.check_bad_request();
    assert_eq!(response.refused_at(), "/cardIds");
    let many: Vec<String> = (0..201).map(|_| fvoci_model::new_id()).collect();
    let response = ts.client.post(&path, &json!({"cardIds": many})).await;
    response.check_bad_request();
    assert_eq!(
        (response.refused_at(), &response.value()["limit"]),
        ("/cardIds".to_owned(), &json!(200)),
        "{}",
        response.text()
    );
    let response = ts
        .client
        .post(
            &path,
            &json!({
                "cardIds": [card.id, "not an id", card.id],
                "updatedProperties": {"size": 7},
            }),
        )
        .await;
    response.check_bad_request();
    assert_eq!(
        response.refused_places(),
        ["/cardIds/2", "/updatedProperties/size"],
        "{}",
        response.text()
    );
    let response = ts
        .client
        .post(&path, &json!({"cardIds": [card.id], "properties": {}}))
        .await;
    response.check_bad_request();
    assert_eq!(
        read_card(&ts.client, &card.id).await,
        card,
        "nothing changed"
    );

    ts.teardown().await;
}

/// A card of another board, a block that is no card, a card that is not there and text
/// that is no identifier are not found, a template of the board's cards is refused, a value the board refuses is
/// named as `patchCard` names it; and then no card of the batch is changed.
#[tokio::test]
async fn a_batch_changes_no_card_when_one_is_refused() {
    let ts = Setup::native().start().await.init_basic().await;
    let board = board_with_values(&ts.client).await;
    let other = board_with_values(&ts.client).await;
    let card = card_of(&ts.client, &board.id, &json!({"title": "card"})).await;
    let template = card_of(
        &ts.client,
        &board.id,
        &json!({"title": "template", "isTemplate": true}),
    )
    .await;
    let elsewhere = card_of(&ts.client, &other.id, &json!({"title": "elsewhere"})).await;
    let view = crate::board_views::add_view(&ts.client, &board.id, "View").await;
    let path = batch_patch_of(&board.id);
    let change = |other_id: &str| json!({"cardIds": [card.id, other_id], "updatedProperties": {"note": "changed"}});

    for other_id in [
        elsewhere.id.as_str(),
        view.id.as_str(),
        &fvoci_model::new_id(),
        "not an id",
    ] {
        let response = ts.client.post(&path, &change(other_id)).await;
        response.check_not_found();
        assert_eq!(response.error_message(), "card not found", "{other_id}");
    }
    let response = ts.client.post(&path, &change(&template.id)).await;
    response.check_bad_request();
    assert_eq!(response.refused_at(), "/cardIds/1", "{}", response.text());
    let response = ts
        .client
        .post(
            &path,
            &json!({"cardIds": [card.id], "updatedProperties": {"size": "many"}}),
        )
        .await;
    let single = ts
        .client
        .patch(
            &format!("{TEST_WS}/cards/{}", card.id),
            &json!({"updatedProperties": {"size": "many"}}),
        )
        .await;
    assert_eq!(
        (response.status, response.refused_places()),
        (single.status, single.refused_places()),
        "as patchCard refuses it: {} {}",
        response.text(),
        single.text()
    );
    assert_eq!(response.status, StatusCode::BAD_REQUEST);
    assert_eq!(
        read_card(&ts.client, &card.id).await,
        card,
        "nothing changed"
    );
    assert_eq!(read_card(&ts.client, &elsewhere.id).await, elsewhere);

    ts.teardown().await;
}
