//! Connecting, schema creation and schema checks. The original has no tests for these
//! (its schema comes from a migration engine); they pin what `Store::connect` promises.

use fvoci_store::{DbType, Store, StoreError};

use crate::common::{self, TABLE_PREFIX};

#[tokio::test]
async fn connect_creates_the_schema_once_and_reopens_it() {
    let test = common::setup().await;
    assert_eq!(test.db_type(), common::db_type());
    let version = test
        .db_version()
        .await
        .expect("the database reports a version");
    assert!(!version.is_empty(), "the version string is not empty");

    // A second store on the same database finds the schema and leaves it alone.
    let again = Store::connect(common::db_type(), &test.dbconfig(), TABLE_PREFIX)
        .await
        .expect("an existing schema at the current version is accepted");
    again.close().await;

    test.teardown().await;
}

#[tokio::test]
async fn connect_rejects_a_table_prefix_that_is_not_an_identifier() {
    let error = Store::connect(DbType::Sqlite, "file::memory:", "x; DROP TABLE y")
        .await
        .expect_err("the prefix is spliced into SQL and must be a plain identifier");
    assert!(matches!(error, StoreError::Config(_)), "got {error:?}");
}

#[tokio::test]
async fn in_memory_sqlite_databases_are_private_to_their_store() {
    let first = Store::connect(
        DbType::Sqlite,
        "file::memory:?cache=shared&_busy_timeout=5000",
        "",
    )
    .await
    .expect("an in-memory database");
    let second = Store::connect(DbType::Sqlite, "file::memory:?cache=shared", "")
        .await
        .expect("another in-memory database");

    let mut block = fvoci_model::Block {
        id: "only-in-first".to_owned(),
        board_id: "board".to_owned(),
        ..fvoci_model::Block::default()
    };
    first
        .insert_block(&mut block, "user")
        .await
        .expect("inserted");

    assert!(
        first.get_block("only-in-first").await.is_ok(),
        "the first store has the block"
    );
    let missing = second
        .get_block("only-in-first")
        .await
        .expect_err("not in the second store");
    assert!(missing.is_not_found(), "got {missing:?}");

    first.close().await;
    second.close().await;
}
