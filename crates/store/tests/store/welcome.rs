//! The record that an account was given its welcome board: made once, by one of the
//! calls that would make it at once, and taken back where no board came of it.

use std::sync::Arc;

use fvoci_model::new_id;
use fvoci_store::Store;
use tokio::sync::Barrier;
use tokio::task::JoinSet;

use crate::common;

/// Of the claims made for an account at once, one records the welcome; none does
/// after, until it is taken back.
#[tokio::test]
async fn one_claim_of_those_made_at_once_records_the_welcome() {
    let test = common::setup().await;
    let alice = common::user(&test.store, "alice").await;
    let bob = common::user(&test.store, "bob").await;

    let barrier = Arc::new(Barrier::new(8));
    let mut claims = JoinSet::new();
    for _ in 0..8 {
        let (store, alice, barrier) = (test.store.clone(), alice.clone(), Arc::clone(&barrier));
        claims.spawn(async move {
            barrier.wait().await;
            store.claim_welcome(&alice, 7).await.expect("a claim")
        });
    }
    let recorded = claims.join_all().await;
    assert_eq!(
        recorded.iter().filter(|recorded| **recorded).count(),
        1,
        "{recorded:?}"
    );
    assert!(!test.claim_welcome(&alice, 8).await.unwrap(), "given");
    assert!(test.claim_welcome(&bob, 8).await.unwrap(), "each its own");

    test.forget_welcome(&alice).await.unwrap();
    assert!(test.claim_welcome(&alice, 9).await.unwrap(), "taken back");
    assert!(!test.claim_welcome(&bob, 9).await.unwrap(), "not bob's");
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
