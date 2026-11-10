//! The test harness: a fresh database per test, on SQLite or PostgreSQL.

use std::path::PathBuf;

use fvoci_model::{Block, Board, BoardType, IdKind, new_id};
use fvoci_store::{DbType, Store};
use sqlx::Connection as _;
use sqlx::postgres::PgConnection;

/// The table prefix the original test suite runs with.
pub(crate) const TABLE_PREFIX: &str = "test_";

/// A store on a database that belongs to one test.
pub(crate) struct TestStore {
    pub(crate) store: Store,
    cleanup: Cleanup,
}

enum Cleanup {
    SqliteFile(PathBuf),
    PostgresSchema { url: String, schema: String },
}

impl std::ops::Deref for TestStore {
    type Target = Store;

    fn deref(&self) -> &Store {
        &self.store
    }
}

/// Which database the tests run against, from `FVOCI_STORE_TEST_DB_TYPE`.
pub(crate) fn db_type() -> DbType {
    match std::env::var("FVOCI_STORE_TEST_DB_TYPE") {
        Ok(name) if !name.trim().is_empty() => name
            .trim()
            .parse()
            .expect("FVOCI_STORE_TEST_DB_TYPE must be sqlite3 or postgres"),
        _ => DbType::Sqlite,
    }
}

fn postgres_url() -> String {
    std::env::var("FVOCI_STORE_TEST_CONN_STRING")
        .expect("FVOCI_STORE_TEST_CONN_STRING must hold a PostgreSQL URL for postgres test runs")
}

/// Appends a query parameter to a URL that may already have some.
fn with_param(url: &str, param: &str) -> String {
    let separator = if url.contains('?') { '&' } else { '?' };
    format!("{url}{separator}{param}")
}

/// Returns the `dbconfig` and cleanup for a fresh, empty database.
async fn fresh_database() -> (String, Cleanup) {
    let name = new_id(IdKind::None);
    match db_type() {
        DbType::Sqlite => {
            let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("store-tests");
            std::fs::create_dir_all(&dir).expect("the test database directory can be created");
            let path = dir.join(format!("{name}.db"));
            let dbconfig = format!("{}?_busy_timeout=5000", path.display());
            (dbconfig, Cleanup::SqliteFile(path))
        }
        DbType::Postgres => {
            let url = postgres_url();
            let schema = format!("t_{name}");
            let mut admin = PgConnection::connect(&url)
                .await
                .expect("the test PostgreSQL server accepts connections");
            sqlx::raw_sql(sqlx::AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
                .execute(&mut admin)
                .await
                .expect("a test schema can be created");
            let dbconfig = with_param(&url, &format!("search_path={schema}"));
            (dbconfig, Cleanup::PostgresSchema { url, schema })
        }
    }
}

/// Connects a store to a fresh database with the test table prefix.
pub(crate) async fn setup() -> TestStore {
    setup_with_prefix(TABLE_PREFIX).await
}

/// Connects a store to a fresh database with the given table prefix.
pub(crate) async fn setup_with_prefix(table_prefix: &str) -> TestStore {
    let (dbconfig, cleanup) = fresh_database().await;
    let store = Store::connect(db_type(), &dbconfig, table_prefix)
        .await
        .expect("a store connects to a fresh database");
    TestStore { store, cleanup }
}

impl TestStore {
    /// The `dbconfig` that reaches this test's database again.
    pub(crate) fn dbconfig(&self) -> String {
        match &self.cleanup {
            Cleanup::SqliteFile(path) => path.display().to_string(),
            Cleanup::PostgresSchema { url, schema } => {
                with_param(url, &format!("search_path={schema}"))
            }
        }
    }

    /// Closes the store and removes its database.
    pub(crate) async fn teardown(self) {
        self.store.close().await;
        match self.cleanup {
            Cleanup::SqliteFile(path) => {
                // Journal files may or may not exist; a leftover file is harmless.
                let _ = std::fs::remove_file(&path);
            }
            Cleanup::PostgresSchema { url, schema } => {
                let mut admin = PgConnection::connect(&url)
                    .await
                    .expect("the test PostgreSQL server accepts connections");
                sqlx::raw_sql(sqlx::AssertSqlSafe(format!("DROP SCHEMA {schema} CASCADE")))
                    .execute(&mut admin)
                    .await
                    .expect("the test schema can be dropped");
            }
        }
    }
}

/// `storetests.InsertBlocks`: inserts each block on its own.
pub(crate) async fn insert_blocks(store: &Store, blocks: &mut [Block], user_id: &str) {
    for block in blocks {
        store
            .insert_block(block, user_id)
            .await
            .expect("the block is inserted");
    }
}

/// `storetests.DeleteBlocks`: deletes each block on its own.
pub(crate) async fn delete_blocks(store: &Store, blocks: &[Block], modified_by: &str) {
    for block in blocks {
        store
            .delete_block(&block.id, modified_by)
            .await
            .expect("the block is deleted");
    }
}

/// `storetests.ContainsBlockWithID`.
pub(crate) fn contains_block_with_id(blocks: &[Block], block_id: &str) -> bool {
    blocks.iter().any(|block| block.id == block_id)
}

/// The identifiers of the blocks, sorted.
pub(crate) fn sorted_ids(blocks: &[Block]) -> Vec<String> {
    let mut ids: Vec<String> = blocks.iter().map(|block| block.id.clone()).collect();
    ids.sort();
    ids
}

/// `storetests.createTestBoards`: inserts `count` open boards of a team.
pub(crate) async fn create_test_boards(
    store: &Store,
    team_id: &str,
    user_id: &str,
    count: usize,
) -> Vec<Board> {
    let mut boards = Vec::with_capacity(count);
    for index in 0..count {
        let mut board = Board {
            id: new_id(IdKind::Board),
            team_id: team_id.to_owned(),
            board_type: BoardType::Open,
            created_by: user_id.to_owned(),
            title: format!("board {index}"),
            ..Board::default()
        };
        store
            .insert_board(&mut board, user_id)
            .await
            .expect("the board is inserted");
        boards.push(board);
    }
    boards
}
