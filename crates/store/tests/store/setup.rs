//! Connecting, schema migration and the databases that are refused: what
//! `Store::connect` promises.

use fvoci_store::{DbType, Store, StoreError};

use crate::common;

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
    let again = Store::connect(common::db_type(), &test.dbconfig())
        .await
        .expect("an existing schema at the current version is accepted");
    again.close().await;

    test.teardown().await;
}

/// Connects to a database that holds what `prepare` put there and returns the refusal.
async fn refusal_of_a_database_with(prepare: &str) -> String {
    let database = common::empty_database().await;
    common::execute_raw(&common::raw_dbconfig(&database), prepare).await;

    let error = Store::connect(database.db_type(), database.dbconfig())
        .await
        .expect_err("the database is not this server's to open");
    assert!(matches!(error, StoreError::Schema(_)), "got {error:?}");
    database.remove().await;
    error.to_string()
}

#[tokio::test]
async fn a_database_of_the_original_server_is_refused_with_the_reason() {
    // The bookkeeping table of the original's migration engine: a fixed name on SQLite,
    // the table prefix and `schema_migrations` on PostgreSQL.
    let bookkeeping = match common::db_type() {
        DbType::Sqlite => "db_migrations",
        DbType::Postgres => "boards_schema_migrations",
    };
    let refusal = refusal_of_a_database_with(&format!(
        "CREATE TABLE {bookkeeping} (version BIGINT NOT NULL PRIMARY KEY, name VARCHAR NOT NULL); \
         CREATE TABLE blocks (id VARCHAR(36) PRIMARY KEY);"
    ))
    .await;
    assert!(
        refusal.contains("created by the original server"),
        "the refusal says why: {refusal}"
    );
}

#[tokio::test]
async fn tables_without_a_migration_record_are_refused() {
    let refusal =
        refusal_of_a_database_with("CREATE TABLE blocks (id VARCHAR(36) PRIMARY KEY);").await;
    assert!(
        refusal.contains("no record of schema migrations"),
        "the refusal says why: {refusal}"
    );
}

#[tokio::test]
async fn a_database_migrated_by_a_newer_server_is_refused() {
    let test = common::setup().await;
    test.store.close().await;
    // What a later version of this server leaves behind: a migration this one lacks.
    common::execute_raw(
        &test.dbconfig(),
        "INSERT INTO _sqlx_migrations \
         (version, description, success, checksum, execution_time) \
         SELECT 9999, 'from the future', success, checksum, execution_time \
         FROM _sqlx_migrations WHERE version = 1",
    )
    .await;

    let error = Store::connect(common::db_type(), &test.dbconfig())
        .await
        .expect_err("a database with a migration this server does not know is refused");
    assert!(
        matches!(&error, StoreError::Schema(reason) if reason.contains("newer server")),
        "got {error:?}"
    );

    test.teardown().await;
}

#[tokio::test]
async fn a_changed_migration_history_is_refused() {
    let test = common::setup().await;
    test.store.close().await;
    common::execute_raw(
        &test.dbconfig(),
        "UPDATE _sqlx_migrations SET checksum = checksum || checksum WHERE version = 1",
    )
    .await;

    let error = Store::connect(common::db_type(), &test.dbconfig())
        .await
        .expect_err("a database whose applied migration differs is refused");
    assert!(
        matches!(&error, StoreError::Schema(reason) if reason.contains("differs")),
        "got {error:?}"
    );

    test.teardown().await;
}

#[tokio::test]
async fn the_tables_carry_no_prefix() {
    let test = common::setup().await;
    // A statement that names the tables plainly runs, on either database.
    common::execute_raw(
        &test.dbconfig(),
        "SELECT (SELECT COUNT(*) FROM blocks) + (SELECT COUNT(*) FROM boards) \
         + (SELECT COUNT(*) FROM users) + (SELECT COUNT(*) FROM sessions)",
    )
    .await;
    test.teardown().await;
}

#[tokio::test]
async fn in_memory_sqlite_databases_are_private_to_their_store() {
    let first = Store::connect(
        DbType::Sqlite,
        "file::memory:?cache=shared&_busy_timeout=5000",
    )
    .await
    .expect("an in-memory database");
    let second = Store::connect(DbType::Sqlite, "file::memory:?cache=shared")
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
