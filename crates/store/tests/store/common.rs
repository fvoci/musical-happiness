//! The test harness: a fresh database per test, on SQLite or PostgreSQL.

use std::path::PathBuf;

use fvoci_model::{Block, Board, BoardType, IdKind, new_id};
use fvoci_store::testing::TestDatabase;
use fvoci_store::{DbType, Store};

/// The table prefix the original test suite runs with.
pub(crate) const TABLE_PREFIX: &str = "test_";

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

/// Connects a store to a fresh database with the test table prefix.
pub(crate) async fn setup() -> TestStore {
    setup_with_prefix(TABLE_PREFIX).await
}

/// Connects a store to a fresh database with the given table prefix.
pub(crate) async fn setup_with_prefix(table_prefix: &str) -> TestStore {
    let directory = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("store-tests");
    let database = TestDatabase::create(&directory).await;
    let store = Store::connect(database.db_type(), database.dbconfig(), table_prefix)
        .await
        .expect("a store connects to a fresh database");
    TestStore { store, database }
}

impl TestStore {
    /// What reaches this test's database again: the path of the SQLite file, or the
    /// PostgreSQL URL with the test's schema.
    pub(crate) fn dbconfig(&self) -> String {
        match self.database.sqlite_path() {
            Some(path) => path.display().to_string(),
            None => self.database.dbconfig().to_owned(),
        }
    }

    /// Closes the store and removes its database.
    pub(crate) async fn teardown(self) {
        self.store.close().await;
        self.database.remove().await;
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
