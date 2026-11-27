//! Cards that repeat: completing one makes the next in the same write, once; the next
//! starts not done, due one period on in the completer's day, with its tasks unchecked;
//! only a board with a rule of done and a due date property lets a card repeat, and a
//! template does not.

use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use fvoci_model::{
    Block, BlockError, BlockPatch, BlockType, Body, CardEventKind, CardPropertyDefinition,
    Container, ContainerVisibility, DoneWhen, DuplicateAs, PropertyColor, PropertyOption,
    PropertyType, UserSettingsPatch, new_id,
};
use fvoci_store::{BlockFilter, Paging, Previews, Store, StoreError};
use serde_json::{Value, json};

use crate::common::{self, TestStore};

/// 2027-01-15 12:00 UTC: the 16th already at UTC+14.
const NOON: i64 = 1_800_014_400_000;

const STATUS: &str = "status";
const DUE: &str = "due";
const TODO: &str = "todo";
const DOING: &str = "doing";
const DONE: &str = "done";

fn status() -> CardPropertyDefinition {
    CardPropertyDefinition {
        id: STATUS.to_owned(),
        name: "Status".to_owned(),
        property_type: PropertyType::Select,
        options: vec![
            PropertyOption::new(DONE.to_owned(), "Done", PropertyColor::Green),
            PropertyOption::new(TODO.to_owned(), "Todo", PropertyColor::Gray),
            PropertyOption::new(DOING.to_owned(), "Doing", PropertyColor::Blue),
        ],
    }
}

/// A board on which cards repeat: a status whose `done` is done, and a due date.
async fn board(test: &TestStore, user_id: &str) -> String {
    let rule = DoneWhen {
        property_id: STATUS.to_owned(),
        option_ids: vec![DONE.to_owned()],
    };
    board_ruled(test, user_id, Some(rule)).await
}

/// The same properties and `done_when` as the rule of done.
async fn board_ruled(test: &TestStore, user_id: &str, done_when: Option<DoneWhen>) -> String {
    let workspace_id = common::workspace_of(test, &[user_id]).await;
    let mut board = Container {
        id: new_id(),
        workspace_id,
        visibility: ContainerVisibility::Open,
        title: "chores".to_owned(),
        card_properties: vec![
            status(),
            CardPropertyDefinition::new(DUE, "Due", PropertyType::Date),
        ],
        done_when,
        ..Container::default()
    };
    test.insert_container(&mut board, user_id)
        .await
        .expect("the board is stored");
    board.id
}

fn weekly() -> Value {
    json!({"every": 1, "unit": "week", "anchor": "due"})
}

/// A card doing, due on the 15th, that repeats weekly, with a task done and an icon.
async fn chore(store: &Store, board_id: &str, user_id: &str) -> Block {
    let mut card = Block {
        id: new_id(),
        container_id: board_id.to_owned(),
        block_type: BlockType::Card,
        title: "water the plants".to_owned(),
        body: Some(Body::from_stored(json!({"type": "doc", "content": [
            {"type": "taskList", "content": [{"type": "taskItem", "attrs": {"checked": true},
                "content": [{"type": "paragraph", "content": [{"type": "text", "text": "the fern"}]}]}]}
        ]}))),
        ..Block::default()
    };
    card.fields.insert("icon".to_owned(), json!("🌿"));
    card.fields.insert(
        "properties".to_owned(),
        json!({STATUS: DOING, DUE: {"from": "2027-01-15"}}),
    );
    card.fields.insert("recurrence".to_owned(), weekly());
    store
        .insert_block(&mut card, user_id)
        .await
        .expect("the card is stored");
    card
}

fn set_status(value: &str) -> BlockPatch {
    let mut patch = BlockPatch::default();
    patch
        .property_changes
        .updated
        .insert(STATUS.to_owned(), json!(value));
    patch
}

async fn set(store: &Store, card: &Block, patch: &BlockPatch, user_id: &str) -> Block {
    store
        .patch_block(&card.id, patch, user_id)
        .await
        .expect("the card is changed")
}

/// The cards of the board, oldest first.
async fn cards(store: &Store, board_id: &str) -> Vec<Block> {
    store
        .get_listed_blocks(&BlockFilter::container(board_id), Previews::Without)
        .await
        .expect("the cards are listed")
        .into_iter()
        .map(|listed| listed.block)
        .filter(|block| block.block_type == BlockType::Card)
        .collect()
}

async fn block(store: &Store, id: &str) -> Block {
    store.get_block(id).await.expect("the card is there")
}

fn at_noon(store: &Store) {
    let now = Arc::new(AtomicI64::new(NOON));
    store.clock().set_source(move || now.load(Ordering::SeqCst));
}

#[tokio::test]
async fn completing_a_card_that_repeats_makes_the_next_one() {
    let test = common::setup().await;
    at_noon(&test);
    let user = common::user(&test, "gardener").await;
    let board_id = board(&test, &user).await;
    let card = chore(&test, &board_id, &user).await;

    let completed = set(&test, &card, &set_status(DONE), &user).await;
    let next_id = completed.fields["recurredTo"]
        .as_str()
        .expect("the completed card names the next")
        .to_owned();
    assert_eq!(completed.fields.get("recurrence"), None, "handed on");
    let next = block(&test, &next_id).await;
    assert_eq!(next.title, card.title);
    assert_eq!(next.fields["icon"], json!("🌿"));
    assert_eq!(next.parent_id, card.parent_id);
    assert_eq!(next.created_by, user);
    assert_eq!(next.number, Some(2), "a card of its own");
    assert_eq!(next.fields["recurrence"], weekly());
    assert_eq!(next.fields.get("recurredTo"), None);
    assert_eq!(
        next.fields["properties"],
        json!({STATUS: TODO, DUE: {"from": "2027-01-22"}}),
        "not done, due a week on"
    );
    let body = next.body.as_ref().expect("the body is copied");
    assert_eq!((body.tasks().total, body.tasks().done), (1, 0));
    assert_eq!(card.body.as_ref().map(|body| body.tasks().done), Some(1));

    // The completion is one change of the card; the next card was made.
    let every = Paging {
        page: 0,
        per_page: std::num::NonZeroU32::new(100).expect("a page size"),
    };
    let lines = test.card_events(&card.id, every).await.expect("read");
    assert_eq!(
        lines.iter().map(|line| line.kind).collect::<Vec<_>>(),
        [CardEventKind::Property, CardEventKind::Created]
    );
    let lines = test.card_events(&next_id, every).await.expect("read");
    assert_eq!(
        lines
            .iter()
            .map(|line| (line.kind, line.actor_id.as_deref()))
            .collect::<Vec<_>>(),
        [(CardEventKind::Created, Some(user.as_str()))]
    );
    test.teardown().await;
}

#[tokio::test]
async fn a_card_completed_reopened_and_completed_again_has_one_next_card() {
    let test = common::setup().await;
    at_noon(&test);
    let user = common::user(&test, "gardener").await;
    let board_id = board(&test, &user).await;
    let card = chore(&test, &board_id, &user).await;
    let first = set(&test, &card, &set_status(DONE), &user).await;
    set(&test, &card, &set_status(TODO), &user).await;
    let again = set(&test, &card, &set_status(DONE), &user).await;
    assert_eq!(cards(&test, &board_id).await.len(), 2);
    assert_eq!(again.fields["recurredTo"], first.fields["recurredTo"]);
    test.teardown().await;
}

#[tokio::test]
async fn a_completion_undone_and_done_again_makes_a_fresh_next_card() {
    let test = common::setup().await;
    at_noon(&test);
    let user = common::user(&test, "gardener").await;
    let board_id = board(&test, &user).await;
    let card = chore(&test, &board_id, &user).await;
    let first = set(&test, &card, &set_status(DONE), &user).await;
    let first_next = first.fields["recurredTo"]
        .as_str()
        .expect("named")
        .to_owned();
    // The undo: the status and the recurrence back, the next card gone.
    let mut undo = set_status(DOING);
    undo.updated_fields
        .insert("recurrence".to_owned(), weekly());
    let undone = set(&test, &card, &undo, &user).await;
    assert_eq!(
        undone.fields.get("recurredTo"),
        None,
        "set again, it repeats anew"
    );
    test.delete_block(&first_next, &user)
        .await
        .expect("the next card goes");

    let again = set(&test, &card, &set_status(DONE), &user).await;
    let second_next = again.fields["recurredTo"].as_str().expect("named");
    assert_ne!(second_next, first_next);
    assert_eq!(cards(&test, &board_id).await.len(), 2);
    test.teardown().await;
}

#[tokio::test]
async fn the_next_card_is_due_after_the_completers_today() {
    let test = common::setup().await;
    at_noon(&test);
    let user = common::user(&test, "far-east").await;
    test.patch_user_settings(
        &user,
        &UserSettingsPatch::from_members(
            json!({"timeZone": "Pacific/Kiritimati"})
                .as_object()
                .expect("members"),
        )
        .expect("a change of the settings"),
    )
    .await
    .expect("the zone is set");
    let board_id = board(&test, &user).await;
    let card = chore(&test, &board_id, &user).await;
    let mut daily_from_done = BlockPatch::default();
    daily_from_done.updated_fields.insert(
        "recurrence".to_owned(),
        json!({"every": 1, "unit": "day", "anchor": "done"}),
    );
    set(&test, &card, &daily_from_done, &user).await;
    let completed = set(&test, &card, &set_status(DONE), &user).await;
    let next = block(
        &test,
        completed.fields["recurredTo"].as_str().expect("named"),
    )
    .await;
    // Noon UTC on the 15th is the 16th at UTC+14: the day after is the 17th.
    assert_eq!(
        next.fields["properties"][DUE],
        json!({"from": "2027-01-17"})
    );
    test.teardown().await;
}

#[tokio::test]
async fn a_card_repeats_only_on_a_board_with_rules_and_never_as_a_template() {
    let test = common::setup().await;
    at_noon(&test);
    let user = common::user(&test, "gardener").await;
    let bare = board_ruled(&test, &user, None).await;
    let mut card = Block {
        id: new_id(),
        container_id: bare.clone(),
        block_type: BlockType::Card,
        title: "sweep".to_owned(),
        ..Block::default()
    };
    card.fields.insert("recurrence".to_owned(), weekly());
    let refused = test
        .insert_block(&mut card.clone(), &user)
        .await
        .expect_err("no rules, no recurrence");
    assert!(
        matches!(
            refused,
            StoreError::InvalidBlock {
                source: BlockError::RecurrenceNeedsBoardRules,
                ..
            }
        ),
        "{refused:?}"
    );
    card.fields.remove("recurrence");
    test.insert_block(&mut card, &user)
        .await
        .expect("a card that does not repeat");
    let mut repeat = BlockPatch::default();
    repeat
        .updated_fields
        .insert("recurrence".to_owned(), weekly());
    assert!(matches!(
        test.patch_block(&card.id, &repeat, &user).await,
        Err(StoreError::InvalidBlock {
            source: BlockError::RecurrenceNeedsBoardRules,
            ..
        })
    ));

    // A template keeps how it repeats, and makes no next card.
    let ruled = board(&test, &user).await;
    let template = chore(&test, &ruled, &user).await;
    let mut make_template = BlockPatch::default();
    make_template
        .updated_fields
        .insert("isTemplate".to_owned(), json!(true));
    set(&test, &template, &make_template, &user).await;
    let completed = set(&test, &template, &set_status(DONE), &user).await;
    assert_eq!(completed.fields.get("recurredTo"), None);
    assert_eq!(completed.fields["recurrence"], weekly());
    assert_eq!(cards(&test, &ruled).await.len(), 1);
    test.teardown().await;
}

#[tokio::test]
async fn a_copy_repeats_anew_and_the_server_alone_names_the_next_card() {
    let test = common::setup().await;
    at_noon(&test);
    let user = common::user(&test, "gardener").await;
    let board_id = board(&test, &user).await;
    let card = chore(&test, &board_id, &user).await;
    let completed = set(&test, &card, &set_status(DONE), &user).await;
    let copies = test
        .duplicate_block(&board_id, &card.id, &user, DuplicateAs::Copy)
        .await
        .expect("the completed card is copied");
    assert_eq!(
        copies[0].fields.get("recurredTo"),
        None,
        "a copy names no next card"
    );

    // What a write sends as the next card is not written.
    let mut forged = BlockPatch::default();
    forged
        .updated_fields
        .insert("recurredTo".to_owned(), json!(new_id()));
    let kept = set(&test, &card, &forged, &user).await;
    assert_eq!(kept.fields["recurredTo"], completed.fields["recurredTo"]);
    test.teardown().await;
}
