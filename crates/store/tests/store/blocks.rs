//! Port of `server/services/store/storetests/blocks.go`.
//!
//! There is one test per test function of the original, because the subtests of one
//! function share a store and run in order. Each subtest is a section headed by its name
//! in quotes. The original sleeps a millisecond between writes so that two history rows of
//! one block cannot collide; the store rules such collisions out, so those sleeps are not
//! ported.
//!
//! After the ported suite come tests the original does not have: concurrent writers, rapid
//! writes to one block, and behavior of the block operations that its suite leaves
//! uncovered.

use std::collections::BTreeSet;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use fvoci_model::{
    BLOCK_FIELDS_MAX_CHARS, BLOCK_TITLE_MAX_CHARS, Block, BlockError, BlockPatch, BlockPatchBatch,
    BlockType, BoardsAndBlocksError, IdKind, JsonMap, new_id,
};
use fvoci_store::{
    BlockFilter, DbType, DuplicateAs, HistoryOrder, HistoryQuery, Paging, Store, StoreError,
    SubtreeQuery,
};
use serde_json::{Value, json};
use sqlx::Connection as _;
use sqlx::sqlite::SqliteConnection;
use tokio::task::JoinSet;

use crate::common;

/// `testUserID` of the original suite.
const TEST_USER_ID: &str = "user-id";
const TEST_TEAM_ID: &str = "team-id";
/// `testBoardID` of the original suite.
const TEST_BOARD_ID: &str = "board-id";

/// A block with the given identifier and board and nothing else.
///
/// Many block literals of the original also preset `ModifiedBy`. Every insert overwrites
/// it before anything reads it, so it is only carried over where a test is about it.
fn block(id: &str, board_id: &str) -> Block {
    Block {
        id: id.to_owned(),
        board_id: board_id.to_owned(),
        ..Block::default()
    }
}

/// A block of the test board with a parent and a type.
fn typed_block(id: &str, parent_id: &str, block_type: &str) -> Block {
    Block {
        parent_id: parent_id.to_owned(),
        block_type: BlockType::from(block_type),
        ..block(id, TEST_BOARD_ID)
    }
}

/// `subtreeSampleBlocks` (`server/services/store/storetests/blocks.go:529`).
fn subtree_sample_blocks() -> Vec<Block> {
    [
        ("parent", ""),
        ("child1", "parent"),
        ("child2", "parent"),
        ("grandchild1", "child1"),
        ("grandchild2", "child2"),
        ("greatgrandchild1", "grandchild1"),
    ]
    .into_iter()
    .map(|(id, parent_id)| typed_block(id, parent_id, ""))
    .collect()
}

/// The five blocks that `testGetBlocks` and `testGetBlockMetadata` insert.
fn typed_sample_blocks() -> Vec<Block> {
    [
        ("block1", "", "test"),
        ("block2", "block1", "test"),
        ("block3", "block1", "test"),
        ("block4", "block1", "test2"),
        ("block5", "block2", "test"),
    ]
    .into_iter()
    .map(|(id, parent_id, block_type)| typed_block(id, parent_id, block_type))
    .collect()
}

/// The three blocks that `testDeleteBlock` and `testUndeleteBlock` insert.
fn three_blocks() -> Vec<Block> {
    ["block1", "block2", "block3"]
        .into_iter()
        .map(|id| block(id, TEST_BOARD_ID))
        .collect()
}

/// Four fields that together exceed the size limit of a block's fields.
fn oversized_fields() -> JsonMap {
    [("one", "1"), ("two", "2"), ("three", "3"), ("four", "4")]
        .into_iter()
        .map(|(key, digit)| {
            let value = Value::String(digit.repeat(BLOCK_FIELDS_MAX_CHARS / 4));
            (key.to_owned(), value)
        })
        .collect()
}

/// The fields of a block, from a JSON object literal.
fn fields(object: Value) -> JsonMap {
    serde_json::from_value(object).expect("the literal is a JSON object")
}

/// A patch that sets the title.
fn title_patch(title: &str) -> BlockPatch {
    BlockPatch {
        title: Some(title.to_owned()),
        ..BlockPatch::default()
    }
}

fn ids<const N: usize>(ids: [&str; N]) -> Vec<String> {
    ids.into_iter().map(str::to_owned).collect()
}

fn now_millis() -> i64 {
    let since_epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("the clock is past 1970");
    i64::try_from(since_epoch.as_millis()).expect("the time fits 64 bits")
}

/// `assert.WithinDurationf(expected, actual, 1*time.Second, ...)`.
#[track_caller]
fn assert_within_a_second(expected: i64, actual: i64, what: &str) {
    assert!(
        (actual - expected).abs() <= 1000,
        "{what} should be the current time: expected about {expected}, got {actual}"
    );
}

/// `require.ErrorAs(t, err, &nf)` with `nf` a `*model.ErrNotFound`.
#[track_caller]
fn assert_not_found(error: &StoreError) {
    assert!(
        matches!(error, StoreError::NotFound(_)),
        "expected a not-found error, got {error:?}"
    );
}

/// `require.ErrorIs(t, err, <a block validation error>)`.
#[track_caller]
fn assert_invalid_block(error: &StoreError, expected: BlockError) {
    assert!(
        matches!(error, StoreError::InvalidBlock { source, .. } if *source == expected),
        "expected the validation error {expected:?}, got {error:?}"
    );
}

/// `GetBlocksForBoard` followed by `require.NoError`.
async fn board_blocks(store: &Store, board_id: &str) -> Vec<Block> {
    store
        .get_blocks(&BlockFilter::board(board_id))
        .await
        .expect("the blocks of a board are read")
}

/// `GetBlock` followed by `require.NoError` and `require.NotNil`.
async fn existing_block(store: &Store, block_id: &str) -> Block {
    store
        .get_block(block_id)
        .await
        .unwrap_or_else(|error| panic!("block {block_id} should exist: {error:?}"))
}

/// `GetBlock` followed by `require.ErrorAs(t, err, &nf)` and `require.Nil(t, block)`.
async fn assert_no_block(store: &Store, block_id: &str) {
    match store.get_block(block_id).await {
        Ok(block) => panic!("block {block_id} should not exist, got {block:?}"),
        Err(error) => assert_not_found(&error),
    }
}

/// `GetBlockHistoryDescendants` followed by `require.NoError`.
async fn board_history(store: &Store, board_id: &str, query: &HistoryQuery) -> Vec<Block> {
    store
        .get_block_history_descendants(board_id, query)
        .await
        .expect("the history of a board is read")
}

/// `createTestCards` (`server/services/store/storetests/util.go:76`).
async fn create_test_cards(
    store: &Store,
    user_id: &str,
    board_id: &str,
    count: usize,
) -> Vec<Block> {
    let mut cards = Vec::with_capacity(count);
    for index in 0..count {
        let mut card = Block {
            parent_id: board_id.to_owned(),
            block_type: BlockType::Card,
            created_by: user_id.to_owned(),
            title: format!("card {index}"),
            ..block(&new_id(IdKind::Card), board_id)
        };
        store
            .insert_block(&mut card, user_id)
            .await
            .expect("a test card is inserted");
        cards.push(card);
    }
    cards
}

/// `createTestBlocksForCard` (`server/services/store/storetests/util.go:52`).
async fn create_test_blocks_for_card(store: &Store, card_id: &str, count: usize) -> Vec<Block> {
    let card = existing_block(store, card_id).await;
    assert_eq!(
        card.block_type,
        BlockType::Card,
        "content blocks are created for a card"
    );

    let mut blocks = Vec::with_capacity(count);
    for index in 0..count {
        let mut text = Block {
            parent_id: card.id.clone(),
            block_type: BlockType::Text,
            created_by: card.created_by.clone(),
            title: format!("text {index}"),
            ..block(&new_id(IdKind::Block), &card.board_id)
        };
        store
            .insert_block(&mut text, &card.created_by)
            .await
            .expect("a test content block is inserted");
        blocks.push(text);
    }
    blocks
}

/// A second connection to the SQLite database of a test, for what the store has no
/// operation for.
async fn sqlite_connection(test: &common::TestStore) -> SqliteConnection {
    SqliteConnection::connect(&format!("sqlite://{}", test.dbconfig()))
        .await
        .expect("the test database opens a second time")
}

/// Writes a file-info row that is not deleted for each identifier.
async fn insert_file_infos<const N: usize>(raw: &mut SqliteConnection, ids: [&str; N]) {
    for id in ids {
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "INSERT INTO {}file_info (id, create_at, delete_at, name, extension, size) \
             VALUES ($1, 1, 0, 'file', 'png', 1)",
            common::TABLE_PREFIX
        )))
        .bind(id)
        .execute(&mut *raw)
        .await
        .expect("a file-info row is written");
    }
}

/// The identifiers of the file-info rows that are marked deleted, sorted.
async fn deleted_file_infos(raw: &mut SqliteConnection) -> Vec<String> {
    sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT id FROM {}file_info WHERE delete_at != 0 ORDER BY id",
        common::TABLE_PREFIX
    )))
    .fetch_all(raw)
    .await
    .expect("the file-info rows are read")
}

/// `testInsertBlock`.
#[tokio::test]
#[expect(
    clippy::too_many_lines,
    reason = "one test per test function of the original, whose subtests share a store"
)]
async fn insert_block() {
    let test = common::setup().await;
    let store = &test.store;
    let board_id = TEST_BOARD_ID;

    let initial_count = board_blocks(store, board_id).await.len();

    // "valid block"
    let mut valid = block("id-test", board_id);
    store
        .insert_block(&mut valid, "user-id-1")
        .await
        .expect("a valid block is inserted");
    let blocks = board_blocks(store, board_id).await;
    assert_eq!(
        blocks.len(),
        initial_count + 1,
        "the board has one more block: {blocks:?}"
    );

    // "invalid rootid"
    let mut without_board = block("id-test", "");
    let error = store
        .insert_block(&mut without_board, "user-id-1")
        .await
        .expect_err("a block without a board is rejected");
    assert_invalid_block(&error, BlockError::EmptyBoardId);
    let blocks = board_blocks(store, board_id).await;
    assert_eq!(
        blocks.len(),
        initial_count + 1,
        "the rejected block changed nothing: {blocks:?}"
    );

    // "invalid fields data"
    // Not expressible: the original stores a function value in the fields, which cannot
    // be serialized to JSON. The fields of a block here are a JSON object by type, so
    // there is no value to pass that fails to serialize.

    // "block with title too large"
    let mut long_title = Block {
        title: "A".repeat(BLOCK_TITLE_MAX_CHARS + 1),
        ..block("id-test", board_id)
    };
    let error = store
        .insert_block(&mut long_title, "user-id-1")
        .await
        .expect_err("a title over the limit is rejected");
    assert_invalid_block(&error, BlockError::TitleSizeLimitExceeded);

    // "block with aggregated fields size too large"
    let mut large_fields = Block {
        fields: oversized_fields(),
        ..block("id-test", board_id)
    };
    let error = store
        .insert_block(&mut large_fields, "user-id-2")
        .await
        .expect_err("fields over the limit are rejected");
    assert_invalid_block(&error, BlockError::FieldsSizeLimitExceeded);

    // "insert new block"
    let mut new_block = block("", TEST_BOARD_ID);
    store
        .insert_block(&mut new_block, "user-id-2")
        .await
        .expect("a block without an identifier is inserted");
    assert_eq!(
        new_block.created_by, "user-id-2",
        "the creator of a new block is the inserting user"
    );

    // "update existing block"
    let mut old = Block {
        title: "Old Title".to_owned(),
        ..block("id-2", "board-id-1")
    };
    store
        .insert_block(&mut old, "user-id-2")
        .await
        .expect("the block is inserted");
    assert_eq!(
        old.created_by, "user-id-2",
        "the creator of a new block is the inserting user"
    );

    let mut new = Block {
        created_by: "user-id-3".to_owned(),
        title: "New Title".to_owned(),
        ..block("id-2", "board-id-1")
    };
    store
        .insert_block(&mut new, "user-id-4")
        .await
        .expect("the block is updated");
    assert_eq!(
        new.created_by, "user-id-3",
        "the creator given for an existing block is not altered"
    );
    assert_eq!(new.title, "New Title", "the title is as given");

    // "update existing block with title too large"
    let mut existing = Block {
        created_by: "user-id-3".to_owned(),
        title: "New Title".to_owned(),
        ..block("id-3", "board-id-1")
    };
    store
        .insert_block(&mut existing, "user-id-3")
        .await
        .expect("the block is inserted");
    assert_eq!(
        existing.created_by, "user-id-3",
        "the creator of a new block is the inserting user"
    );

    let mut long_title = Block {
        created_by: "user-id-3".to_owned(),
        title: "A".repeat(BLOCK_TITLE_MAX_CHARS + 1),
        ..block("id-3", "board-id-1")
    };
    let error = store
        .insert_block(&mut long_title, "user-id-3")
        .await
        .expect_err("an update with a title over the limit is rejected");
    assert_invalid_block(&error, BlockError::TitleSizeLimitExceeded);

    // "update existing block with aggregated fields size too large"
    let mut existing = Block {
        created_by: "user-id-3".to_owned(),
        title: "New Title".to_owned(),
        ..block("id-3", "board-id-1")
    };
    store
        .insert_block(&mut existing, "user-id-3")
        .await
        .expect("the block is written again");
    assert_eq!(
        existing.created_by, "user-id-3",
        "the creator given for an existing block is not altered"
    );

    let mut large_fields = Block {
        created_by: "user-id-3".to_owned(),
        fields: oversized_fields(),
        ..block("id-3", "board-id-1")
    };
    let error = store
        .insert_block(&mut large_fields, "user-id-3")
        .await
        .expect_err("an update with fields over the limit is rejected");
    assert_invalid_block(&error, BlockError::FieldsSizeLimitExceeded);

    // "data tamper attempt"
    // `01 Jan 90 01:00` and `02 Jan 90 01:00`; the original parses them with a zone
    // abbreviation that its parser reads as UTC.
    let created_at = 631_155_600_000;
    let updated_at = 631_242_000_000;
    let mut tampered = Block {
        title: "Old Title".to_owned(),
        create_at: created_at,
        update_at: updated_at,
        created_by: "user-id-5".to_owned(),
        modified_by: "user-id-6".to_owned(),
        ..block("id-10", "board-id-1")
    };
    store
        .insert_block(&mut tampered, "user-id-1")
        .await
        .expect("the block is inserted");
    let expected_time = now_millis();

    let retrieved = existing_block(store, "id-10").await;
    assert_eq!(retrieved.board_id, "board-id-1", "the board is as given");
    assert_eq!(
        retrieved.created_by, "user-id-1",
        "the creator is the inserting user, not the one given"
    );
    assert_eq!(
        retrieved.modified_by, "user-id-1",
        "the modifier is the inserting user, not the one given"
    );
    assert_within_a_second(expected_time, retrieved.create_at, "create time");
    assert_within_a_second(expected_time, retrieved.update_at, "update time");

    test.teardown().await;
}

/// `testInsertBlocks`.
#[tokio::test]
async fn insert_blocks() {
    let test = common::setup().await;
    let store = &test.store;

    let initial_count = board_blocks(store, "id-test").await.len();

    // "invalid block"
    let mut new_blocks = [block("id-test", "id-test"), block("id-test", "")];
    let error = store
        .insert_blocks(&mut new_blocks, "user-id-1")
        .await
        .expect_err("a batch with an invalid block is rejected");
    assert_invalid_block(&error, BlockError::EmptyBoardId);

    let blocks = board_blocks(store, "id-test").await;
    assert_eq!(
        blocks.len(),
        initial_count,
        "no blocks should have been inserted: {blocks:?}"
    );

    test.teardown().await;
}

/// `testPatchBlock`.
#[tokio::test]
async fn patch_block() {
    let test = common::setup().await;
    let store = &test.store;
    let board_id = "board-id-1";

    let mut block = Block {
        title: "oldTitle".to_owned(),
        fields: fields(json!({ "test": "test value", "test2": "test value 2" })),
        ..block("id-test", board_id)
    };
    store
        .insert_block(&mut block, "user-id-1")
        .await
        .expect("the block is inserted");

    let initial_count = board_blocks(store, board_id).await.len();

    // "not existing block id"
    let error = store
        .patch_block("invalid-block-id", &BlockPatch::default(), "user-id-1")
        .await
        .expect_err("a missing block cannot be patched");
    assert_not_found(&error);
    assert!(
        error.is_not_found(),
        "the error counts as not found: {error:?}"
    );
    let blocks = board_blocks(store, board_id).await;
    assert_eq!(
        blocks.len(),
        initial_count,
        "the failed patch changed nothing: {blocks:?}"
    );

    // "invalid fields data"
    // Not expressible: the original puts a function value into the updated fields, which
    // cannot be serialized to JSON. The updated fields of a patch here are a JSON object
    // by type.

    // "update block fields"
    store
        .patch_block("id-test", &title_patch("New title"), "user-id-2")
        .await
        .expect("the title is patched");
    let retrieved = existing_block(store, "id-test").await;
    assert_eq!(
        retrieved.modified_by, "user-id-2",
        "the modifier is the patching user"
    );
    assert_eq!(retrieved.title, "New title", "the title is the patched one");

    // "update block custom fields"
    let patch = BlockPatch {
        updated_fields: fields(json!({ "test": "new test value", "test3": "new value" })),
        ..BlockPatch::default()
    };
    store
        .patch_block("id-test", &patch, "user-id-2")
        .await
        .expect("the fields are patched");
    let retrieved = existing_block(store, "id-test").await;
    assert_eq!(
        retrieved.modified_by, "user-id-2",
        "the modifier is the patching user"
    );
    assert_eq!(
        retrieved.fields.get("test"),
        Some(&json!("new test value")),
        "an existing field is replaced"
    );
    assert_eq!(
        retrieved.fields.get("test2"),
        Some(&json!("test value 2")),
        "a field the patch does not name is kept"
    );
    assert_eq!(
        retrieved.fields.get("test3"),
        Some(&json!("new value")),
        "a new field is added"
    );

    // "remove block custom fields"
    let patch = BlockPatch {
        deleted_fields: ids(["test", "test3", "test100"]),
        ..BlockPatch::default()
    };
    store
        .patch_block("id-test", &patch, "user-id-2")
        .await
        .expect("the fields are removed");
    let retrieved = existing_block(store, "id-test").await;
    assert_eq!(
        retrieved.modified_by, "user-id-2",
        "the modifier is the patching user"
    );
    assert_eq!(
        retrieved.fields.get("test"),
        None,
        "a deleted field is gone"
    );
    assert_eq!(
        retrieved.fields.get("test2"),
        Some(&json!("test value 2")),
        "a field the patch does not name is kept"
    );
    assert_eq!(
        retrieved.fields.get("test3"),
        None,
        "a deleted field is gone"
    );

    test.teardown().await;
}

/// `testPatchBlocks`.
#[tokio::test]
async fn patch_blocks() {
    let test = common::setup().await;
    let store = &test.store;

    let mut insert_blocks = [
        Block {
            title: "oldTitle".to_owned(),
            ..block("id-test", "id-test")
        },
        Block {
            title: "oldTitle2".to_owned(),
            ..block("id-test2", "id-test2")
        },
    ];
    store
        .insert_blocks(&mut insert_blocks, "user-id-1")
        .await
        .expect("the blocks are inserted");

    // "successful updated existing blocks"
    let title = "updatedTitle";
    let batch = BlockPatchBatch {
        block_ids: ids(["id-test", "id-test2"]),
        block_patches: vec![title_patch(title), title_patch(title)],
    };
    store
        .patch_blocks(&batch, "user-id-1")
        .await
        .expect("both blocks are patched");
    let retrieved = existing_block(store, "id-test").await;
    assert_eq!(retrieved.title, title, "the first block has the new title");
    let retrieved2 = existing_block(store, "id-test2").await;
    assert_eq!(
        retrieved2.title, title,
        "the second block has the new title"
    );

    // "invalid block id, nothing updated existing blocks"
    // The original skips this on SQLite, where it has no transactions. This store has
    // them on both databases, so it runs on both.
    let title = "Another Title";
    let batch = BlockPatchBatch {
        block_ids: ids(["id-test", "invalid id"]),
        block_patches: vec![title_patch(title), title_patch(title)],
    };
    let error = store
        .patch_blocks(&batch, "user-id-1")
        .await
        .expect_err("a batch that names a missing block fails");
    assert_not_found(&error);
    let retrieved = existing_block(store, "id-test").await;
    assert_ne!(
        retrieved.title, title,
        "the patch of the first block was rolled back"
    );

    test.teardown().await;
}

/// `testGetSubTree2`.
#[tokio::test]
async fn get_sub_tree2() {
    let test = common::setup().await;
    let store = &test.store;
    let board_id = TEST_BOARD_ID;

    let initial_count = board_blocks(store, board_id).await.len();

    let mut sample = subtree_sample_blocks();
    common::insert_blocks(store, &mut sample, "user-id-1").await;

    let blocks = board_blocks(store, board_id).await;
    assert_eq!(
        blocks.len(),
        initial_count + 6,
        "the six sample blocks were added: {blocks:?}"
    );

    // "from root id"
    let blocks = store
        .get_subtree(board_id, "parent", &SubtreeQuery::default())
        .await
        .expect("the subtree of the root is read");
    assert_eq!(blocks.len(), 3, "the root and its two children: {blocks:?}");
    for id in ["parent", "child1", "child2"] {
        assert!(
            common::contains_block_with_id(&blocks, id),
            "the subtree of the root contains {id}: {blocks:?}"
        );
    }

    // "from child id"
    let blocks = store
        .get_subtree(board_id, "child1", &SubtreeQuery::default())
        .await
        .expect("the subtree of a child is read");
    assert_eq!(blocks.len(), 2, "the child and its one child: {blocks:?}");
    for id in ["child1", "grandchild1"] {
        assert!(
            common::contains_block_with_id(&blocks, id),
            "the subtree of the child contains {id}: {blocks:?}"
        );
    }

    // "from not existing id"
    let blocks = store
        .get_subtree(board_id, "not-exists", &SubtreeQuery::default())
        .await
        .expect("the subtree of a missing block is read");
    assert!(
        blocks.is_empty(),
        "a missing block has no subtree: {blocks:?}"
    );

    common::delete_blocks(store, &sample, "test").await;
    test.teardown().await;
}

/// `testDeleteBlock`.
#[tokio::test]
async fn delete_block() {
    let test = common::setup().await;
    let store = &test.store;
    let user_id = TEST_USER_ID;
    let board_id = TEST_BOARD_ID;

    let initial_count = board_blocks(store, board_id).await.len();

    let mut blocks_to_insert = three_blocks();
    common::insert_blocks(store, &mut blocks_to_insert, "user-id-1").await;

    let blocks = board_blocks(store, board_id).await;
    assert_eq!(
        blocks.len(),
        initial_count + 3,
        "the three blocks were added: {blocks:?}"
    );

    // "existing id"
    store
        .delete_block("block1", user_id)
        .await
        .expect("an existing block is deleted");

    // "existing id multiple times"
    store
        .delete_block("block1", user_id)
        .await
        .expect("deleting a deleted block succeeds");
    store
        .delete_block("block1", user_id)
        .await
        .expect("deleting a deleted block succeeds again");

    // "from not existing id"
    store
        .delete_block("not-exists", user_id)
        .await
        .expect("deleting a block that never existed succeeds");

    common::delete_blocks(store, &blocks_to_insert, "test").await;
    test.teardown().await;
}

/// `testUndeleteBlock`.
#[tokio::test]
async fn undelete_block() {
    let test = common::setup().await;
    let store = &test.store;
    let board_id = TEST_BOARD_ID;
    let user_id = TEST_USER_ID;

    let initial_count = board_blocks(store, board_id).await.len();

    let mut blocks_to_insert = three_blocks();
    common::insert_blocks(store, &mut blocks_to_insert, "user-id-1").await;

    let blocks = board_blocks(store, board_id).await;
    assert_eq!(
        blocks.len(),
        initial_count + 3,
        "the three blocks were added: {blocks:?}"
    );

    // "existing id"
    store
        .delete_block("block1", user_id)
        .await
        .expect("the block is deleted");
    assert_no_block(store, "block1").await;

    store
        .undelete_block("block1", user_id)
        .await
        .expect("the block is undeleted");
    existing_block(store, "block1").await;

    // "existing id multiple times"
    store
        .delete_block("block1", user_id)
        .await
        .expect("the block is deleted a second time");
    assert_no_block(store, "block1").await;

    store
        .undelete_block("block1", user_id)
        .await
        .expect("the block is undeleted a second time");
    existing_block(store, "block1").await;

    store
        .undelete_block("block1", user_id)
        .await
        .expect("undeleting a block that is not deleted succeeds");
    existing_block(store, "block1").await;

    // "from not existing id"
    store
        .undelete_block("not-exists", user_id)
        .await
        .expect("undeleting a block that never existed succeeds");
    assert_no_block(store, "not-exists").await;

    common::delete_blocks(store, &blocks_to_insert, "test").await;
    test.teardown().await;
}

/// `testGetBlocks`.
#[tokio::test]
async fn get_blocks() {
    let test = common::setup().await;
    let store = &test.store;
    let board_id = TEST_BOARD_ID;
    let board = || BlockFilter::board(board_id);

    let mut blocks_to_insert = typed_sample_blocks();
    common::insert_blocks(store, &mut blocks_to_insert, "user-id-1").await;

    // "not existing parent"
    let filter = board()
        .with_parent("not-exists")
        .with_type(BlockType::from("test"));
    let blocks = store.get_blocks(&filter).await.expect("the query runs");
    assert!(
        blocks.is_empty(),
        "no block has the missing parent: {blocks:?}"
    );

    // "not existing type"
    let filter = board()
        .with_parent("block1")
        .with_type(BlockType::from("not-existing"));
    let blocks = store.get_blocks(&filter).await.expect("the query runs");
    assert!(
        blocks.is_empty(),
        "no block has the missing type: {blocks:?}"
    );

    // "valid parent and type"
    let filter = board()
        .with_parent("block1")
        .with_type(BlockType::from("test"));
    let blocks = store.get_blocks(&filter).await.expect("the query runs");
    assert_eq!(
        blocks.len(),
        2,
        "two children of block1 have the type: {blocks:?}"
    );

    // "not existing parent"
    let filter = board().with_parent("not-exists");
    let blocks = store.get_blocks(&filter).await.expect("the query runs");
    assert!(
        blocks.is_empty(),
        "no block has the missing parent: {blocks:?}"
    );

    // "valid parent"
    let filter = board().with_parent("block1");
    let blocks = store.get_blocks(&filter).await.expect("the query runs");
    assert_eq!(blocks.len(), 3, "block1 has three children: {blocks:?}");

    // "not existing type"
    let filter = board().with_type(BlockType::from("not-exists"));
    let blocks = store.get_blocks(&filter).await.expect("the query runs");
    assert!(
        blocks.is_empty(),
        "no block has the missing type: {blocks:?}"
    );

    // "valid type"
    let filter = board().with_type(BlockType::from("test"));
    let blocks = store.get_blocks(&filter).await.expect("the query runs");
    assert_eq!(blocks.len(), 4, "four blocks have the type: {blocks:?}");

    // "not existing board"
    let blocks = board_blocks(store, "not-exists").await;
    assert!(
        blocks.is_empty(),
        "a missing board has no blocks: {blocks:?}"
    );

    // "all blocks of the a board"
    let blocks = board_blocks(store, board_id).await;
    assert_eq!(blocks.len(), 5, "the board has all five blocks: {blocks:?}");

    // "several blocks by ids"
    let blocks = store
        .get_blocks_by_ids(&ids(["block2", "block4"]))
        .await
        .expect("both blocks exist");
    assert_eq!(blocks.len(), 2, "both blocks are returned: {blocks:?}");

    // "blocks by ids where some are not found"
    // The original also hands back the one block it found next to the error
    // (`require.Len(t, blocks, 1)`). A `Result` carries the error alone; neither caller
    // of the original (`server/app/blocks.go:106`, `server/app/boards_and_blocks.go:59`)
    // looks at the blocks when there is an error.
    let error = store
        .get_blocks_by_ids(&ids(["block2", "blockNonexistent"]))
        .await
        .expect_err("one of the two blocks does not exist");
    assert!(
        matches!(error, StoreError::NotAllFound { .. }),
        "expected a not-all-found error, got {error:?}"
    );
    assert!(
        error.is_not_found(),
        "the error counts as not found: {error:?}"
    );

    // "blocks by ids where none are found"
    let error = store
        .get_blocks_by_ids(&ids(["blockNonexistent1", "blockNonexistent2"]))
        .await
        .expect_err("neither block exists");
    assert!(
        matches!(error, StoreError::NotAllFound { .. }),
        "expected a not-all-found error, got {error:?}"
    );
    assert!(
        error.is_not_found(),
        "the error counts as not found: {error:?}"
    );

    common::delete_blocks(store, &blocks_to_insert, "test").await;
    test.teardown().await;
}

/// `testGetBlock`.
#[tokio::test]
async fn get_block() {
    let test = common::setup().await;
    let store = &test.store;

    // "get a block"
    let mut block = Block {
        modified_by: "user-id-1".to_owned(),
        ..block("block-id-10", "board-id-1")
    };
    store
        .insert_block(&mut block, "user-id-1")
        .await
        .expect("the block is inserted");

    let fetched = existing_block(store, "block-id-10").await;
    assert_eq!(fetched.id, "block-id-10", "the identifier is as inserted");
    assert_eq!(fetched.board_id, "board-id-1", "the board is as inserted");
    assert_eq!(
        fetched.created_by, "user-id-1",
        "the creator is the inserting user"
    );
    assert_eq!(
        fetched.modified_by, "user-id-1",
        "the modifier is the inserting user"
    );
    assert_within_a_second(now_millis(), fetched.create_at, "create time");
    assert_within_a_second(now_millis(), fetched.update_at, "update time");

    // "get a non-existing block"
    assert_no_block(store, "non-existing-id").await;

    test.teardown().await;
}

/// `testDuplicateBlock`.
#[tokio::test]
async fn duplicate_block() {
    let test = common::setup().await;
    let store = &test.store;

    let sample = subtree_sample_blocks();
    let mut blocks_to_insert = sample.clone();
    blocks_to_insert.push(typed_block("grandchild1a", "child1", "comment"));
    blocks_to_insert.push(typed_block("grandchild2a", "child2", "comment"));
    common::insert_blocks(store, &mut blocks_to_insert, "user-id-1").await;

    // "duplicate existing block as no template"
    let blocks = store
        .duplicate_block(TEST_BOARD_ID, "child1", TEST_USER_ID, DuplicateAs::Copy)
        .await
        .expect("an existing block is duplicated");
    assert_eq!(
        blocks.len(),
        2,
        "the block and its child that is not a comment: {blocks:?}"
    );
    assert_eq!(
        blocks[0].fields.get("isTemplate"),
        Some(&json!(false)),
        "a copy is not a template"
    );

    // "duplicate existing block as template"
    let blocks = store
        .duplicate_block(TEST_BOARD_ID, "child1", TEST_USER_ID, DuplicateAs::Template)
        .await
        .expect("an existing block is duplicated as a template");
    assert_eq!(
        blocks.len(),
        2,
        "the block and its child that is not a comment: {blocks:?}"
    );
    assert_eq!(
        blocks[0].fields.get("isTemplate"),
        Some(&json!(true)),
        "a template copy is a template"
    );

    // "duplicate not existing block"
    let error = store
        .duplicate_block(
            TEST_BOARD_ID,
            "not-existing-id",
            TEST_USER_ID,
            DuplicateAs::Copy,
        )
        .await
        .expect_err("a missing block cannot be duplicated");
    assert_not_found(&error);

    // "duplicate not existing board"
    let error = store
        .duplicate_block(
            "not-existing-board",
            "not-existing-id",
            TEST_USER_ID,
            DuplicateAs::Copy,
        )
        .await
        .expect_err("a block of a missing board cannot be duplicated");
    assert_not_found(&error);

    // "not matching board/block"
    let error = store
        .duplicate_block("other-id", "child1", TEST_USER_ID, DuplicateAs::Copy)
        .await
        .expect_err("a block cannot be duplicated through another board");
    assert_not_found(&error);

    // Like the original, this deletes the six sample blocks and leaves the comments.
    common::delete_blocks(store, &sample, "test").await;
    test.teardown().await;
}

/// `testGetBlockMetadata`. Despite the name it tests the history of a board's blocks,
/// and needs nothing but blocks.
#[tokio::test]
#[expect(
    clippy::too_many_lines,
    reason = "one test per test function of the original, whose subtests share a store"
)]
async fn get_block_metadata() {
    let test = common::setup().await;
    let store = &test.store;
    let board_id = TEST_BOARD_ID;
    let ascending = HistoryQuery::default();
    let descending = HistoryQuery {
        order: HistoryOrder::Descending,
        ..HistoryQuery::default()
    };

    let mut blocks_to_insert = typed_sample_blocks();
    for block in &mut blocks_to_insert {
        // The original sleeps 20 ms here. Unlike its 1 ms sleeps this one matters to the
        // test: the subtests on `update_at` bounds below need every block to have a later
        // modification time than the one before, and that time is the wall clock in
        // milliseconds.
        tokio::time::sleep(Duration::from_millis(2)).await;
        common::insert_blocks(store, std::slice::from_mut(block), TEST_USER_ID).await;
    }
    let first_id = blocks_to_insert[0].id.as_str();
    let last_id = blocks_to_insert[blocks_to_insert.len() - 1].id.as_str();

    // "get full block history"
    let blocks = board_history(store, board_id, &ascending).await;
    assert_eq!(blocks.len(), 5, "one record per insert: {blocks:?}");
    assert_eq!(blocks[0].id, first_id, "the oldest record comes first");

    // "get full block history descending"
    let blocks = board_history(store, board_id, &descending).await;
    assert_eq!(blocks.len(), 5, "one record per insert: {blocks:?}");
    assert_eq!(blocks[0].id, last_id, "the newest record comes first");

    // "get limited block history"
    let query = HistoryQuery {
        limit: Some(3),
        ..ascending
    };
    let blocks = board_history(store, board_id, &query).await;
    assert_eq!(blocks.len(), 3, "the limit is applied: {blocks:?}");

    // "get first block history"
    let query = HistoryQuery {
        limit: Some(1),
        ..ascending
    };
    let blocks = board_history(store, board_id, &query).await;
    assert_eq!(blocks.len(), 1, "the limit is applied: {blocks:?}");
    assert_eq!(blocks[0].id, first_id, "the oldest record is the first");

    // "get last block history"
    let query = HistoryQuery {
        limit: Some(1),
        ..descending
    };
    let blocks = board_history(store, board_id, &query).await;
    assert_eq!(blocks.len(), 1, "the limit is applied: {blocks:?}");
    assert_eq!(blocks[0].id, last_id, "the newest record is the first");

    // "get block history after updateAt"
    let third = existing_block(store, "block3").await;
    assert_ne!(third.update_at, 0, "the block has a modification time");
    let query = HistoryQuery {
        after_update_at: Some(third.update_at),
        ..ascending
    };
    let blocks = board_history(store, board_id, &query).await;
    assert_eq!(
        blocks.len(),
        2,
        "two blocks were written after block3: {blocks:?}"
    );
    assert_eq!(
        blocks[0].id, blocks_to_insert[3].id,
        "the older of the two comes first"
    );

    // "get block history before updateAt"
    let third = existing_block(store, "block3").await;
    assert_ne!(third.update_at, 0, "the block has a modification time");
    let query = HistoryQuery {
        before_update_at: Some(third.update_at),
        ..descending
    };
    let blocks = board_history(store, board_id, &query).await;
    assert_eq!(
        blocks.len(),
        2,
        "two blocks were written before block3: {blocks:?}"
    );
    assert_eq!(
        blocks[0].id, blocks_to_insert[1].id,
        "the newer of the two comes first"
    );

    // "get full block history after delete"
    // This deletes `block1` and the three blocks that have `block1` as parent.
    store
        .delete_block(first_id, TEST_USER_ID)
        .await
        .expect("block1 is deleted");
    let blocks = board_history(store, board_id, &descending).await;
    assert_eq!(
        blocks.len(),
        9,
        "five records for the inserts, one for deleting block1 and three for deleting its \
         children: {blocks:?}"
    );

    // "get full block history after undelete"
    // This undeletes `block1` and its children.
    store
        .undelete_block(first_id, TEST_USER_ID)
        .await
        .expect("block1 is undeleted");
    let blocks = board_history(store, board_id, &descending).await;
    assert_eq!(
        blocks.len(),
        13,
        "nine records from before, one for undeleting block1 and three for undeleting its \
         children: {blocks:?}"
    );

    // "get block history of a board with no history"
    let blocks = board_history(store, "nonexistent-board-id", &HistoryQuery::default()).await;
    assert!(
        blocks.is_empty(),
        "a board without blocks has no history: {blocks:?}"
    );

    common::delete_blocks(store, &blocks_to_insert, "test").await;
    test.teardown().await;
}

/// `testUndeleteBlockChildren`, as far as it goes without the boards area.
#[tokio::test]
#[expect(
    clippy::too_many_lines,
    reason = "one test function of the original, whose subtests share their state"
)]
async fn undelete_block_children() {
    let test = common::setup().await;
    let store = &test.store;
    let user_id = TEST_USER_ID;

    let boards = common::create_test_boards(store, TEST_TEAM_ID, user_id, 2).await;
    let board_delete_id = boards[0].id.clone();
    let board_keep_id = boards[1].id.clone();

    // Some blocks to be deleted.
    let cards_delete = create_test_cards(store, user_id, &board_delete_id, 3).await;
    let blocks_delete = create_test_blocks_for_card(store, &cards_delete[0].id, 5).await;
    assert_eq!(blocks_delete.len(), 5, "five content blocks to delete");

    // Some blocks to keep.
    let cards_keep = create_test_cards(store, user_id, &board_keep_id, 3).await;
    let blocks_keep = create_test_blocks_for_card(store, &cards_keep[0].id, 4).await;
    assert_eq!(blocks_keep.len(), 4, "four content blocks to keep");

    // "undelete block children for card"
    let deleted_card = &cards_delete[0];
    let kept_card = &cards_keep[0];
    let text_children = |card: &Block| {
        BlockFilter::board(&card.board_id)
            .with_parent(&card.id)
            .with_type(BlockType::Text)
    };

    store
        .delete_block(&deleted_card.id, TEST_USER_ID)
        .await
        .expect("the card is deleted");

    // The card was deleted,
    let error = store
        .get_block(&deleted_card.id)
        .await
        .expect_err("the deleted card is gone");
    assert_not_found(&error);

    // and so were its children,
    let blocks = store
        .get_blocks(&text_children(deleted_card))
        .await
        .expect("the children of the deleted card are read");
    assert!(
        blocks.is_empty(),
        "the children of the deleted card are gone: {blocks:?}"
    );

    // while the children of the other card remain.
    let blocks = store
        .get_blocks(&text_children(kept_card))
        .await
        .expect("the children of the other card are read");
    assert_eq!(
        blocks.len(),
        blocks_keep.len(),
        "the children of the other card remain: {blocks:?}"
    );

    store
        .undelete_block(&deleted_card.id, TEST_USER_ID)
        .await
        .expect("the card is undeleted");

    // The card was restored,
    existing_block(store, &deleted_card.id).await;

    // and so were its children.
    let blocks = store
        .get_blocks(&text_children(deleted_card))
        .await
        .expect("the children of the restored card are read");
    assert_eq!(
        blocks.len(),
        blocks_delete.len(),
        "the children of the restored card are back: {blocks:?}"
    );

    // "undelete block children for board"
    store
        .delete_board(&board_delete_id, TEST_USER_ID)
        .await
        .expect("the board is deleted");

    // The board was deleted,
    let error = store
        .get_board(&board_delete_id)
        .await
        .expect_err("the deleted board is gone");
    assert_not_found(&error);

    // and so were all its cards and blocks,
    let blocks = store
        .get_blocks(&BlockFilter::board(&board_delete_id))
        .await
        .expect("the blocks of the deleted board are read");
    assert!(
        blocks.is_empty(),
        "the blocks of the deleted board are gone: {blocks:?}"
    );

    // while the cards and blocks of the other board remain.
    let blocks = store
        .get_blocks(&BlockFilter::board(&board_keep_id))
        .await
        .expect("the blocks of the other board are read");
    assert_eq!(
        blocks.len(),
        blocks_keep.len() + cards_keep.len(),
        "the blocks of the other board remain"
    );

    store
        .undelete_board(&board_delete_id, TEST_USER_ID)
        .await
        .expect("the board is undeleted");

    // The board was restored,
    store
        .get_board(&board_delete_id)
        .await
        .expect("the board is back");

    // and so were its cards and blocks.
    let blocks = store
        .get_blocks(&BlockFilter::board(&board_delete_id))
        .await
        .expect("the blocks of the restored board are read");
    assert_eq!(
        blocks.len(),
        blocks_delete.len() + cards_delete.len(),
        "the blocks of the restored board are back"
    );

    test.teardown().await;
}

// `testGetBlockHistoryNewestChildren` has no counterpart: no code of the standalone server
// calls `GetBlockHistoryNewestChildren`, so the store does not have it.

/// The writes of one task of [`concurrent_writers_on_one_board_all_succeed`]: per round,
/// four writes and one read. On failure, says which operation failed.
async fn write_rounds(
    store: Store,
    board_id: &'static str,
    task: usize,
    rounds: usize,
) -> Result<(), String> {
    let user_id = format!("user-{task}");
    let failed = |round: usize, operation: &str, error: StoreError| {
        format!("task {task}, round {round}: {operation} failed: {error:?}")
    };

    for round in 0..rounds {
        let own_id = format!("task-{task}-round-{round}");
        let mut own = Block {
            parent_id: "shared".to_owned(),
            block_type: BlockType::Text,
            ..block(&own_id, board_id)
        };
        store
            .insert_block(&mut own, &user_id)
            .await
            .map_err(|error| failed(round, "insert", error))?;
        store
            .patch_block(&own_id, &title_patch("patched"), &user_id)
            .await
            .map_err(|error| failed(round, "patch", error))?;
        let shared_title = format!("task {task}, round {round}");
        store
            .patch_block("shared", &title_patch(&shared_title), &user_id)
            .await
            .map_err(|error| failed(round, "patch of the shared block", error))?;
        store
            .get_subtree(board_id, "shared", &SubtreeQuery::default())
            .await
            .map_err(|error| failed(round, "read", error))?;
        if round % 2 == 0 {
            store
                .delete_block(&own_id, &user_id)
                .await
                .map_err(|error| failed(round, "delete", error))?;
        } else {
            store
                .patch_block(&own_id, &title_patch("kept"), &user_id)
                .await
                .map_err(|error| failed(round, "second patch", error))?;
        }
    }
    Ok(())
}

/// Many tasks insert, patch and delete blocks of one board through clones of one store
/// at the same time. Every operation must succeed, and afterwards the blocks and the
/// history must add up.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_writers_on_one_board_all_succeed() {
    const BOARD_ID: &str = "board-concurrent";
    const TASKS: usize = 16;
    /// Four writes per round, so twenty writes per task.
    const ROUNDS: usize = 5;
    /// The rounds of a task that end by deleting the block they inserted.
    const DELETING_ROUNDS: usize = ROUNDS.div_ceil(2);

    let test = common::setup().await;
    let store = &test.store;

    // One block that every task patches, so that writes to the same block race too.
    let mut shared = Block {
        block_type: BlockType::Card,
        ..block("shared", BOARD_ID)
    };
    store
        .insert_block(&mut shared, TEST_USER_ID)
        .await
        .expect("the shared block is inserted");

    let mut writers = JoinSet::new();
    for task in 0..TASKS {
        writers.spawn(write_rounds(store.clone(), BOARD_ID, task, ROUNDS));
    }
    while let Some(joined) = writers.join_next().await {
        let outcome = joined.expect("a writer task runs to its end");
        assert_eq!(outcome, Ok(()), "every operation of every task succeeds");
    }

    // The blocks: the shared one and, per task, those of the rounds that did not delete.
    let blocks = board_blocks(store, BOARD_ID).await;
    assert_eq!(
        blocks.len(),
        1 + TASKS * (ROUNDS - DELETING_ROUNDS),
        "the shared block and the surviving blocks of every task: {:?}",
        common::sorted_ids(&blocks)
    );
    for block in blocks.iter().filter(|block| block.id != "shared") {
        assert_eq!(
            block.title, "kept",
            "block {} survives with its last patch",
            block.id
        );
    }

    // The history: one record per write.
    let history = board_history(store, BOARD_ID, &HistoryQuery::default()).await;
    assert_eq!(
        history.len(),
        1 + TASKS * ROUNDS * 4,
        "one history record for the shared block's insert and one per write of every task"
    );
    let shared_history = store
        .get_block_history("shared", &HistoryQuery::default())
        .await
        .expect("the history of the shared block is read");
    assert_eq!(
        shared_history.len(),
        1 + TASKS * ROUNDS,
        "one history record per patch of the shared block, after the one for its insert"
    );

    // The shared block is in the state of its newest history record.
    let shared = existing_block(store, "shared").await;
    let newest = shared_history
        .last()
        .expect("the shared block has a history");
    assert_eq!(
        (&shared.title, &shared.modified_by, shared.update_at),
        (&newest.title, &newest.modified_by, newest.update_at),
        "the shared block matches its newest history record"
    );

    // Every block a task wrote has three records, and its last one says whether the
    // block was deleted.
    for task in 0..TASKS {
        for round in 0..ROUNDS {
            let own_id = format!("task-{task}-round-{round}");
            let own_history: Vec<&Block> =
                history.iter().filter(|block| block.id == own_id).collect();
            assert_eq!(
                own_history.len(),
                3,
                "block {own_id} was written three times: {own_history:?}"
            );
            let deleted = own_history[2].delete_at != 0;
            assert_eq!(
                deleted,
                round % 2 == 0,
                "the newest record of block {own_id} says whether it was deleted"
            );
            assert_eq!(
                common::contains_block_with_id(&blocks, &own_id),
                !deleted,
                "block {own_id} exists unless it was deleted"
            );
        }
    }

    test.teardown().await;
}

/// Writes one block many times in quick succession, each write in a transaction of its
/// own, and checks that every write got a history record, in the order of the writes.
async fn assert_rapid_writes_get_a_history_record_each(store: &Store) {
    const UPDATES: usize = 200;

    let board_id = "board-rapid";

    let mut rapid = Block {
        title: "write 0".to_owned(),
        ..block("rapid", board_id)
    };
    store
        .insert_block(&mut rapid, TEST_USER_ID)
        .await
        .expect("the block is inserted");

    // Updates through both ways of writing a block.
    for write in 1..=UPDATES {
        let title = format!("write {write}");
        if write % 2 == 0 {
            rapid.title = title;
            store
                .insert_block(&mut rapid, TEST_USER_ID)
                .await
                .unwrap_or_else(|error| panic!("update {write} should succeed: {error:?}"));
        } else {
            store
                .patch_block("rapid", &title_patch(&title), TEST_USER_ID)
                .await
                .unwrap_or_else(|error| panic!("patch {write} should succeed: {error:?}"));
        }
    }

    let history = store
        .get_block_history("rapid", &HistoryQuery::default())
        .await
        .expect("the history is read");
    let titles: Vec<&str> = history.iter().map(|block| block.title.as_str()).collect();
    let expected: Vec<String> = (0..=UPDATES)
        .map(|write| format!("write {write}"))
        .collect();
    assert_eq!(
        titles, expected,
        "one history record per write, oldest first"
    );

    // Deleting and undeleting write history records too.
    for round in 0..10 {
        store
            .delete_block("rapid", TEST_USER_ID)
            .await
            .unwrap_or_else(|error| panic!("delete {round} should succeed: {error:?}"));
        store
            .undelete_block("rapid", TEST_USER_ID)
            .await
            .unwrap_or_else(|error| panic!("undelete {round} should succeed: {error:?}"));
    }
    let history = store
        .get_block_history("rapid", &HistoryQuery::default())
        .await
        .expect("the history is read");
    assert_eq!(
        history.len(),
        1 + UPDATES + 20,
        "one more history record per delete and per undelete"
    );
    let deleted: Vec<bool> = history[1 + UPDATES..]
        .iter()
        .map(|block| block.delete_at != 0)
        .collect();
    let alternating: Vec<bool> = (0..20).map(|index| index % 2 == 0).collect();
    assert_eq!(
        deleted, alternating,
        "the delete and undelete records alternate in the order they were written"
    );
}

/// Writes to one block in quick succession, without the sleeps the original needs, each
/// get a history record of their own.
#[tokio::test]
async fn rapid_writes_to_one_block_never_collide_in_history() {
    let test = common::setup().await;

    assert_rapid_writes_get_a_history_record_each(&test.store).await;

    test.teardown().await;
}

/// The same on an in-memory SQLite database, where a write takes a small fraction of a
/// millisecond, so that many writes of separate transactions fall into one. A database
/// file is too slow for that: every commit waits for the disk.
#[tokio::test]
async fn rapid_writes_within_one_millisecond_never_collide_in_history() {
    if common::db_type() != DbType::Sqlite {
        // The millisecond is the resolution of SQLite's clock; this is about SQLite only.
        return;
    }
    // Not `common::setup`, which makes a database file.
    let store = Store::connect(DbType::Sqlite, "file::memory:", common::TABLE_PREFIX)
        .await
        .expect("an in-memory database");

    assert_rapid_writes_get_a_history_record_each(&store).await;

    store.close().await;
}

/// One transaction that writes the same block many times gets a history record per
/// write, in the order of the writes.
///
/// On SQLite such writes take far less than a millisecond each, so their history
/// timestamps run ahead of the clock, a millisecond apart, through every millisecond of a
/// second and more.
#[tokio::test]
async fn one_batch_writing_a_block_many_times_never_collides_in_history() {
    const WRITES: usize = 1100;

    let test = common::setup().await;
    let store = &test.store;

    let mut batch: Vec<Block> = (0..WRITES)
        .map(|write| Block {
            title: format!("write {write}"),
            ..block("batched", "board-batch")
        })
        .collect();
    store
        .insert_blocks(&mut batch, TEST_USER_ID)
        .await
        .expect("a batch may write one block many times");

    let history = store
        .get_block_history("batched", &HistoryQuery::default())
        .await
        .expect("the history is read");
    assert_eq!(history.len(), WRITES, "one history record per write");
    for (write, record) in history.iter().enumerate() {
        assert_eq!(
            record.title,
            format!("write {write}"),
            "history record {write} is that of write {write}"
        );
    }
    let stored = existing_block(store, "batched").await;
    assert_eq!(
        stored.title,
        format!("write {}", WRITES - 1),
        "the block is in the state of the last write"
    );

    test.teardown().await;
}

/// `get_blocks_by_ids` succeeds only if it finds exactly one block per identifier.
#[tokio::test]
async fn get_blocks_by_ids_wants_one_block_per_identifier() {
    let test = common::setup().await;
    let store = &test.store;

    let mut sample = typed_sample_blocks();
    common::insert_blocks(store, &mut sample, TEST_USER_ID).await;

    let all = ids(["block1", "block2", "block3", "block4", "block5"]);
    let blocks = store
        .get_blocks_by_ids(&all)
        .await
        .expect("all five blocks exist");
    assert_eq!(
        common::sorted_ids(&blocks),
        all,
        "exactly the blocks asked for"
    );

    let blocks = store
        .get_blocks_by_ids(&[])
        .await
        .expect("asking for no blocks finds all of them");
    assert!(
        blocks.is_empty(),
        "no identifiers, no blocks: {}",
        blocks.len()
    );

    let asked = ids(["block1", "missing"]);
    let error = store
        .get_blocks_by_ids(&asked)
        .await
        .expect_err("one block is missing");
    match &error {
        StoreError::NotAllFound { entity, ids } => {
            assert_eq!(*entity, "block", "the error names the kind of record");
            assert_eq!(ids, &asked, "the error names the identifiers asked for");
        }
        other => panic!("expected a not-all-found error, got {other:?}"),
    }
    assert!(
        error.is_not_found(),
        "the error counts as not found: {error:?}"
    );

    // Preserved quirk: the same identifier twice finds one block for two identifiers.
    let error = store
        .get_blocks_by_ids(&ids(["block1", "block1"]))
        .await
        .expect_err("one block for two identifiers is not all found");
    assert!(
        matches!(error, StoreError::NotAllFound { .. }),
        "expected a not-all-found error, got {error:?}"
    );

    test.teardown().await;
}

/// `get_blocks` returns the page asked for, together with the other filters.
#[tokio::test]
async fn get_blocks_pages_the_result() {
    let test = common::setup().await;
    let store = &test.store;
    let board_id = new_id(IdKind::Board);

    let cards = create_test_cards(store, TEST_USER_ID, &board_id, 5).await;
    create_test_blocks_for_card(store, &cards[0].id, 3).await;
    let card_ids: BTreeSet<String> = cards.iter().map(|card| card.id.clone()).collect();

    // Cards only, as the handler that lists the cards of a board asks for them.
    let page = |page: u32, per_page: u32| BlockFilter {
        paging: Some(Paging { page, per_page }),
        ..BlockFilter::board(&board_id).with_type(BlockType::Card)
    };

    let mut seen = BTreeSet::new();
    for (number, expected_len) in [(0, 2), (1, 2), (2, 1), (3, 0)] {
        let blocks = store
            .get_blocks(&page(number, 2))
            .await
            .expect("a page is read");
        assert_eq!(
            blocks.len(),
            expected_len,
            "page {number} of five cards, two per page: {blocks:?}"
        );
        seen.extend(blocks.into_iter().map(|block| block.id));
    }
    assert_eq!(seen, card_ids, "the pages together hold every card once");

    let blocks = store
        .get_blocks(&page(0, 10))
        .await
        .expect("a page is read");
    assert_eq!(
        blocks.len(),
        5,
        "a page larger than the result holds all of it: {blocks:?}"
    );

    // Like the original, which limits only if the page size is positive
    // (`server/services/store/sqlstore/blocks.go:77`).
    for number in [0, 3] {
        let blocks = store
            .get_blocks(&page(number, 0))
            .await
            .expect("a page is read");
        assert_eq!(
            blocks.len(),
            5,
            "a page size of zero does not limit, whatever the page number {number}: {blocks:?}"
        );
    }

    test.teardown().await;
}

/// Preserved quirk: the empty type and the type `unknown` do not filter
/// (`server/services/store/sqlstore/blocks.go:69`).
#[tokio::test]
async fn get_blocks_does_not_filter_by_the_empty_or_the_unknown_type() {
    let test = common::setup().await;
    let store = &test.store;

    let mut sample = typed_sample_blocks();
    sample.push(typed_block("typeless", "", ""));
    sample.push(typed_block("unknown-typed", "", "unknown"));
    common::insert_blocks(store, &mut sample, TEST_USER_ID).await;

    for block_type in ["", "unknown"] {
        let filter = BlockFilter::board(TEST_BOARD_ID).with_type(BlockType::from(block_type));
        let blocks = store.get_blocks(&filter).await.expect("the query runs");
        assert_eq!(
            blocks.len(),
            7,
            "the type {block_type:?} selects every block of the board: {:?}",
            common::sorted_ids(&blocks)
        );

        let filter = filter.with_parent("block1");
        let blocks = store.get_blocks(&filter).await.expect("the query runs");
        assert_eq!(
            common::sorted_ids(&blocks),
            ["block2", "block3", "block4"],
            "the type {block_type:?} leaves the other filters in force"
        );
    }

    test.teardown().await;
}

/// A batch with more identifiers than patches is rejected as a whole.
#[tokio::test]
async fn patch_blocks_with_more_ids_than_patches_changes_nothing() {
    let test = common::setup().await;
    let store = &test.store;
    let board_id = "board-batch";

    let mut blocks = [
        Block {
            title: "first".to_owned(),
            ..block("first", board_id)
        },
        Block {
            title: "second".to_owned(),
            ..block("second", board_id)
        },
    ];
    store
        .insert_blocks(&mut blocks, TEST_USER_ID)
        .await
        .expect("the blocks are inserted");

    let batch = BlockPatchBatch {
        block_ids: ids(["first", "second"]),
        block_patches: vec![title_patch("patched")],
    };
    let error = store
        .patch_blocks(&batch, TEST_USER_ID)
        .await
        .expect_err("the second identifier has no patch");
    assert!(
        matches!(
            error,
            StoreError::InvalidBoardsAndBlocks(BoardsAndBlocksError::BlockIdsAndPatchesMismatch)
        ),
        "expected a mismatch error, got {error:?}"
    );

    let first = existing_block(store, "first").await;
    assert_eq!(
        first.title, "first",
        "the patch of the first block was rolled back"
    );
    let history = board_history(store, board_id, &HistoryQuery::default()).await;
    assert_eq!(
        history.len(),
        2,
        "the rejected batch left no history record: {history:?}"
    );

    // More patches than identifiers is fine: the surplus is ignored, as in the original.
    let batch = BlockPatchBatch {
        block_ids: ids(["first"]),
        block_patches: vec![title_patch("patched"), title_patch("ignored")],
    };
    store
        .patch_blocks(&batch, TEST_USER_ID)
        .await
        .expect("surplus patches are ignored");
    let first = existing_block(store, "first").await;
    assert_eq!(first.title, "patched", "the first block is patched");
    let second = existing_block(store, "second").await;
    assert_eq!(second.title, "second", "the second block is untouched");

    test.teardown().await;
}

/// Deleting a block that does not exist succeeds and writes nothing.
#[tokio::test]
async fn delete_block_of_a_missing_block_writes_nothing() {
    let test = common::setup().await;
    let store = &test.store;

    let mut sample = three_blocks();
    common::insert_blocks(store, &mut sample, TEST_USER_ID).await;

    store
        .delete_block("missing", TEST_USER_ID)
        .await
        .expect("deleting a missing block succeeds");

    let blocks = board_blocks(store, TEST_BOARD_ID).await;
    assert_eq!(
        common::sorted_ids(&blocks),
        ["block1", "block2", "block3"],
        "the blocks that exist are untouched"
    );
    let history = board_history(store, TEST_BOARD_ID, &HistoryQuery::default()).await;
    assert_eq!(
        history.len(),
        3,
        "only the three inserts are in the history: {history:?}"
    );
    let missing_history = store
        .get_block_history("missing", &HistoryQuery::default())
        .await
        .expect("the history is read");
    assert!(
        missing_history.is_empty(),
        "the missing block got no history record: {missing_history:?}"
    );

    test.teardown().await;
}

/// Undeleting a block that is not deleted succeeds and writes nothing.
#[tokio::test]
async fn undelete_block_of_a_block_that_is_not_deleted_writes_nothing() {
    let test = common::setup().await;
    let store = &test.store;

    let mut parent = block("parent", TEST_BOARD_ID);
    store
        .insert_block(&mut parent, "creator")
        .await
        .expect("the block is inserted");
    // A deleted child, which undeleting the parent would restore if it did anything.
    let mut child = typed_block("child", "parent", "text");
    store
        .insert_block(&mut child, "creator")
        .await
        .expect("the child is inserted");
    store
        .delete_block("child", "creator")
        .await
        .expect("the child is deleted");
    let before = existing_block(store, "parent").await;

    store
        .undelete_block("parent", "someone-else")
        .await
        .expect("undeleting a block that is not deleted succeeds");

    let after = existing_block(store, "parent").await;
    assert_eq!(after, before, "the block is untouched");
    assert_no_block(store, "child").await;
    let history = board_history(store, TEST_BOARD_ID, &HistoryQuery::default()).await;
    assert_eq!(
        history.len(),
        3,
        "two inserts and one delete, and nothing for the undelete: {history:?}"
    );

    test.teardown().await;
}

/// Preserved quirk: a block may have the empty identifier, and deleting or undeleting it
/// takes every block of its board along, not only its children
/// (`server/services/store/sqlstore/blocks.go:450`, `:522`, `:929`, `:1017`).
#[tokio::test]
async fn deleting_the_block_with_the_empty_id_takes_its_whole_board_along() {
    let test = common::setup().await;
    let store = &test.store;

    let mut sample = subtree_sample_blocks();
    sample.push(block("", TEST_BOARD_ID));
    common::insert_blocks(store, &mut sample, TEST_USER_ID).await;
    let mut other_board = block("elsewhere", "other-board");
    store
        .insert_block(&mut other_board, TEST_USER_ID)
        .await
        .expect("a block of another board is inserted");

    store
        .delete_block("", TEST_USER_ID)
        .await
        .expect("the block with the empty identifier is deleted");
    let blocks = board_blocks(store, TEST_BOARD_ID).await;
    assert!(
        blocks.is_empty(),
        "every block of the board is gone, whatever its parent: {:?}",
        common::sorted_ids(&blocks)
    );
    existing_block(store, "elsewhere").await;

    store
        .undelete_block("", TEST_USER_ID)
        .await
        .expect("the block with the empty identifier is undeleted");
    let blocks = board_blocks(store, TEST_BOARD_ID).await;
    assert_eq!(
        common::sorted_ids(&blocks),
        common::sorted_ids(&sample),
        "every block of the board is back"
    );

    test.teardown().await;
}

/// Preserved quirk: writing a block that exists under another board leaves the block as
/// it is and still adds a history record (`server/services/store/sqlstore/blocks.go:283`).
#[tokio::test]
async fn insert_block_under_another_board_writes_only_history() {
    let test = common::setup().await;
    let store = &test.store;

    let mut original = Block {
        title: "original".to_owned(),
        ..block("moving", "board-a")
    };
    store
        .insert_block(&mut original, "user-a")
        .await
        .expect("the block is inserted");
    let before = existing_block(store, "moving").await;

    let mut moved = Block {
        title: "moved".to_owned(),
        ..block("moving", "board-b")
    };
    store
        .insert_block(&mut moved, "user-b")
        .await
        .expect("writing the block under another board succeeds");

    let after = existing_block(store, "moving").await;
    assert_eq!(after, before, "the block is untouched");
    let blocks = board_blocks(store, "board-b").await;
    assert!(
        blocks.is_empty(),
        "the other board did not get the block: {blocks:?}"
    );
    let history = store
        .get_block_history("moving", &HistoryQuery::default())
        .await
        .expect("the history is read");
    let records: Vec<(&str, &str, &str)> = history
        .iter()
        .map(|record| {
            (
                record.board_id.as_str(),
                record.title.as_str(),
                record.modified_by.as_str(),
            )
        })
        .collect();
    assert_eq!(
        records,
        [
            ("board-a", "original", "user-a"),
            ("board-b", "moved", "user-b")
        ],
        "the write that changed nothing is in the history all the same"
    );

    test.teardown().await;
}

/// Preserved quirk: the history record of an update names the updating user as creator
/// and the time of the update as creation time, while the block keeps both
/// (`server/services/store/sqlstore/blocks.go:274`, `:276`, `:309`).
#[tokio::test]
async fn the_history_record_of_an_update_names_the_updater_as_creator() {
    let test = common::setup().await;
    let store = &test.store;

    let mut block = block("updated", TEST_BOARD_ID);
    store
        .insert_block(&mut block, "creator")
        .await
        .expect("the block is inserted");
    let created = existing_block(store, "updated").await;

    // A later millisecond, so that the two times can be told apart.
    tokio::time::sleep(Duration::from_millis(2)).await;
    store
        .patch_block("updated", &title_patch("new title"), "updater")
        .await
        .expect("the block is updated");

    let updated = existing_block(store, "updated").await;
    assert_eq!(
        (updated.created_by.as_str(), updated.create_at),
        ("creator", created.create_at),
        "the block keeps its creator and creation time"
    );
    assert_eq!(
        updated.modified_by, "updater",
        "the block has a new modifier"
    );
    assert!(
        updated.update_at > created.update_at,
        "the block has a later modification time: {} after {}",
        updated.update_at,
        created.update_at
    );

    let history = store
        .get_block_history("updated", &HistoryQuery::default())
        .await
        .expect("the history is read");
    assert_eq!(history.len(), 2, "an insert and an update: {history:?}");
    let record = &history[1];
    assert_eq!(
        (record.created_by.as_str(), record.modified_by.as_str()),
        ("updater", "updater"),
        "the record of the update names the updater as creator too"
    );
    assert_eq!(
        (record.create_at, record.update_at),
        (updated.update_at, updated.update_at),
        "the record of the update has the time of the update as creation time too"
    );

    test.teardown().await;
}

/// Deleting an image or an attachment, or a card that holds one, succeeds and marks the
/// file as deleted. The original fails to delete such blocks when it runs standalone,
/// because it updates a table that only its host application has
/// (`server/services/store/sqlstore/blocks.go:429`, `:986`).
#[tokio::test]
async fn deleting_blocks_that_hold_files_marks_the_files_deleted() {
    let test = common::setup().await;
    let store = &test.store;
    let board_id = new_id(IdKind::Board);

    // The blocks area has no operation for file infos, so their rows are written and
    // checked on SQLite only; on PostgreSQL the test checks that the deletions work.
    let mut raw = match common::db_type() {
        DbType::Sqlite => Some(sqlite_connection(&test).await),
        DbType::Postgres => None,
    };
    if let Some(raw) = &mut raw {
        insert_file_infos(raw, ["direct", "image", "attachment", "untouched"]).await;
    }

    // A file name in a block is `7`, the identifier of the file info, and an extension.
    let cards = create_test_cards(store, TEST_USER_ID, &board_id, 2).await;
    let mut blocks: Vec<Block> = [
        ("direct", 0, "image", "fileId", "7direct.png"),
        ("image", 1, "image", "fileId", "7image.png"),
        (
            "attachment",
            1,
            "attachment",
            "attachmentId",
            "7attachment.pdf",
        ),
        ("kept", 0, "image", "fileId", "7untouched.png"),
    ]
    .into_iter()
    .map(|(id, card, block_type, field, file_name)| Block {
        parent_id: cards[card].id.clone(),
        block_type: BlockType::from(block_type),
        fields: fields(json!({ field: file_name })),
        ..block(id, &board_id)
    })
    .collect();
    common::insert_blocks(store, &mut blocks, TEST_USER_ID).await;

    // An image on its own, then a card with an image and an attachment.
    store
        .delete_block("direct", TEST_USER_ID)
        .await
        .expect("an image block is deleted");
    store
        .delete_block(&cards[1].id, TEST_USER_ID)
        .await
        .expect("a card that holds files is deleted");

    let left = board_blocks(store, &board_id).await;
    let mut expected_left = vec![cards[0].id.clone(), "kept".to_owned()];
    expected_left.sort();
    assert_eq!(
        common::sorted_ids(&left),
        expected_left,
        "the first card and its remaining image are left"
    );

    if let Some(mut raw) = raw {
        assert_eq!(
            deleted_file_infos(&mut raw).await,
            ["attachment", "direct", "image"],
            "the files of the deleted blocks, and only those, are marked deleted"
        );
        raw.close()
            .await
            .expect("the second connection closes cleanly");
    }

    test.teardown().await;
}

/// The bounds of a subtree query include the boundary and combine with a limit.
#[tokio::test]
async fn get_subtree_applies_bounds_and_limit() {
    let test = common::setup().await;
    let store = &test.store;
    let board_id = TEST_BOARD_ID;

    let mut sample = subtree_sample_blocks();
    for block in &mut sample[..3] {
        // The bounds are on the modification time, which is the wall clock in
        // milliseconds; the three blocks of the subtree need three different ones.
        tokio::time::sleep(Duration::from_millis(2)).await;
        common::insert_blocks(store, std::slice::from_mut(block), TEST_USER_ID).await;
    }
    let child1_update_at = sample[1].update_at;
    let subtree_ids = async |query: SubtreeQuery| -> Vec<String> {
        let blocks = store
            .get_subtree(board_id, "parent", &query)
            .await
            .expect("the subtree is read");
        blocks.into_iter().map(|block| block.id).collect()
    };

    assert_eq!(
        subtree_ids(SubtreeQuery::default()).await,
        ["parent", "child1", "child2"],
        "the whole subtree, oldest first"
    );
    let query = SubtreeQuery {
        before_update_at: Some(child1_update_at),
        ..SubtreeQuery::default()
    };
    assert_eq!(
        subtree_ids(query).await,
        ["parent", "child1"],
        "blocks updated at or before the bound"
    );
    let query = SubtreeQuery {
        after_update_at: Some(child1_update_at),
        ..SubtreeQuery::default()
    };
    assert_eq!(
        subtree_ids(query).await,
        ["child1", "child2"],
        "blocks updated at or after the bound"
    );
    let query = SubtreeQuery {
        limit: Some(1),
        ..query
    };
    assert_eq!(
        subtree_ids(query).await,
        ["child1"],
        "the oldest block within the bounds"
    );

    test.teardown().await;
}

/// `get_block_counts_by_type` counts the blocks that exist, per type.
#[tokio::test]
async fn get_block_counts_by_type_counts_existing_blocks() {
    let test = common::setup().await;
    let store = &test.store;

    let counts = store
        .get_block_counts_by_type()
        .await
        .expect("the counts are read");
    assert!(
        counts.is_empty(),
        "an empty store has no counts: {counts:?}"
    );

    let mut sample = typed_sample_blocks();
    common::insert_blocks(store, &mut sample, TEST_USER_ID).await;
    let mut other_board = Block {
        block_type: BlockType::from("test2"),
        ..block("elsewhere", "other-board")
    };
    store
        .insert_block(&mut other_board, TEST_USER_ID)
        .await
        .expect("a block of another board is inserted");
    store
        .delete_block("block5", TEST_USER_ID)
        .await
        .expect("a block is deleted");

    let counts = store
        .get_block_counts_by_type()
        .await
        .expect("the counts are read");
    let expected = [("test".to_owned(), 3), ("test2".to_owned(), 2)];
    assert_eq!(
        counts.into_iter().collect::<Vec<_>>(),
        expected,
        "blocks of every board, without the deleted one"
    );

    test.teardown().await;
}

/// The original's SQLite driver stores the JSON of a block's fields as a blob, where this
/// store writes text. A database written by the original must still be readable.
#[tokio::test]
async fn sqlite_fields_stored_as_a_blob_are_read() {
    if common::db_type() != DbType::Sqlite {
        // PostgreSQL has a JSON column there, which the original writes like this store.
        return;
    }
    let test = common::setup().await;
    let store = &test.store;

    let mut card = Block {
        block_type: BlockType::Card,
        fields: fields(json!({ "icon": "🎨", "contentOrder": ["a", ["b", "c"]] })),
        ..block("card", TEST_BOARD_ID)
    };
    store
        .insert_block(&mut card, TEST_USER_ID)
        .await
        .expect("the card is inserted");
    store
        .delete_block("card", TEST_USER_ID)
        .await
        .expect("the card is deleted");
    store
        .undelete_block("card", TEST_USER_ID)
        .await
        .expect("the card is undeleted");

    // Rewrite the fields the way the original's driver stores them.
    let mut raw = sqlite_connection(&test).await;
    for table in ["blocks", "blocks_history"] {
        let rewritten = sqlx::query(sqlx::AssertSqlSafe(format!(
            "UPDATE {}{table} SET fields = CAST(fields AS BLOB)",
            common::TABLE_PREFIX
        )))
        .execute(&mut raw)
        .await
        .expect("the fields are rewritten as blobs");
        assert_ne!(
            rewritten.rows_affected(),
            0,
            "the {table} table has rows to rewrite"
        );
    }
    raw.close()
        .await
        .expect("the second connection closes cleanly");

    let stored = existing_block(store, "card").await;
    assert_eq!(
        stored.fields, card.fields,
        "the fields of a block are read from a blob"
    );
    let history = store
        .get_block_history("card", &HistoryQuery::default())
        .await
        .expect("history records with blob fields are read");
    assert_eq!(history.len(), 3, "insert, delete and undelete: {history:?}");
    for record in &history {
        assert_eq!(
            record.fields, card.fields,
            "the fields of a history record are read from a blob"
        );
    }

    // Deleting and undeleting copy the fields between the two tables.
    store
        .delete_block("card", TEST_USER_ID)
        .await
        .expect("a block with blob fields is deleted");
    store
        .undelete_block("card", TEST_USER_ID)
        .await
        .expect("a block with blob fields is undeleted");
    let restored = existing_block(store, "card").await;
    assert_eq!(
        restored.fields, card.fields,
        "the fields survive a delete and an undelete"
    );

    test.teardown().await;
}
