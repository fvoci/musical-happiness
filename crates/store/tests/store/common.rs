//! The test harness: a fresh database per test, on SQLite or PostgreSQL.

use std::path::PathBuf;

use fvoci_model::{Block, Board, BoardType, Team, User, new_id};
use fvoci_store::testing::TestDatabase;
use fvoci_store::{DbType, Store};
use sqlx::postgres::PgConnection;
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::{ConnectOptions as _, Connection as _};

/// A store on a database that belongs to one test.
pub(crate) struct TestStore {
    pub(crate) store: Store,
    database: TestDatabase,
}

impl std::ops::Deref for TestStore {
    type Target = Store;

    fn deref(&self) -> &Store {
        &self.store
    }
}

/// Which database the tests run against.
pub(crate) fn db_type() -> DbType {
    match std::env::var(fvoci_store::testing::DB_TYPE_ENV) {
        Ok(name) if !name.trim().is_empty() => name
            .trim()
            .parse()
            .expect("the test database type must be sqlite3 or postgres"),
        _ => DbType::Sqlite,
    }
}

/// Connects a store to a fresh database.
pub(crate) async fn setup() -> TestStore {
    let directory = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("store-tests");
    let database = TestDatabase::create(&directory).await;
    let store = Store::connect(database.db_type(), database.dbconfig())
        .await
        .expect("a store connects to a fresh database");
    TestStore { store, database }
}

/// Creates a fresh database without connecting a store to it, for tests of what
/// [`Store::connect`] makes of a database that is not empty.
pub(crate) async fn empty_database() -> TestDatabase {
    let directory = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("store-tests");
    TestDatabase::create(&directory).await
}

/// What reaches a test database through a connection of the test's own: the path of the
/// SQLite file, or the PostgreSQL URL with the test's schema.
pub(crate) fn raw_dbconfig(database: &TestDatabase) -> String {
    match database.sqlite_path() {
        Some(path) => path.display().to_string(),
        None => database.dbconfig().to_owned(),
    }
}

/// Runs statements on a connection of its own, past the store.
pub(crate) async fn execute_raw(dbconfig: &str, sql: &str) {
    let sql = sqlx::AssertSqlSafe(sql.to_owned());
    match db_type() {
        DbType::Sqlite => {
            let options = SqliteConnectOptions::new()
                .filename(dbconfig)
                .create_if_missing(true);
            let mut conn = options
                .connect()
                .await
                .expect("the test database file opens");
            sqlx::raw_sql(sql)
                .execute(&mut conn)
                .await
                .expect("the statement runs");
            conn.close().await.expect("the connection closes");
        }
        DbType::Postgres => {
            let (url, schema) = dbconfig
                .rsplit_once("search_path=")
                .expect("the test dbconfig names its schema");
            let mut conn = PgConnection::connect(url.trim_end_matches(['?', '&']))
                .await
                .expect("the test PostgreSQL server accepts connections");
            sqlx::raw_sql(sqlx::AssertSqlSafe(format!("SET search_path TO {schema}")))
                .execute(&mut conn)
                .await
                .expect("the schema is selected");
            sqlx::raw_sql(sql)
                .execute(&mut conn)
                .await
                .expect("the statement runs");
            conn.close().await.expect("the connection closes");
        }
    }
}

impl TestStore {
    /// What reaches this test's database again: the path of the SQLite file, or the
    /// PostgreSQL URL with the test's schema.
    pub(crate) fn dbconfig(&self) -> String {
        raw_dbconfig(&self.database)
    }

    /// Closes the store and removes its database.
    pub(crate) async fn teardown(self) {
        self.store.close().await;
        self.database.remove().await;
    }
}

/// Stores a user with the given name and returns their identifier. The email address
/// is derived from the name; both must be unique within a test's database.
pub(crate) async fn user(store: &Store, username: &str) -> String {
    let mut user = User {
        id: new_id(),
        username: username.to_owned(),
        email: format!("{username}@example.com"),
        ..User::default()
    };
    store
        .create_user(&mut user)
        .await
        .expect("the user is stored");
    user.id
}

/// Stores a team and returns its identifier.
pub(crate) async fn team(store: &Store) -> String {
    let team = Team {
        id: new_id(),
        signup_token: "signup-token".to_owned(),
        ..Team::default()
    };
    store.create_team(&team).await.expect("the team is stored");
    team.id
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
            id: new_id(),
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
