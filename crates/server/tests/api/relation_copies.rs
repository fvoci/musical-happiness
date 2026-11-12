//! Card relations in copies: a copy of a board (also as a template, and a board made
//! from one) and an imported archive name the copies of the cards their values named,
//! and leave out what they cannot name so. A copy of a card on its own board names the
//! cards its source named.

use std::collections::HashMap;

use fvoci_model::{Block, Board, BoardsAndBlocks, Card, new_id};
use serde_json::{Value, json};

use crate::common::TEST_WS;
use crate::common::{Client, Setup, TEST_WORKSPACE_ID, TestServer};
use crate::export::import;

const RELATION: &str = "rel";

/// A board whose cards name each other: `B` names `A` and `C`, `A` names a card that is
/// in the trash (`gone`) and, as an older server stored it, one that never was.
struct Named {
    board: Board,
    /// The cards by title.
    cards: HashMap<String, String>,
    view: String,
}

async fn named_board(th: &TestServer) -> Named {
    let client = &th.client;
    let board: Board = client
        .post(
            &format!("{TEST_WS}/boards"),
            &json!({
                "workspaceId": TEST_WORKSPACE_ID, "type": "P", "title": "Named",
                "cardProperties": [{"id": RELATION, "name": "After", "type": "relation"}],
            }),
        )
        .await
        .created_json();
    let mut cards = HashMap::new();
    for title in ["A", "B", "C", "gone"] {
        let card: Card = client
            .post(
                &format!("{TEST_WS}/boards/{}/cards", board.id),
                &json!({"title": title}),
            )
            .await
            .created_json();
        cards.insert(title.to_owned(), card.id);
    }
    let id = |title: &str| cards[title].clone();
    let view: Vec<Block> = client
        .post(
            &format!("{TEST_WS}/boards/{}/blocks", board.id),
            &json!([{
                "id": "v", "boardId": board.id, "parentId": board.id, "type": "view",
                "title": "Gantt", "schema": 1, "createAt": 1, "updateAt": 1,
                "fields": {"viewType": "gantt", "dependencyPropertyId": RELATION},
            }]),
        )
        .await
        .created_json();
    client
        .patch(
            &format!("{TEST_WS}/cards/{}", id("B")),
            &json!({"updatedProperties": {RELATION: [id("A"), id("C")]}}),
        )
        .await
        .check_ok();
    client
        .patch(
            &format!("{TEST_WS}/cards/{}", id("A")),
            &json!({"updatedProperties": {RELATION: [id("gone")]}}),
        )
        .await
        .check_ok();
    client
        .delete(&format!(
            "{TEST_WS}/boards/{}/blocks/{}",
            board.id,
            id("gone")
        ))
        .await
        .check_no_content();
    let stale = json!({"properties": {RELATION: [id("gone"), new_id(), id("A")]}});
    th.app
        .store()
        .set_block_fields_unchecked(&id("A"), stale.as_object().expect("an object"))
        .await
        .expect("written as an older server would have");
    Named {
        board,
        cards,
        view: view[0].id.clone(),
    }
}

/// The relation values of a board's cards by title, the cards they name by title too
/// (`?` for what names no card of the board).
async fn relations_by_title(client: &Client, board_id: &str) -> HashMap<String, Value> {
    let blocks: Vec<Block> = client
        .get(&format!("{TEST_WS}/boards/{board_id}/blocks"))
        .await
        .ok_json();
    let titles: HashMap<&str, &str> = blocks
        .iter()
        .filter(|block| block.block_type == fvoci_model::BlockType::Card)
        .map(|block| (block.id.as_str(), block.title.as_str()))
        .collect();
    blocks
        .iter()
        .filter(|block| block.block_type == fvoci_model::BlockType::Card)
        .map(|block| {
            let value = match block.fields["properties"].get(RELATION) {
                Some(Value::Array(ids)) => Value::Array(
                    ids.iter()
                        .map(|id| json!(id.as_str().and_then(|id| titles.get(id)).unwrap_or(&"?")))
                        .collect(),
                ),
                other => other.cloned().unwrap_or(Value::Null),
            };
            (block.title.clone(), value)
        })
        .collect()
}

fn dependency_of(blocks: &[Block]) -> Option<&Value> {
    blocks
        .iter()
        .find(|block| block.block_type == fvoci_model::BlockType::View)
        .and_then(|view| view.fields.get("dependencyPropertyId"))
}

#[tokio::test]
async fn a_copy_of_a_board_names_its_own_cards() {
    let th = Setup::native().start().await.init_basic().await;
    let Named { board, .. } = named_board(&th).await;

    let copy: BoardsAndBlocks = th
        .client
        .post_empty(&format!("{TEST_WS}/boards/{}/duplicate", board.id))
        .await
        .created_json();
    let copied = &copy.boards[0];
    let values = relations_by_title(&th.client, &copied.id).await;
    assert_eq!(values["B"], json!(["A", "C"]), "the copies of A and C");
    assert_eq!(
        values["A"],
        json!([]),
        "a card in the trash is not copied, and a card that never was is not named"
    );
    assert!(!values.contains_key("gone"));
    assert_eq!(
        dependency_of(&copy.blocks),
        Some(&json!(RELATION)),
        "the view names the same property"
    );
    th.teardown().await;
}

#[tokio::test]
async fn a_template_and_a_board_made_from_it_name_their_own_cards() {
    let th = Setup::native().start().await.init_basic().await;
    let Named { board, cards, .. } = named_board(&th).await;

    let template: BoardsAndBlocks = th
        .client
        .post_empty(&format!(
            "{TEST_WS}/boards/{}/duplicate?asTemplate=true",
            board.id
        ))
        .await
        .created_json();
    let template = &template.boards[0];
    let made: BoardsAndBlocks = th
        .client
        .post_empty(&format!("{TEST_WS}/boards/{}/duplicate", template.id))
        .await
        .created_json();
    let made = &made.boards[0];
    for board_id in [&template.id, &made.id] {
        let values = relations_by_title(&th.client, board_id).await;
        assert_eq!(values["B"], json!(["A", "C"]), "{board_id}");
    }
    let blocks: Vec<Block> = th
        .client
        .get(&format!("{TEST_WS}/boards/{}/blocks", made.id))
        .await
        .ok_json();
    let named_ids: Vec<&Value> = blocks
        .iter()
        .filter_map(|block| block.fields.get("properties")?.get(RELATION))
        .filter_map(Value::as_array)
        .flatten()
        .collect();
    assert!(
        named_ids.iter().all(|id| !cards
            .values()
            .any(|source| Some(source.as_str()) == id.as_str())),
        "no card of the source is named: {named_ids:?}"
    );
    th.teardown().await;
}

#[tokio::test]
async fn an_imported_archive_names_the_imported_cards() {
    let th = Setup::native().start().await.init_basic().await;
    let Named { board, .. } = named_board(&th).await;

    let archive = th
        .client
        .get(&format!("{TEST_WS}/boards/{}/archive/export", board.id))
        .await;
    archive.check_ok();
    import(&th.client, TEST_WORKSPACE_ID, &archive.body)
        .await
        .check_ok();
    let boards: Vec<Board> = th.client.get(&format!("{TEST_WS}/boards")).await.ok_json();
    let imported = boards
        .iter()
        .find(|each| each.id != board.id && each.title == board.title)
        .expect("the imported board");
    let values = relations_by_title(&th.client, &imported.id).await;
    assert_eq!(values["B"], json!(["A", "C"]));
    assert_eq!(
        values["A"],
        json!([]),
        "what is not in the archive is left out"
    );
    th.teardown().await;
}

#[tokio::test]
async fn a_copy_of_a_card_names_the_cards_its_source_named() {
    let th = Setup::native().start().await.init_basic().await;
    let Named {
        board, cards, view, ..
    } = named_board(&th).await;

    for (source, expected) in [("B", json!(["A", "C"])), ("A", json!(["gone"]))] {
        let copies: Vec<Block> = th
            .client
            .post_empty(&format!(
                "{TEST_WS}/boards/{}/blocks/{}/duplicate",
                board.id, cards[source]
            ))
            .await
            .created_json();
        let named: Vec<&str> = copies[0].fields["properties"][RELATION]
            .as_array()
            .expect("a list")
            .iter()
            .filter_map(Value::as_str)
            .collect();
        let titles: Vec<&str> = named
            .iter()
            .map(|id| {
                cards
                    .iter()
                    .find(|(_, card)| card.as_str() == *id)
                    .map_or("?", |(title, _)| title.as_str())
            })
            .collect();
        assert_eq!(
            json!(titles),
            expected,
            "{source}: the same cards, the one in the trash too; what never was, and the source itself, are left out"
        );
    }
    let blocks: Vec<Block> = th
        .client
        .get(&format!("{TEST_WS}/boards/{}/blocks", board.id))
        .await
        .ok_json();
    assert!(blocks.iter().any(|block| block.id == view));
    th.teardown().await;
}

/// A relation is a card's: another block's value names nothing, is not checked, and
/// does not stop a copy.
#[tokio::test]
async fn a_relation_value_of_a_block_that_is_no_card_does_not_stop_a_copy() {
    let th = Setup::native().start().await.init_basic().await;
    let Named { board, cards, .. } = named_board(&th).await;
    th.client
        .post(
            &format!("{TEST_WS}/boards/{}/blocks", board.id),
            &json!([{
                "id": "v", "boardId": board.id, "parentId": board.id, "type": "view",
                "title": "Odd", "schema": 1, "createAt": 1, "updateAt": 1,
                "fields": {"properties": {RELATION: [cards["A"], new_id()]}},
            }]),
        )
        .await
        .check_created();
    let copy: BoardsAndBlocks = th
        .client
        .post_empty(&format!("{TEST_WS}/boards/{}/duplicate", board.id))
        .await
        .created_json();
    let values = relations_by_title(&th.client, &copy.boards[0].id).await;
    assert_eq!(values["B"], json!(["A", "C"]));
    th.teardown().await;
}
