//! The record that an account was given its welcome board: made once, with the board,
//! by one of the calls that would make it at once, and not where no board came of it.

use std::sync::Arc;

use fvoci_model::new_id;
use fvoci_store::{CopyChanges, CopyTitles, Store};
use tokio::sync::Barrier;
use tokio::task::JoinSet;

use crate::common;

/// The titles of copies, in English, as a caller of the store brings them.
const TITLES: CopyTitles = CopyTitles {
    copy_of: |title| format!("{title} copy"),
    new_template: "New board template",
};

/// Of the welcome boards asked for an account at once, one is made, and none after;
/// one that cannot be copied records nothing, and the account is given it the next time.
#[tokio::test]
async fn one_welcome_board_is_made_of_those_asked_for_at_once() {
    let test = common::setup().await;
    let workspace_id = common::workspace(&test).await;
    let owner = common::user(&test, "owner").await;
    let alice = common::user(&test, "alice").await;
    let bob = common::user(&test, "bob").await;
    for user_id in [&alice, &bob] {
        common::works_in(&test, &workspace_id, user_id).await;
    }
    let board = common::create_test_boards(&test, &workspace_id, &owner, 1)
        .await
        .remove(0);
    let boards_of = async |user_id: &str| {
        test.texts_unchecked(&format!(
            "SELECT CAST(id AS TEXT) FROM containers WHERE CAST(created_by AS TEXT) = '{user_id}'"
        ))
        .await
        .expect("the boards are read")
    };

    // No such board to copy: nothing is made, and nothing recorded.
    let missing = test
        .duplicate_welcome_board(&new_id(), &alice, &workspace_id, &TITLES, <_>::default(), 6)
        .await;
    assert!(missing.is_err_and(|error| error.is_not_found()));

    let barrier = Arc::new(Barrier::new(8));
    let mut asked = JoinSet::new();
    for _ in 0..8 {
        let (store, barrier) = (test.store.clone(), Arc::clone(&barrier));
        let (board_id, alice, workspace_id) =
            (board.id.clone(), alice.clone(), workspace_id.clone());
        asked.spawn(async move {
            barrier.wait().await;
            store
                .duplicate_welcome_board(
                    &board_id,
                    &alice,
                    &workspace_id,
                    &TITLES,
                    CopyChanges::default(),
                    7,
                )
                .await
                .expect("answered")
                .is_some()
        });
    }
    let made = asked.join_all().await;
    assert_eq!(made.iter().filter(|made| **made).count(), 1, "{made:?}");
    assert_eq!(boards_of(&alice).await.len(), 1, "one board");
    assert!(!test.claim_welcome(&alice, 8).await.unwrap(), "recorded");

    // Each account its own; a record made for a board that was asked for counts.
    assert!(test.claim_welcome(&bob, 8).await.unwrap());
    let again = test
        .duplicate_welcome_board(&board.id, &bob, &workspace_id, &TITLES, <_>::default(), 9)
        .await
        .expect("answered");
    assert_eq!(again, None);
    assert_eq!(boards_of(&bob).await, Vec::<String>::new());

    // Taken back, the account is given its board the next time.
    test.forget_welcome(&bob).await.unwrap();
    let later = test
        .duplicate_welcome_board(&board.id, &bob, &workspace_id, &TITLES, <_>::default(), 9)
        .await
        .expect("answered");
    assert!(later.is_some());
    assert!(
        !test.claim_welcome(&new_id(), 9).await.unwrap(),
        "nobody's account"
    );
    test.teardown().await;
}

/// Migration 0048: the accounts that were there are past their welcome, and one made
/// after is not.
#[tokio::test]
async fn the_migration_records_the_accounts_that_were_there_as_welcomed() {
    let database = common::empty_database().await;
    let connection = common::raw_connection(&database);
    fvoci_store::testing::migrate_up_to(database.db_type(), database.database_connection(), 47)
        .await;
    let before = new_id();
    common::execute_raw(
        &connection,
        &format!(
            "INSERT INTO users (id, username, email, password_hash, create_at, update_at) \
             VALUES ('{before}', 'before', 'before@example.com', '', 1, 1);"
        ),
    )
    .await;
    let store = Store::connect(database.db_type(), database.database_connection())
        .await
        .expect("migrated");
    let after = common::user(&store, "after").await;
    let claimed = (
        store.claim_welcome(&before, 2).await.unwrap(),
        store.claim_welcome(&after, 2).await.unwrap(),
    );
    store.close().await;
    database.remove().await;

    assert_eq!(claimed, (false, true));
}
