//! Whom a write may be told to, and what a person was told: the list a write of a
//! block puts its rows on in its own transaction, what the worker takes off it, and
//! the bounds a person's notifications are kept within.

use fvoci_model::{Block, BlockType, Body, Container, ContainerType, NotificationKind, new_id};
use fvoci_store::{
    Making, NotificationEvent, NotificationLimits, NotificationReader, Paging, Store,
    StoredNotification,
};
use serde_json::json;

use crate::common;

/// A time by which every row is due.
const LATER: i64 = i64::MAX;
/// An hour, and no cap anybody reaches.
const LIMITS: NotificationLimits = NotificationLimits {
    same_within_millis: 3_600_000,
    most_of_a_person: 1000,
};

fn mentioning(user_id: &str) -> Option<Body> {
    Body::parse(json!({"type": "doc", "content": [
        {"type": "paragraph", "content": [{"type": "mention", "attrs": {"userId": user_id}}]},
    ]}))
    .expect("a document")
}

/// A board of a new workspace of the store's organization, its maker, and somebody
/// else of the organization.
async fn board_and_two(store: &Store) -> (String, String, String) {
    let writer = common::user(store, &format!("w{}", &new_id()[24..])).await;
    let named = common::user(store, &format!("n{}", &new_id()[24..])).await;
    let workspace_id = common::workspace(store).await;
    let mut boards = common::create_test_boards(store, &workspace_id, &writer, 1).await;
    (boards.remove(0).id, writer, named)
}

fn card(board_id: &str, body: Option<Body>) -> Block {
    Block {
        id: new_id(),
        container_id: board_id.to_owned(),
        block_type: BlockType::Card,
        title: "A card".to_owned(),
        body,
        ..Block::default()
    }
}

async fn events(store: &Store) -> Vec<NotificationEvent> {
    store
        .due_notification_events(LATER, 1000)
        .await
        .expect("the list is read")
}

/// The user, reading with a session made by a password.
fn reader(user_id: &str) -> NotificationReader<'_> {
    NotificationReader {
        user_id,
        making: Making::with_password(),
    }
}

async fn told(store: &Store, user_id: &str) -> Vec<StoredNotification> {
    let all = Paging {
        page: 0,
        per_page: 1000,
    };
    store
        .notifications_of_user(reader(user_id), all)
        .await
        .expect("the notifications are read")
}

/// A write that names somebody puts one row on the list, with the write: a write
/// that fails leaves none. The same noted again before it was worked off moves the
/// row on, and whoever read it before takes nothing off and tells nobody.
#[tokio::test]
async fn a_write_notes_whom_it_names_with_itself_and_a_row_noted_again_is_not_taken_off() {
    let test = common::setup().await;
    let store = &test.store;
    let (board_id, writer, named) = board_and_two(store).await;

    // Two cards in one write, the second on no board: nothing is stored, nor noted.
    let mut both = [
        card(&board_id, mentioning(&named)),
        card(&new_id(), mentioning(&named)),
    ];
    store
        .insert_blocks(&mut both, &writer)
        .await
        .expect_err("a card on no board");
    assert_eq!(events(store).await, [], "nothing of a write that failed");

    let mut block = card(&board_id, mentioning(&named));
    store
        .insert_block(&mut block, &writer)
        .await
        .expect("the card is stored");
    let noted = events(store).await;
    assert_eq!(noted.len(), 1, "{noted:?}");
    let first = &noted[0];
    assert_eq!(
        (
            first.recipient_id.as_str(),
            first.kind,
            first.source_id.as_str(),
            first.block_id.as_str(),
            first.container_id.as_str(),
            first.actor_id.as_str(),
            first.attempts,
        ),
        (
            named.as_str(),
            NotificationKind::Mention,
            block.id.as_str(),
            block.id.as_str(),
            board_id.as_str(),
            writer.as_str(),
            0,
        )
    );
    // The writer names themself: nobody is told of their own doing.
    let mut own = card(&board_id, mentioning(&writer));
    store
        .insert_block(&mut own, &writer)
        .await
        .expect("the card is stored");
    assert_eq!(events(store).await.len(), 1, "nothing of one's own");

    // Out of the body and in again, before the row was worked off.
    for body in [None, mentioning(&named)] {
        let mut again = block.clone();
        again.body = body;
        store
            .insert_block(&mut again, &writer)
            .await
            .expect("the card is written again");
    }
    let again = events(store).await;
    assert_eq!(again.len(), 1, "still one row: {again:?}");
    assert!(again[0].seq > first.seq, "moved on: {again:?}");
    assert_eq!(
        store.tell_of(first, 10, LIMITS).await.expect("asked"),
        None,
        "read before it was noted again"
    );
    assert!(!store.drop_notification_event(first).await.expect("asked"));
    assert_eq!(events(store).await, again, "the row stays as it is");
    assert_eq!(told(store, &named).await, []);

    // A row that could not be worked off waits, and counts its tries.
    store
        .notification_event_failed(&again[0], 500)
        .await
        .expect("noted");
    let due_now = store.due_notification_events(499, 10).await.expect("read");
    assert_eq!(due_now, [], "not due before its time");
    let waited = store.due_notification_events(500, 10).await.expect("read");
    assert_eq!(waited.len(), 1);
    assert_eq!((waited[0].attempts, waited[0].seq), (1, again[0].seq));

    let id = store
        .tell_of(&waited[0], 1000, LIMITS)
        .await
        .expect("told")
        .expect("a notification");
    assert_eq!(events(store).await, [], "taken off with the telling");
    let theirs = told(store, &named).await;
    assert_eq!(theirs.len(), 1);
    assert_eq!(
        (
            theirs[0].id.as_str(),
            theirs[0].create_at,
            theirs[0].read_at
        ),
        (id.as_str(), 1000, 0)
    );
    test.teardown().await;
}

/// The same told again within the hour makes no other notification; after it, or of
/// another kind or from another place, it does.
#[tokio::test]
async fn the_same_within_the_hour_is_told_once() {
    let test = common::setup().await;
    let store = &test.store;
    let (board_id, writer, named) = board_and_two(store).await;
    let mut block = card(&board_id, mentioning(&named));
    store
        .insert_block(&mut block, &writer)
        .await
        .expect("the card is stored");
    let event = events(store).await.remove(0);
    let hour = LIMITS.same_within_millis;
    let start = 10 * hour;
    assert!(
        store
            .tell_of(&event, start, LIMITS)
            .await
            .expect("told")
            .is_some()
    );

    // Out of the body and in again, each time: one row, of the same.
    let noted_again = async || {
        for body in [None, mentioning(&named)] {
            let mut again = block.clone();
            again.body = body;
            store
                .insert_block(&mut again, &writer)
                .await
                .expect("the card is written again");
        }
        let noted = events(store).await;
        assert_eq!(noted.len(), 1, "{noted:?}");
        noted.into_iter().next().expect("one row")
    };
    let within = noted_again().await;
    assert_eq!(
        store
            .tell_of(&within, start + hour - 1, LIMITS)
            .await
            .expect("asked"),
        None,
        "within the hour"
    );
    assert_eq!(
        events(store).await,
        [],
        "and the row is taken off all the same"
    );
    let after = noted_again().await;
    assert!(
        store
            .tell_of(&after, start + hour, LIMITS)
            .await
            .expect("told")
            .is_some(),
        "an hour later"
    );
    assert_eq!(told(store, &named).await.len(), 2);
    test.teardown().await;
}

/// Stores a card of the board that names the user, by the board's writer, and tells
/// the user of it at `at`. The identifier of the notification.
async fn told_of_a_new_card(
    store: &Store,
    (board_id, writer): (&str, &str),
    user_id: &str,
    at: i64,
    limits: NotificationLimits,
) -> String {
    let mut block = card(board_id, mentioning(user_id));
    store
        .insert_block(&mut block, writer)
        .await
        .expect("the card is stored");
    let event = events(store).await.remove(0);
    store
        .tell_of(&event, at, limits)
        .await
        .expect("told")
        .expect("a notification")
}

/// A person has so many notifications and no more: past it the oldest go, those that
/// were read before those that were not. And those read long ago are forgotten.
#[tokio::test]
async fn a_person_keeps_the_newest_and_the_unread_and_what_was_read_long_ago_is_forgotten() {
    let test = common::setup().await;
    let store = &test.store;
    let (board_id, writer, named) = board_and_two(store).await;
    let few = NotificationLimits {
        most_of_a_person: 3,
        ..LIMITS
    };
    // Somebody else has two, the oldest of all and read: nothing of theirs goes
    // for what the other is told.
    let other = common::user(store, &format!("o{}", &new_id()[24..])).await;
    for at in [1_i64, 2] {
        let id = told_of_a_new_card(store, (&board_id, &writer), &other, at, few).await;
        assert!(
            store
                .read_notification(reader(&other), &id, at + 1)
                .await
                .expect("read")
        );
    }
    // Five, one after another, each of another card; the second and the fourth are
    // read as they come.
    let mut made = Vec::new();
    for at in 1..=5_i64 {
        let id = told_of_a_new_card(store, (&board_id, &writer), &named, at * 100, few).await;
        if at == 2 || at == 4 {
            assert!(
                store
                    .read_notification(reader(&named), &id, at * 100 + 1)
                    .await
                    .expect("read")
            );
        }
        made.push(id);
        let held: Vec<String> = told(store, &named)
            .await
            .into_iter()
            .map(|n| n.id)
            .collect();
        let expected: Vec<&String> = match at {
            1 => vec![&made[0]],
            2 => vec![&made[1], &made[0]],
            3 => vec![&made[2], &made[1], &made[0]],
            // Past three: the one that was read goes, older ones that were not stay.
            4 => vec![&made[3], &made[2], &made[0]],
            // The fourth was read, and goes before the first.
            _ => vec![&made[4], &made[2], &made[0]],
        };
        assert_eq!(held.iter().collect::<Vec<_>>(), expected, "after {at}");
    }

    assert_eq!(
        told(store, &other).await.len(),
        2,
        "the other's are as they were"
    );

    // Another's notification is none of theirs to read.
    assert!(
        !store
            .read_notification(reader(&writer), &made[4], 1000)
            .await
            .expect("asked"),
        "the writer has no such notification"
    );
    assert_eq!(
        store
            .unread_notifications_of_user(reader(&named))
            .await
            .expect("counted"),
        3
    );
    // All that were made by 300 are read at 2000; the newest stays unread.
    assert_eq!(
        store
            .read_notifications_of_user(reader(&named), 300, 2000)
            .await
            .expect("read"),
        2
    );
    // Read before a time: the other's two, read long ago; then the two read at 2000.
    let forgotten = async |before: i64| {
        store
            .forget_read_notifications(before)
            .await
            .expect("asked")
    };
    assert_eq!(forgotten(4).await, 2, "the other's, read at 2 and 3");
    assert_eq!(forgotten(2000).await, 0, "read at 2000, and not before it");
    assert_eq!(forgotten(2001).await, 2);
    let left = told(store, &named).await;
    assert_eq!(left.len(), 1, "what was not read is kept however old");
    assert_eq!(
        (left[0].id.as_str(), left[0].read_at),
        (made[4].as_str(), 0)
    );
    test.teardown().await;
}

/// A handle that tells nobody notes nothing; and a card given to somebody, by a
/// person property of its board, is noted as that, once for each person it takes.
#[tokio::test]
async fn a_card_given_is_noted_for_whom_it_takes_and_a_quiet_handle_notes_nothing() {
    let test = common::setup().await;
    let store = &test.store;
    let (_, writer, named) = board_and_two(store).await;
    let other = common::user(store, &format!("o{}", &new_id()[24..])).await;
    let card_properties = [
        json!({"id": "owner", "name": "Owner", "type": "person", "options": []}),
        json!({"id": "helpers", "name": "Helpers", "type": "multiPerson", "options": []}),
        json!({"id": "note", "name": "Note", "type": "text", "options": []}),
    ]
    .into_iter()
    .map(|definition| definition.as_object().expect("an object").clone())
    .collect();
    let mut board = Container {
        id: new_id(),
        workspace_id: common::workspace(store).await,
        container_type: ContainerType::Open,
        created_by: writer.clone(),
        card_properties,
        ..Container::default()
    };
    store
        .insert_container(&mut board, &writer)
        .await
        .expect("the board is stored");
    let board_id = board.id;

    let with = |properties: serde_json::Value| {
        let mut block = card(&board_id, None);
        block.fields.insert("properties".to_owned(), properties);
        block
    };
    // A text that is somebody's identifier gives the card to nobody.
    let mut block = with(json!({"owner": named, "helpers": [other, writer], "note": other}));
    store
        .telling_nobody()
        .insert_block(&mut block.clone(), &writer)
        .await
        .expect("the card is stored quietly");
    assert_eq!(events(store).await, [], "a quiet handle notes nothing");
    block.id = new_id();
    store
        .insert_block(&mut block, &writer)
        .await
        .expect("the card is stored");
    let mut noted: Vec<(String, &str)> = events(store)
        .await
        .into_iter()
        .map(|event| (event.recipient_id, event.kind.as_str()))
        .collect();
    noted.sort();
    let mut expected = vec![
        (named.clone(), NotificationKind::Assignment.as_str()),
        (other.clone(), NotificationKind::Assignment.as_str()),
    ];
    expected.sort();
    assert_eq!(noted, expected, "whom it takes, but for the writer");
    for event in events(store).await {
        assert!(
            store
                .drop_notification_event(&event)
                .await
                .expect("dropped")
        );
    }

    // Written again with the same people, and one fewer: nobody is new.
    let mut same = block.clone();
    same.fields.insert(
        "properties".to_owned(),
        json!({"owner": named, "helpers": [writer]}),
    );
    store
        .insert_block(&mut same, &writer)
        .await
        .expect("the card is written again");
    assert_eq!(events(store).await, [], "nobody it had not taken already");
    test.teardown().await;
}
