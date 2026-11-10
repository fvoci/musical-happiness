//! Port of `server/services/store/storetests/blocks.go`.
//!
//! There is one test per test function of the original, because the subtests of one
//! function share a store and run in order. Each subtest is a section headed by its name
//! in quotes. The original sleeps a millisecond between writes so that two history rows of
//! one block cannot collide; the store rules such collisions out, so those sleeps are not
//! ported.
//!
//! The original names its users, boards and blocks with made-up text such as `user-id-1`,
//! `board-id` and `block1`, and none of them has to exist. Here an identifier is a UUID,
//! and what a block refers to (its board, the users who write it) is stored before the
//! block. So every test first stores its users, its team and its boards, and the sample
//! blocks carry the names the original gives them as their titles.
//!
//! After the ported suite come tests the original does not have: concurrent writers, rapid
//! writes to one block, and behavior of the block operations that its suite leaves
//! uncovered.

use std::collections::BTreeSet;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use fvoci_model::{
    BLOCK_FIELDS_MAX_CHARS, BLOCK_TITLE_MAX_CHARS, Block, BlockError, BlockPatch, BlockPatchBatch,
    BlockType, BoardsAndBlocksError, FileInfo, JsonMap, new_id,
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

/// `subtreeSampleBlocks` (`server/services/store/storetests/blocks.go:529`): the name of
/// each block, the name of its parent and its type.
const SUBTREE_SAMPLE: [(&str, &str, &str); 6] = [
    ("parent", "", ""),
    ("child1", "parent", ""),
    ("child2", "parent", ""),
    ("grandchild1", "child1", ""),
    ("grandchild2", "child2", ""),
    ("greatgrandchild1", "grandchild1", ""),
];

/// The five blocks that `testGetBlocks` and `testGetBlockMetadata` insert: the name of
/// each block, the name of its parent and its type.
const TYPED_SAMPLE: [(&str, &str, &str); 5] = [
    ("block1", "", "test"),
    ("block2", "block1", "test"),
    ("block3", "block1", "test"),
    ("block4", "block1", "test2"),
    ("block5", "block2", "test"),
];

/// What has to exist before a block does: a user to write it and a board to hold it,
/// which in turn is a team's.
#[expect(
    clippy::struct_field_names,
    reason = "the members are identifiers, named like the variables that hold them"
)]
struct Fixture {
    /// Takes the place of `testUserID` of the original suite.
    user_id: String,
    team_id: String,
    /// Takes the place of `testBoardID` of the original suite.
    board_id: String,
}

/// Stores a user, a team and a board of that team.
async fn fixture(store: &Store) -> Fixture {
    let user_id = common::user(store, "test-user").await;
    let team_id = common::team(store).await;
    let board_id = another_board(store, &team_id, &user_id).await;
    Fixture {
        user_id,
        team_id,
        board_id,
    }
}

/// Stores one more board of a team and returns its identifier.
async fn another_board(store: &Store, team_id: &str, user_id: &str) -> String {
    common::create_test_boards(store, team_id, user_id, 1)
        .await
        .into_iter()
        .next()
        .expect("one board was asked for")
        .id
}

/// Stores `N` users, which take the place of `user-id-1`, `user-id-2` and so on of the
/// original suite, and returns their identifiers.
async fn users<const N: usize>(store: &Store) -> [String; N] {
    let mut user_ids = Vec::with_capacity(N);
    for number in 1..=N {
        user_ids.push(common::user(store, &format!("user-{number}")).await);
    }
    user_ids
        .try_into()
        .expect("one identifier per user that was stored")
}

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

/// Blocks of a board with new identifiers, from the name, the parent's name and the type
/// of each. A parent comes before its children; the empty name means no parent.
///
/// The name of a block is its title, and [`id_of`] finds its identifier by it.
fn named_blocks(board_id: &str, specs: &[(&str, &str, &str)]) -> Vec<Block> {
    let mut blocks: Vec<Block> = Vec::with_capacity(specs.len());
    for &(name, parent, block_type) in specs {
        let parent_id = if parent.is_empty() {
            String::new()
        } else {
            id_of(&blocks, parent).to_owned()
        };
        blocks.push(Block {
            parent_id,
            block_type: BlockType::from(block_type),
            title: name.to_owned(),
            ..block(&new_id(), board_id)
        });
    }
    blocks
}

/// The identifier of the block that [`named_blocks`] made under the given name.
#[track_caller]
fn id_of<'a>(blocks: &'a [Block], name: &str) -> &'a str {
    blocks
        .iter()
        .find(|block| block.title == name)
        .map_or_else(|| panic!("no block is named {name}"), |block| &block.id)
}

/// The names of blocks that [`named_blocks`] made, sorted.
fn sorted_names(blocks: &[Block]) -> Vec<&str> {
    let mut names: Vec<&str> = blocks.iter().map(|block| block.title.as_str()).collect();
    names.sort_unstable();
    names
}

/// The blocks of [`SUBTREE_SAMPLE`] on a board.
fn subtree_sample_blocks(board_id: &str) -> Vec<Block> {
    named_blocks(board_id, &SUBTREE_SAMPLE)
}

/// The blocks of [`TYPED_SAMPLE`] on a board.
fn typed_sample_blocks(board_id: &str) -> Vec<Block> {
    named_blocks(board_id, &TYPED_SAMPLE)
}

/// The three blocks that `testDeleteBlock` and `testUndeleteBlock` insert.
fn three_blocks(board_id: &str) -> Vec<Block> {
    named_blocks(
        board_id,
        &[("block1", "", ""), ("block2", "", ""), ("block3", "", "")],
    )
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

/// Owned strings from string slices: a list of identifiers, or of field names.
fn strings<const N: usize>(strings: [&str; N]) -> Vec<String> {
    strings.into_iter().map(str::to_owned).collect()
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
fn assert_invalid_block(error: &StoreError, expected: &BlockError) {
    assert!(
        matches!(error, StoreError::InvalidBlock { source, .. } if source == expected),
        "expected the validation error {expected:?}, got {error:?}"
    );
}

/// Asserts that a write was refused because `text` is no identifier.
#[track_caller]
fn assert_invalid_id(error: &StoreError, text: &str) {
    assert!(
        matches!(error, StoreError::InvalidId(refused) if refused == text),
        "expected {text:?} to be refused as no identifier, got {error:?}"
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

/// `createTestCards` (`server/services/store/storetests/util.go:76`). Like the original,
/// it gives a card its board as parent.
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
            ..block(&new_id(), board_id)
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
            ..block(&new_id(), &card.board_id)
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

/// Stores the record of a file that a board has under the given stored name.
async fn save_file(store: &Store, board_id: &str, stored_name: &str) {
    let file_info = FileInfo {
        id: new_id(),
        board_id: board_id.to_owned(),
        stored_name: stored_name.to_owned(),
        name: format!("uploaded as {stored_name}"),
        size: 1,
        path: format!("{board_id}/{stored_name}"),
        create_at: now_millis(),
        ..FileInfo::default()
    };
    store
        .save_file_info(&file_info)
        .await
        .expect("the record of a file is stored");
}

/// Whether the file that a board has under the given stored name is marked as deleted.
async fn file_is_deleted(store: &Store, board_id: &str, stored_name: &str) -> bool {
    let file_info = store
        .get_file_info(board_id, stored_name)
        .await
        .unwrap_or_else(|error| panic!("file {stored_name} should have a record: {error:?}"));
    file_info.delete_at != 0
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
    let Fixture {
        user_id: owner_id,
        team_id,
        board_id,
    } = fixture(store).await;
    let board_id = board_id.as_str();
    // `board-id-1` of the original.
    let other_board_id = another_board(store, &team_id, &owner_id).await;
    let [user1, user2, user3, user4, user5, user6] = users(store).await;

    let initial_count = board_blocks(store, board_id).await.len();

    // "valid block"
    let test_id = new_id();
    let mut valid = block(&test_id, board_id);
    store
        .insert_block(&mut valid, &user1)
        .await
        .expect("a valid block is inserted");
    let blocks = board_blocks(store, board_id).await;
    assert_eq!(
        blocks.len(),
        initial_count + 1,
        "the board has one more block: {blocks:?}"
    );

    // "invalid rootid"
    let mut without_board = block(&test_id, "");
    let error = store
        .insert_block(&mut without_board, &user1)
        .await
        .expect_err("a block without a board is rejected");
    assert_invalid_block(&error, &BlockError::EmptyBoardId);
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
        ..block(&test_id, board_id)
    };
    let error = store
        .insert_block(&mut long_title, &user1)
        .await
        .expect_err("a title over the limit is rejected");
    assert_invalid_block(&error, &BlockError::TitleSizeLimitExceeded);

    // "block with aggregated fields size too large"
    let mut large_fields = Block {
        fields: oversized_fields(),
        ..block(&test_id, board_id)
    };
    let error = store
        .insert_block(&mut large_fields, &user2)
        .await
        .expect_err("fields over the limit are rejected");
    assert_invalid_block(&error, &BlockError::FieldsSizeLimitExceeded);

    // "insert new block"
    // The block of the original has no identifier, and the original stores it under the
    // empty one. Here the empty string is no identifier, so that block is refused, and
    // the subtest goes on with a block that has one.
    let mut without_id = block("", board_id);
    let error = store
        .insert_block(&mut without_id, &user2)
        .await
        .expect_err("a block without an identifier is refused");
    assert_invalid_id(&error, "");
    let blocks = board_blocks(store, board_id).await;
    assert_eq!(
        blocks.len(),
        initial_count + 1,
        "the refused block changed nothing: {blocks:?}"
    );

    let mut new_block = block(&new_id(), board_id);
    store
        .insert_block(&mut new_block, &user2)
        .await
        .expect("a new block is inserted");
    assert_eq!(
        new_block.created_by, user2,
        "the creator of a new block is the inserting user"
    );

    // "update existing block"
    let id2 = new_id();
    let mut old = Block {
        title: "Old Title".to_owned(),
        ..block(&id2, &other_board_id)
    };
    store
        .insert_block(&mut old, &user2)
        .await
        .expect("the block is inserted");
    assert_eq!(
        old.created_by, user2,
        "the creator of a new block is the inserting user"
    );

    let mut new = Block {
        created_by: user3.clone(),
        title: "New Title".to_owned(),
        ..block(&id2, &other_board_id)
    };
    store
        .insert_block(&mut new, &user4)
        .await
        .expect("the block is updated");
    assert_eq!(
        new.created_by, user3,
        "the creator given for an existing block is not altered"
    );
    assert_eq!(new.title, "New Title", "the title is as given");

    // "update existing block with title too large"
    let id3 = new_id();
    let mut existing = Block {
        created_by: user3.clone(),
        title: "New Title".to_owned(),
        ..block(&id3, &other_board_id)
    };
    store
        .insert_block(&mut existing, &user3)
        .await
        .expect("the block is inserted");
    assert_eq!(
        existing.created_by, user3,
        "the creator of a new block is the inserting user"
    );

    let mut long_title = Block {
        created_by: user3.clone(),
        title: "A".repeat(BLOCK_TITLE_MAX_CHARS + 1),
        ..block(&id3, &other_board_id)
    };
    let error = store
        .insert_block(&mut long_title, &user3)
        .await
        .expect_err("an update with a title over the limit is rejected");
    assert_invalid_block(&error, &BlockError::TitleSizeLimitExceeded);

    // "update existing block with aggregated fields size too large"
    let mut existing = Block {
        created_by: user3.clone(),
        title: "New Title".to_owned(),
        ..block(&id3, &other_board_id)
    };
    store
        .insert_block(&mut existing, &user3)
        .await
        .expect("the block is written again");
    assert_eq!(
        existing.created_by, user3,
        "the creator given for an existing block is not altered"
    );

    let mut large_fields = Block {
        created_by: user3.clone(),
        fields: oversized_fields(),
        ..block(&id3, &other_board_id)
    };
    let error = store
        .insert_block(&mut large_fields, &user3)
        .await
        .expect_err("an update with fields over the limit is rejected");
    assert_invalid_block(&error, &BlockError::FieldsSizeLimitExceeded);

    // "data tamper attempt"
    // `01 Jan 90 01:00` and `02 Jan 90 01:00`; the original parses them with a zone
    // abbreviation that its parser reads as UTC.
    let created_at = 631_155_600_000;
    let updated_at = 631_242_000_000;
    let id10 = new_id();
    let mut tampered = Block {
        title: "Old Title".to_owned(),
        create_at: created_at,
        update_at: updated_at,
        created_by: user5,
        modified_by: user6,
        ..block(&id10, &other_board_id)
    };
    store
        .insert_block(&mut tampered, &user1)
        .await
        .expect("the block is inserted");
    let expected_time = now_millis();

    let retrieved = existing_block(store, &id10).await;
    assert_eq!(retrieved.board_id, other_board_id, "the board is as given");
    assert_eq!(
        retrieved.created_by, user1,
        "the creator is the inserting user, not the one given"
    );
    assert_eq!(
        retrieved.modified_by, user1,
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
    let Fixture {
        user_id, board_id, ..
    } = fixture(store).await;

    let initial_count = board_blocks(store, &board_id).await.len();

    // "invalid block"
    let block_id = new_id();
    let mut new_blocks = [block(&block_id, &board_id), block(&block_id, "")];
    let error = store
        .insert_blocks(&mut new_blocks, &user_id)
        .await
        .expect_err("a batch with an invalid block is rejected");
    assert_invalid_block(&error, &BlockError::EmptyBoardId);

    let blocks = board_blocks(store, &board_id).await;
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
    let Fixture { board_id, .. } = fixture(store).await;
    let board_id = board_id.as_str();
    let [user1, user2] = users(store).await;

    let block_id = new_id();
    let mut block = Block {
        title: "oldTitle".to_owned(),
        fields: fields(json!({ "test": "test value", "test2": "test value 2" })),
        ..block(&block_id, board_id)
    };
    store
        .insert_block(&mut block, &user1)
        .await
        .expect("the block is inserted");

    let initial_count = board_blocks(store, board_id).await.len();

    // "not existing block id"
    // An identifier that no block has, and the original's text, which is not even one.
    for missing in [new_id().as_str(), "invalid-block-id"] {
        let error = store
            .patch_block(missing, &BlockPatch::default(), &user1)
            .await
            .expect_err("a missing block cannot be patched");
        assert_not_found(&error);
        assert!(
            error.is_not_found(),
            "the error counts as not found: {error:?}"
        );
    }
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
        .patch_block(&block_id, &title_patch("New title"), &user2)
        .await
        .expect("the title is patched");
    let retrieved = existing_block(store, &block_id).await;
    assert_eq!(
        retrieved.modified_by, user2,
        "the modifier is the patching user"
    );
    assert_eq!(retrieved.title, "New title", "the title is the patched one");

    // "update block custom fields"
    let patch = BlockPatch {
        updated_fields: fields(json!({ "test": "new test value", "test3": "new value" })),
        ..BlockPatch::default()
    };
    store
        .patch_block(&block_id, &patch, &user2)
        .await
        .expect("the fields are patched");
    let retrieved = existing_block(store, &block_id).await;
    assert_eq!(
        retrieved.modified_by, user2,
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
        deleted_fields: strings(["test", "test3", "test100"]),
        ..BlockPatch::default()
    };
    store
        .patch_block(&block_id, &patch, &user2)
        .await
        .expect("the fields are removed");
    let retrieved = existing_block(store, &block_id).await;
    assert_eq!(
        retrieved.modified_by, user2,
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
    let Fixture {
        user_id,
        team_id,
        board_id,
    } = fixture(store).await;
    // The two blocks are on a board each, as in the original.
    let second_board_id = another_board(store, &team_id, &user_id).await;

    let first_id = new_id();
    let second_id = new_id();
    let mut insert_blocks = [
        Block {
            title: "oldTitle".to_owned(),
            ..block(&first_id, &board_id)
        },
        Block {
            title: "oldTitle2".to_owned(),
            ..block(&second_id, &second_board_id)
        },
    ];
    store
        .insert_blocks(&mut insert_blocks, &user_id)
        .await
        .expect("the blocks are inserted");

    // "successful updated existing blocks"
    let title = "updatedTitle";
    let batch = BlockPatchBatch {
        block_ids: strings([&first_id, &second_id]),
        block_patches: vec![title_patch(title), title_patch(title)],
    };
    store
        .patch_blocks(&batch, &user_id)
        .await
        .expect("both blocks are patched");
    let retrieved = existing_block(store, &first_id).await;
    assert_eq!(retrieved.title, title, "the first block has the new title");
    let retrieved2 = existing_block(store, &second_id).await;
    assert_eq!(
        retrieved2.title, title,
        "the second block has the new title"
    );

    // "invalid block id, nothing updated existing blocks"
    // The original skips this on SQLite, where it has no transactions. This store has
    // them on both databases, so it runs on both.
    let title = "Another Title";
    let batch = BlockPatchBatch {
        block_ids: strings([&first_id, &new_id()]),
        block_patches: vec![title_patch(title), title_patch(title)],
    };
    let error = store
        .patch_blocks(&batch, &user_id)
        .await
        .expect_err("a batch that names a missing block fails");
    assert_not_found(&error);
    let retrieved = existing_block(store, &first_id).await;
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
    let Fixture {
        user_id, board_id, ..
    } = fixture(store).await;
    let board_id = board_id.as_str();

    let initial_count = board_blocks(store, board_id).await.len();

    let mut sample = subtree_sample_blocks(board_id);
    common::insert_blocks(store, &mut sample, &user_id).await;

    let blocks = board_blocks(store, board_id).await;
    assert_eq!(
        blocks.len(),
        initial_count + 6,
        "the six sample blocks were added: {blocks:?}"
    );

    // "from root id"
    let blocks = store
        .get_subtree(board_id, id_of(&sample, "parent"), &SubtreeQuery::default())
        .await
        .expect("the subtree of the root is read");
    assert_eq!(
        sorted_names(&blocks),
        ["child1", "child2", "parent"],
        "the root and its two children"
    );

    // "from child id"
    let blocks = store
        .get_subtree(board_id, id_of(&sample, "child1"), &SubtreeQuery::default())
        .await
        .expect("the subtree of a child is read");
    assert_eq!(
        sorted_names(&blocks),
        ["child1", "grandchild1"],
        "the child and its one child"
    );

    // "from not existing id"
    let blocks = store
        .get_subtree(board_id, &new_id(), &SubtreeQuery::default())
        .await
        .expect("the subtree of a missing block is read");
    assert!(
        blocks.is_empty(),
        "a missing block has no subtree: {blocks:?}"
    );

    common::delete_blocks(store, &sample, &user_id).await;
    test.teardown().await;
}

/// `testDeleteBlock`.
#[tokio::test]
async fn delete_block() {
    let test = common::setup().await;
    let store = &test.store;
    let Fixture {
        user_id, board_id, ..
    } = fixture(store).await;
    let user_id = user_id.as_str();
    let [user1] = users(store).await;

    let initial_count = board_blocks(store, &board_id).await.len();

    let mut blocks_to_insert = three_blocks(&board_id);
    common::insert_blocks(store, &mut blocks_to_insert, &user1).await;
    let block1_id = id_of(&blocks_to_insert, "block1");

    let blocks = board_blocks(store, &board_id).await;
    assert_eq!(
        blocks.len(),
        initial_count + 3,
        "the three blocks were added: {blocks:?}"
    );

    // "existing id"
    store
        .delete_block(block1_id, user_id)
        .await
        .expect("an existing block is deleted");

    // "existing id multiple times"
    store
        .delete_block(block1_id, user_id)
        .await
        .expect("deleting a deleted block succeeds");
    store
        .delete_block(block1_id, user_id)
        .await
        .expect("deleting a deleted block succeeds again");

    // "from not existing id"
    store
        .delete_block(&new_id(), user_id)
        .await
        .expect("deleting a block that never existed succeeds");

    common::delete_blocks(store, &blocks_to_insert, user_id).await;
    test.teardown().await;
}

/// `testUndeleteBlock`.
#[tokio::test]
async fn undelete_block() {
    let test = common::setup().await;
    let store = &test.store;
    let Fixture {
        user_id, board_id, ..
    } = fixture(store).await;
    let user_id = user_id.as_str();
    let [user1] = users(store).await;

    let initial_count = board_blocks(store, &board_id).await.len();

    let mut blocks_to_insert = three_blocks(&board_id);
    common::insert_blocks(store, &mut blocks_to_insert, &user1).await;
    let block1_id = id_of(&blocks_to_insert, "block1");

    let blocks = board_blocks(store, &board_id).await;
    assert_eq!(
        blocks.len(),
        initial_count + 3,
        "the three blocks were added: {blocks:?}"
    );

    // "existing id"
    store
        .delete_block(block1_id, user_id)
        .await
        .expect("the block is deleted");
    assert_no_block(store, block1_id).await;

    store
        .undelete_block(block1_id, user_id)
        .await
        .expect("the block is undeleted");
    existing_block(store, block1_id).await;

    // "existing id multiple times"
    store
        .delete_block(block1_id, user_id)
        .await
        .expect("the block is deleted a second time");
    assert_no_block(store, block1_id).await;

    store
        .undelete_block(block1_id, user_id)
        .await
        .expect("the block is undeleted a second time");
    existing_block(store, block1_id).await;

    store
        .undelete_block(block1_id, user_id)
        .await
        .expect("undeleting a block that is not deleted succeeds");
    existing_block(store, block1_id).await;

    // "from not existing id"
    let missing = new_id();
    store
        .undelete_block(&missing, user_id)
        .await
        .expect("undeleting a block that never existed succeeds");
    assert_no_block(store, &missing).await;

    common::delete_blocks(store, &blocks_to_insert, user_id).await;
    test.teardown().await;
}

/// `testGetBlocks`.
#[tokio::test]
async fn get_blocks() {
    let test = common::setup().await;
    let store = &test.store;
    let Fixture {
        user_id, board_id, ..
    } = fixture(store).await;
    let board_id = board_id.as_str();
    let board = || BlockFilter::board(board_id);

    let mut blocks_to_insert = typed_sample_blocks(board_id);
    common::insert_blocks(store, &mut blocks_to_insert, &user_id).await;
    let block1_id = id_of(&blocks_to_insert, "block1");
    let block2_id = id_of(&blocks_to_insert, "block2");
    let block4_id = id_of(&blocks_to_insert, "block4");
    // An identifier that nothing has.
    let missing = new_id();

    // "not existing parent"
    let filter = board()
        .with_parent(&missing)
        .with_type(BlockType::from("test"));
    let blocks = store.get_blocks(&filter).await.expect("the query runs");
    assert!(
        blocks.is_empty(),
        "no block has the missing parent: {blocks:?}"
    );

    // "not existing type"
    let filter = board()
        .with_parent(block1_id)
        .with_type(BlockType::from("not-existing"));
    let blocks = store.get_blocks(&filter).await.expect("the query runs");
    assert!(
        blocks.is_empty(),
        "no block has the missing type: {blocks:?}"
    );

    // "valid parent and type"
    let filter = board()
        .with_parent(block1_id)
        .with_type(BlockType::from("test"));
    let blocks = store.get_blocks(&filter).await.expect("the query runs");
    assert_eq!(
        blocks.len(),
        2,
        "two children of block1 have the type: {blocks:?}"
    );

    // "not existing parent"
    let filter = board().with_parent(&missing);
    let blocks = store.get_blocks(&filter).await.expect("the query runs");
    assert!(
        blocks.is_empty(),
        "no block has the missing parent: {blocks:?}"
    );

    // "valid parent"
    let filter = board().with_parent(block1_id);
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
    // An identifier that no board has, and the original's text, which is not even one.
    for missing_board in [missing.as_str(), "not-exists"] {
        let blocks = board_blocks(store, missing_board).await;
        assert!(
            blocks.is_empty(),
            "the missing board {missing_board} has no blocks: {blocks:?}"
        );
    }

    // "all blocks of the a board"
    let blocks = board_blocks(store, board_id).await;
    assert_eq!(blocks.len(), 5, "the board has all five blocks: {blocks:?}");

    // "several blocks by ids"
    let blocks = store
        .get_blocks_by_ids(&strings([block2_id, block4_id]))
        .await
        .expect("both blocks exist");
    assert_eq!(blocks.len(), 2, "both blocks are returned: {blocks:?}");

    // "blocks by ids where some are not found"
    // The original also hands back the one block it found next to the error
    // (`require.Len(t, blocks, 1)`). A `Result` carries the error alone; neither caller
    // of the original (`server/app/blocks.go:106`, `server/app/boards_and_blocks.go:59`)
    // looks at the blocks when there is an error.
    let error = store
        .get_blocks_by_ids(&strings([block2_id, &missing]))
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
        .get_blocks_by_ids(&strings([&missing, &new_id()]))
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

    common::delete_blocks(store, &blocks_to_insert, &user_id).await;
    test.teardown().await;
}

/// `testGetBlock`.
#[tokio::test]
async fn get_block() {
    let test = common::setup().await;
    let store = &test.store;
    let Fixture { board_id, .. } = fixture(store).await;
    let [user1] = users(store).await;

    // "get a block"
    let block_id = new_id();
    let mut block = Block {
        modified_by: user1.clone(),
        ..block(&block_id, &board_id)
    };
    store
        .insert_block(&mut block, &user1)
        .await
        .expect("the block is inserted");

    let fetched = existing_block(store, &block_id).await;
    assert_eq!(fetched.id, block_id, "the identifier is as inserted");
    assert_eq!(fetched.board_id, board_id, "the board is as inserted");
    assert_eq!(
        fetched.created_by, user1,
        "the creator is the inserting user"
    );
    assert_eq!(
        fetched.modified_by, user1,
        "the modifier is the inserting user"
    );
    assert_within_a_second(now_millis(), fetched.create_at, "create time");
    assert_within_a_second(now_millis(), fetched.update_at, "update time");

    // "get a non-existing block"
    // An identifier that no block has, the original's text, which is not even one, and
    // no text at all.
    for missing in [new_id().as_str(), "non-existing-id", ""] {
        assert_no_block(store, missing).await;
    }

    test.teardown().await;
}

/// `testDuplicateBlock`.
#[tokio::test]
async fn duplicate_block() {
    let test = common::setup().await;
    let store = &test.store;
    let Fixture {
        user_id,
        team_id,
        board_id,
    } = fixture(store).await;
    let board_id = board_id.as_str();
    let user_id = user_id.as_str();
    let [user1] = users(store).await;

    let specs = [
        &SUBTREE_SAMPLE[..],
        &[
            ("grandchild1a", "child1", "comment"),
            ("grandchild2a", "child2", "comment"),
        ],
    ]
    .concat();
    let mut blocks_to_insert = named_blocks(board_id, &specs);
    common::insert_blocks(store, &mut blocks_to_insert, &user1).await;
    let child1_id = id_of(&blocks_to_insert, "child1");

    // "duplicate existing block as no template"
    let blocks = store
        .duplicate_block(board_id, child1_id, user_id, DuplicateAs::Copy)
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
        .duplicate_block(board_id, child1_id, user_id, DuplicateAs::Template)
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
        .duplicate_block(board_id, &new_id(), user_id, DuplicateAs::Copy)
        .await
        .expect_err("a missing block cannot be duplicated");
    assert_not_found(&error);

    // "duplicate not existing board"
    let error = store
        .duplicate_block(&new_id(), &new_id(), user_id, DuplicateAs::Copy)
        .await
        .expect_err("a block of a missing board cannot be duplicated");
    assert_not_found(&error);

    // "not matching board/block"
    // The other board of the original does not exist. Here it does, which is the case
    // that matters: a board that exists does not hand out the blocks of another.
    let other_board_id = another_board(store, &team_id, user_id).await;
    for other in [other_board_id, new_id()] {
        let error = store
            .duplicate_block(&other, child1_id, user_id, DuplicateAs::Copy)
            .await
            .expect_err("a block cannot be duplicated through another board");
        assert_not_found(&error);
    }

    // Like the original, this deletes the six sample blocks and leaves the comments.
    common::delete_blocks(store, &blocks_to_insert[..SUBTREE_SAMPLE.len()], user_id).await;
    test.teardown().await;
}

/// `testGetBlockMetadata`. Despite the name it tests the history of a board's blocks.
#[tokio::test]
#[expect(
    clippy::too_many_lines,
    reason = "one test per test function of the original, whose subtests share a store"
)]
async fn get_block_metadata() {
    let test = common::setup().await;
    let store = &test.store;
    let Fixture {
        user_id, board_id, ..
    } = fixture(store).await;
    let board_id = board_id.as_str();
    let user_id = user_id.as_str();
    let ascending = HistoryQuery::default();
    let descending = HistoryQuery {
        order: HistoryOrder::Descending,
        ..HistoryQuery::default()
    };

    let mut blocks_to_insert = typed_sample_blocks(board_id);
    for block in &mut blocks_to_insert {
        // The original sleeps 20 ms here. Unlike its 1 ms sleeps this one matters to the
        // test: the subtests on `update_at` bounds below need every block to have a later
        // modification time than the one before, and that time is the wall clock in
        // milliseconds.
        tokio::time::sleep(Duration::from_millis(2)).await;
        common::insert_blocks(store, std::slice::from_mut(block), user_id).await;
    }
    let first_id = blocks_to_insert[0].id.as_str();
    let last_id = blocks_to_insert[blocks_to_insert.len() - 1].id.as_str();
    let block3_id = id_of(&blocks_to_insert, "block3");

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
    let third = existing_block(store, block3_id).await;
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
    let third = existing_block(store, block3_id).await;
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
        .delete_block(first_id, user_id)
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
        .undelete_block(first_id, user_id)
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
    // An identifier that no board has, and the original's text, which is not even one.
    for missing_board in [new_id().as_str(), "nonexistent-board-id"] {
        let blocks = board_history(store, missing_board, &HistoryQuery::default()).await;
        assert!(
            blocks.is_empty(),
            "the missing board {missing_board} has no history: {blocks:?}"
        );
    }

    common::delete_blocks(store, &blocks_to_insert, user_id).await;
    test.teardown().await;
}

/// `testUndeleteBlockChildren`.
#[tokio::test]
#[expect(
    clippy::too_many_lines,
    reason = "one test function of the original, whose subtests share their state"
)]
async fn undelete_block_children() {
    let test = common::setup().await;
    let store = &test.store;
    let user_id = common::user(store, "test-user").await;
    let user_id = user_id.as_str();
    let team_id = common::team(store).await;

    let boards = common::create_test_boards(store, &team_id, user_id, 2).await;
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
        .delete_block(&deleted_card.id, user_id)
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
        .undelete_block(&deleted_card.id, user_id)
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
        .delete_board(&board_delete_id, user_id)
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
        .undelete_board(&board_delete_id, user_id)
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

/// One task of [`concurrent_writers_on_one_board_all_succeed`].
struct Writer {
    store: Store,
    task: usize,
    /// The user the task writes as.
    user_id: String,
    board_id: String,
    /// The block that every task patches.
    shared_id: String,
    /// The block the task inserts in each of its rounds.
    own_ids: Vec<String>,
}

impl Writer {
    /// Per round, four writes and one read. On failure, says which operation failed.
    async fn write_rounds(self) -> Result<(), String> {
        let Self {
            store,
            task,
            user_id,
            board_id,
            shared_id,
            own_ids,
        } = self;
        let failed = |round: usize, operation: &str, error: StoreError| {
            format!("task {task}, round {round}: {operation} failed: {error:?}")
        };

        for (round, own_id) in own_ids.iter().enumerate() {
            let mut own = Block {
                parent_id: shared_id.clone(),
                block_type: BlockType::Text,
                ..block(own_id, &board_id)
            };
            store
                .insert_block(&mut own, &user_id)
                .await
                .map_err(|error| failed(round, "insert", error))?;
            store
                .patch_block(own_id, &title_patch("patched"), &user_id)
                .await
                .map_err(|error| failed(round, "patch", error))?;
            let shared_title = format!("task {task}, round {round}");
            store
                .patch_block(&shared_id, &title_patch(&shared_title), &user_id)
                .await
                .map_err(|error| failed(round, "patch of the shared block", error))?;
            store
                .get_subtree(&board_id, &shared_id, &SubtreeQuery::default())
                .await
                .map_err(|error| failed(round, "read", error))?;
            if round % 2 == 0 {
                store
                    .delete_block(own_id, &user_id)
                    .await
                    .map_err(|error| failed(round, "delete", error))?;
            } else {
                store
                    .patch_block(own_id, &title_patch("kept"), &user_id)
                    .await
                    .map_err(|error| failed(round, "second patch", error))?;
            }
        }
        Ok(())
    }
}

/// Checks the blocks one task of [`concurrent_writers_on_one_board_all_succeed`] wrote
/// against the blocks and the history of the board afterwards: every block has three
/// records, and its last one says whether the block was deleted.
fn assert_blocks_of_a_writer_add_up(
    task: usize,
    own_ids: &[String],
    blocks: &[Block],
    history: &[Block],
) {
    for (round, own_id) in own_ids.iter().enumerate() {
        let own_history: Vec<&Block> = history.iter().filter(|block| &block.id == own_id).collect();
        assert_eq!(
            own_history.len(),
            3,
            "the block of task {task}, round {round} was written three times: {own_history:?}"
        );
        let deleted = own_history[2].delete_at != 0;
        assert_eq!(
            deleted,
            round % 2 == 0,
            "the newest record of the block of task {task}, round {round} says whether it \
             was deleted"
        );
        assert_eq!(
            common::contains_block_with_id(blocks, own_id),
            !deleted,
            "the block of task {task}, round {round} exists unless it was deleted"
        );
    }
}

/// Many tasks insert, patch and delete blocks of one board through clones of one store
/// at the same time. Every operation must succeed, and afterwards the blocks and the
/// history must add up.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_writers_on_one_board_all_succeed() {
    const TASKS: usize = 16;
    /// Four writes per round, so twenty writes per task.
    const ROUNDS: usize = 5;
    /// The rounds of a task that end by deleting the block they inserted.
    const DELETING_ROUNDS: usize = ROUNDS.div_ceil(2);

    let test = common::setup().await;
    let store = &test.store;
    let Fixture {
        user_id, board_id, ..
    } = fixture(store).await;
    let board_id = board_id.as_str();

    // One block that every task patches, so that writes to the same block race too.
    let shared_id = new_id();
    let mut shared = Block {
        block_type: BlockType::Card,
        ..block(&shared_id, board_id)
    };
    store
        .insert_block(&mut shared, &user_id)
        .await
        .expect("the shared block is inserted");

    // Every task writes as a user of its own, and its blocks have their identifiers
    // before it starts, so that the test can look for them afterwards.
    let mut writer_ids = Vec::with_capacity(TASKS);
    for task in 0..TASKS {
        writer_ids.push(common::user(store, &format!("writer-{task}")).await);
    }
    let own_ids: Vec<Vec<String>> = (0..TASKS)
        .map(|_| (0..ROUNDS).map(|_| new_id()).collect())
        .collect();

    let mut writers = JoinSet::new();
    for (task, (user_id, own_ids)) in writer_ids.into_iter().zip(&own_ids).enumerate() {
        let writer = Writer {
            store: store.clone(),
            task,
            user_id,
            board_id: board_id.to_owned(),
            shared_id: shared_id.clone(),
            own_ids: own_ids.clone(),
        };
        writers.spawn(writer.write_rounds());
    }
    while let Some(joined) = writers.join_next().await {
        let outcome = joined.expect("a writer task runs to its end");
        assert_eq!(outcome, Ok(()), "every operation of every task succeeds");
    }

    // The blocks: the shared one and, per task, those of the rounds that did not delete.
    let blocks = board_blocks(store, board_id).await;
    assert_eq!(
        blocks.len(),
        1 + TASKS * (ROUNDS - DELETING_ROUNDS),
        "the shared block and the surviving blocks of every task: {:?}",
        common::sorted_ids(&blocks)
    );
    for block in blocks.iter().filter(|block| block.id != shared_id) {
        assert_eq!(
            block.title, "kept",
            "block {} survives with its last patch",
            block.id
        );
    }

    // The history: one record per write.
    let history = board_history(store, board_id, &HistoryQuery::default()).await;
    assert_eq!(
        history.len(),
        1 + TASKS * ROUNDS * 4,
        "one history record for the shared block's insert and one per write of every task"
    );
    let shared_history = store
        .get_block_history(&shared_id, &HistoryQuery::default())
        .await
        .expect("the history of the shared block is read");
    assert_eq!(
        shared_history.len(),
        1 + TASKS * ROUNDS,
        "one history record per patch of the shared block, after the one for its insert"
    );

    // The shared block is in the state of its newest history record.
    let shared = existing_block(store, &shared_id).await;
    let newest = shared_history
        .last()
        .expect("the shared block has a history");
    assert_eq!(
        (&shared.title, &shared.modified_by, shared.update_at),
        (&newest.title, &newest.modified_by, newest.update_at),
        "the shared block matches its newest history record"
    );

    for (task, own_ids) in own_ids.iter().enumerate() {
        assert_blocks_of_a_writer_add_up(task, own_ids, &blocks, &history);
    }

    test.teardown().await;
}

/// Writes one block many times in quick succession, each write in a transaction of its
/// own, and checks that every write got a history record, in the order of the writes.
async fn assert_rapid_writes_get_a_history_record_each(store: &Store) {
    const UPDATES: usize = 200;

    let Fixture {
        user_id, board_id, ..
    } = fixture(store).await;
    let user_id = user_id.as_str();

    let rapid_id = new_id();
    let mut rapid = Block {
        title: "write 0".to_owned(),
        ..block(&rapid_id, &board_id)
    };
    store
        .insert_block(&mut rapid, user_id)
        .await
        .expect("the block is inserted");

    // Updates through both ways of writing a block.
    for write in 1..=UPDATES {
        let title = format!("write {write}");
        if write % 2 == 0 {
            rapid.title = title;
            store
                .insert_block(&mut rapid, user_id)
                .await
                .unwrap_or_else(|error| panic!("update {write} should succeed: {error:?}"));
        } else {
            store
                .patch_block(&rapid_id, &title_patch(&title), user_id)
                .await
                .unwrap_or_else(|error| panic!("patch {write} should succeed: {error:?}"));
        }
    }

    let history = store
        .get_block_history(&rapid_id, &HistoryQuery::default())
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
            .delete_block(&rapid_id, user_id)
            .await
            .unwrap_or_else(|error| panic!("delete {round} should succeed: {error:?}"));
        store
            .undelete_block(&rapid_id, user_id)
            .await
            .unwrap_or_else(|error| panic!("undelete {round} should succeed: {error:?}"));
    }
    let history = store
        .get_block_history(&rapid_id, &HistoryQuery::default())
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
    let store = Store::connect(DbType::Sqlite, "file::memory:")
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
    let Fixture {
        user_id, board_id, ..
    } = fixture(store).await;

    let batched_id = new_id();
    let mut batch: Vec<Block> = (0..WRITES)
        .map(|write| Block {
            title: format!("write {write}"),
            ..block(&batched_id, &board_id)
        })
        .collect();
    store
        .insert_blocks(&mut batch, &user_id)
        .await
        .expect("a batch may write one block many times");

    let history = store
        .get_block_history(&batched_id, &HistoryQuery::default())
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
    let stored = existing_block(store, &batched_id).await;
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
    let Fixture {
        user_id, board_id, ..
    } = fixture(store).await;

    let mut sample = typed_sample_blocks(&board_id);
    common::insert_blocks(store, &mut sample, &user_id).await;
    let block1_id = id_of(&sample, "block1");

    let all = common::sorted_ids(&sample);
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

    // An identifier that no block has, and text that is not even one.
    for missing in [new_id().as_str(), "missing"] {
        let asked = strings([block1_id, missing]);
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
    }

    // Preserved quirk: the same identifier twice finds one block for two identifiers.
    let error = store
        .get_blocks_by_ids(&strings([block1_id, block1_id]))
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
    let Fixture {
        user_id, board_id, ..
    } = fixture(store).await;

    let cards = create_test_cards(store, &user_id, &board_id, 5).await;
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
    let Fixture {
        user_id, board_id, ..
    } = fixture(store).await;

    let specs = [
        &TYPED_SAMPLE[..],
        &[("typeless", "", ""), ("unknown-typed", "", "unknown")],
    ]
    .concat();
    let mut sample = named_blocks(&board_id, &specs);
    common::insert_blocks(store, &mut sample, &user_id).await;

    for block_type in ["", "unknown"] {
        let filter = BlockFilter::board(&board_id).with_type(BlockType::from(block_type));
        let blocks = store.get_blocks(&filter).await.expect("the query runs");
        assert_eq!(
            blocks.len(),
            7,
            "the type {block_type:?} selects every block of the board: {:?}",
            sorted_names(&blocks)
        );

        let filter = filter.with_parent(id_of(&sample, "block1"));
        let blocks = store.get_blocks(&filter).await.expect("the query runs");
        assert_eq!(
            sorted_names(&blocks),
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
    let Fixture {
        user_id, board_id, ..
    } = fixture(store).await;
    let board_id = board_id.as_str();
    let user_id = user_id.as_str();

    let first_id = new_id();
    let second_id = new_id();
    let mut blocks = [
        Block {
            title: "first".to_owned(),
            ..block(&first_id, board_id)
        },
        Block {
            title: "second".to_owned(),
            ..block(&second_id, board_id)
        },
    ];
    store
        .insert_blocks(&mut blocks, user_id)
        .await
        .expect("the blocks are inserted");

    let batch = BlockPatchBatch {
        block_ids: strings([&first_id, &second_id]),
        block_patches: vec![title_patch("patched")],
    };
    let error = store
        .patch_blocks(&batch, user_id)
        .await
        .expect_err("the second identifier has no patch");
    assert!(
        matches!(
            error,
            StoreError::InvalidBoardsAndBlocks(BoardsAndBlocksError::BlockIdsAndPatchesMismatch)
        ),
        "expected a mismatch error, got {error:?}"
    );

    let first = existing_block(store, &first_id).await;
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
        block_ids: strings([&first_id]),
        block_patches: vec![title_patch("patched"), title_patch("ignored")],
    };
    store
        .patch_blocks(&batch, user_id)
        .await
        .expect("surplus patches are ignored");
    let first = existing_block(store, &first_id).await;
    assert_eq!(first.title, "patched", "the first block is patched");
    let second = existing_block(store, &second_id).await;
    assert_eq!(second.title, "second", "the second block is untouched");

    test.teardown().await;
}

/// Deleting a block that does not exist succeeds and writes nothing.
#[tokio::test]
async fn delete_block_of_a_missing_block_writes_nothing() {
    let test = common::setup().await;
    let store = &test.store;
    let Fixture {
        user_id, board_id, ..
    } = fixture(store).await;

    let mut sample = three_blocks(&board_id);
    common::insert_blocks(store, &mut sample, &user_id).await;

    let missing = new_id();
    store
        .delete_block(&missing, &user_id)
        .await
        .expect("deleting a missing block succeeds");

    let blocks = board_blocks(store, &board_id).await;
    assert_eq!(
        sorted_names(&blocks),
        ["block1", "block2", "block3"],
        "the blocks that exist are untouched"
    );
    let history = board_history(store, &board_id, &HistoryQuery::default()).await;
    assert_eq!(
        history.len(),
        3,
        "only the three inserts are in the history: {history:?}"
    );
    let missing_history = store
        .get_block_history(&missing, &HistoryQuery::default())
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
    let Fixture { board_id, .. } = fixture(store).await;
    let creator_id = common::user(store, "creator").await;
    let someone_else_id = common::user(store, "someone-else").await;

    // A deleted child, which undeleting the parent would restore if it did anything.
    let mut sample = named_blocks(
        &board_id,
        &[("parent", "", ""), ("child", "parent", "text")],
    );
    common::insert_blocks(store, &mut sample, &creator_id).await;
    let parent_id = id_of(&sample, "parent");
    let child_id = id_of(&sample, "child");
    store
        .delete_block(child_id, &creator_id)
        .await
        .expect("the child is deleted");
    let before = existing_block(store, parent_id).await;

    store
        .undelete_block(parent_id, &someone_else_id)
        .await
        .expect("undeleting a block that is not deleted succeeds");

    let after = existing_block(store, parent_id).await;
    assert_eq!(after, before, "the block is untouched");
    assert_no_block(store, child_id).await;
    let history = board_history(store, &board_id, &HistoryQuery::default()).await;
    assert_eq!(
        history.len(),
        3,
        "two inserts and one delete, and nothing for the undelete: {history:?}"
    );

    test.teardown().await;
}

/// The original lets a block have the empty identifier, and deleting or undeleting that
/// block takes every block of its board along, not only its children
/// (`server/services/store/sqlstore/blocks.go:450`, `:522`, `:929`, `:1017`). Here the
/// empty string is no identifier: no block is stored under it, and deleting or
/// undeleting it touches nothing.
#[tokio::test]
async fn the_empty_id_names_no_block_and_deleting_it_leaves_its_board_alone() {
    let test = common::setup().await;
    let store = &test.store;
    let Fixture {
        user_id, board_id, ..
    } = fixture(store).await;
    let user_id = user_id.as_str();

    let mut sample = subtree_sample_blocks(&board_id);
    common::insert_blocks(store, &mut sample, user_id).await;

    let mut without_id = block("", &board_id);
    let error = store
        .insert_block(&mut without_id, user_id)
        .await
        .expect_err("a block is not stored under the empty identifier");
    assert_invalid_id(&error, "");
    assert_no_block(store, "").await;

    store
        .delete_block("", user_id)
        .await
        .expect("deleting the block with the empty identifier succeeds");
    let blocks = board_blocks(store, &board_id).await;
    assert_eq!(
        common::sorted_ids(&blocks),
        common::sorted_ids(&sample),
        "every block of the board is still there"
    );

    store
        .undelete_block("", user_id)
        .await
        .expect("undeleting the block with the empty identifier succeeds");
    let blocks = board_blocks(store, &board_id).await;
    assert_eq!(
        common::sorted_ids(&blocks),
        common::sorted_ids(&sample),
        "every block of the board is there once"
    );
    let history = board_history(store, &board_id, &HistoryQuery::default()).await;
    assert_eq!(
        history.len(),
        sample.len(),
        "only the inserts of the sample are in the history: {history:?}"
    );

    test.teardown().await;
}

/// Preserved quirk: writing a block that exists under another board leaves the block as
/// it is and still adds a history record (`server/services/store/sqlstore/blocks.go:283`).
#[tokio::test]
async fn insert_block_under_another_board_writes_only_history() {
    let test = common::setup().await;
    let store = &test.store;
    let team_id = common::team(store).await;
    let user_a = common::user(store, "user-a").await;
    let user_b = common::user(store, "user-b").await;
    let board_a = another_board(store, &team_id, &user_a).await;
    let board_b = another_board(store, &team_id, &user_b).await;

    let moving_id = new_id();
    let mut original = Block {
        title: "original".to_owned(),
        ..block(&moving_id, &board_a)
    };
    store
        .insert_block(&mut original, &user_a)
        .await
        .expect("the block is inserted");
    let before = existing_block(store, &moving_id).await;

    let mut moved = Block {
        title: "moved".to_owned(),
        ..block(&moving_id, &board_b)
    };
    store
        .insert_block(&mut moved, &user_b)
        .await
        .expect("writing the block under another board succeeds");

    let after = existing_block(store, &moving_id).await;
    assert_eq!(after, before, "the block is untouched");
    let blocks = board_blocks(store, &board_b).await;
    assert!(
        blocks.is_empty(),
        "the other board did not get the block: {blocks:?}"
    );
    let history = store
        .get_block_history(&moving_id, &HistoryQuery::default())
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
            (board_a.as_str(), "original", user_a.as_str()),
            (board_b.as_str(), "moved", user_b.as_str())
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
    let Fixture { board_id, .. } = fixture(store).await;
    let creator_id = common::user(store, "creator").await;
    let updater_id = common::user(store, "updater").await;

    let block_id = new_id();
    let mut block = block(&block_id, &board_id);
    store
        .insert_block(&mut block, &creator_id)
        .await
        .expect("the block is inserted");
    let created = existing_block(store, &block_id).await;

    // A later millisecond, so that the two times can be told apart.
    tokio::time::sleep(Duration::from_millis(2)).await;
    store
        .patch_block(&block_id, &title_patch("new title"), &updater_id)
        .await
        .expect("the block is updated");

    let updated = existing_block(store, &block_id).await;
    assert_eq!(
        (updated.created_by.as_str(), updated.create_at),
        (creator_id.as_str(), created.create_at),
        "the block keeps its creator and creation time"
    );
    assert_eq!(
        updated.modified_by, updater_id,
        "the block has a new modifier"
    );
    assert!(
        updated.update_at > created.update_at,
        "the block has a later modification time: {} after {}",
        updated.update_at,
        created.update_at
    );

    let history = store
        .get_block_history(&block_id, &HistoryQuery::default())
        .await
        .expect("the history is read");
    assert_eq!(history.len(), 2, "an insert and an update: {history:?}");
    let record = &history[1];
    assert_eq!(
        (record.created_by.as_str(), record.modified_by.as_str()),
        (updater_id.as_str(), updater_id.as_str()),
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
/// file as deleted: the record its board has under the name in the block. The original
/// fails to delete such blocks when it runs standalone, because it updates a table that
/// only its host application has (`server/services/store/sqlstore/blocks.go:429`, `:986`).
#[tokio::test]
async fn deleting_blocks_that_hold_files_marks_the_files_deleted() {
    let test = common::setup().await;
    let store = &test.store;
    let Fixture {
        user_id,
        team_id,
        board_id,
    } = fixture(store).await;
    let board_id = board_id.as_str();
    let user_id = user_id.as_str();

    // A block names its file by the name the file is stored under.
    for stored_name in ["direct.png", "image.png", "attachment.pdf", "untouched.png"] {
        save_file(store, board_id, stored_name).await;
    }
    // Another board has a file under the name of one that is deleted here.
    let other_board_id = another_board(store, &team_id, user_id).await;
    save_file(store, &other_board_id, "direct.png").await;

    let cards = create_test_cards(store, user_id, board_id, 2).await;
    let mut blocks: Vec<Block> = [
        (0, "image", "fileId", "direct.png"),
        (1, "image", "fileId", "image.png"),
        (1, "attachment", "attachmentId", "attachment.pdf"),
        (0, "image", "fileId", "untouched.png"),
    ]
    .into_iter()
    .map(|(card, block_type, field, stored_name)| Block {
        parent_id: cards[card].id.clone(),
        block_type: BlockType::from(block_type),
        fields: fields(json!({ field: stored_name })),
        ..block(&new_id(), board_id)
    })
    .collect();
    common::insert_blocks(store, &mut blocks, user_id).await;
    let direct_id = blocks[0].id.as_str();
    let kept_id = blocks[3].id.as_str();

    // An image on its own, then a card with an image and an attachment.
    store
        .delete_block(direct_id, user_id)
        .await
        .expect("an image block is deleted");
    store
        .delete_block(&cards[1].id, user_id)
        .await
        .expect("a card that holds files is deleted");

    let left = board_blocks(store, board_id).await;
    let mut expected_left = strings([&cards[0].id, kept_id]);
    expected_left.sort();
    assert_eq!(
        common::sorted_ids(&left),
        expected_left,
        "the first card and its remaining image are left"
    );

    for (stored_name, deleted) in [
        ("direct.png", true),
        ("image.png", true),
        ("attachment.pdf", true),
        ("untouched.png", false),
    ] {
        assert_eq!(
            file_is_deleted(store, board_id, stored_name).await,
            deleted,
            "the files of the deleted blocks, and only those, are marked deleted: \
             {stored_name}"
        );
    }
    assert!(
        !file_is_deleted(store, &other_board_id, "direct.png").await,
        "the file another board has under the same name is not marked deleted"
    );

    test.teardown().await;
}

/// The bounds of a subtree query include the boundary and combine with a limit.
#[tokio::test]
async fn get_subtree_applies_bounds_and_limit() {
    let test = common::setup().await;
    let store = &test.store;
    let Fixture {
        user_id, board_id, ..
    } = fixture(store).await;

    let mut sample = subtree_sample_blocks(&board_id);
    for block in &mut sample[..3] {
        // The bounds are on the modification time, which is the wall clock in
        // milliseconds; the three blocks of the subtree need three different ones.
        tokio::time::sleep(Duration::from_millis(2)).await;
        common::insert_blocks(store, std::slice::from_mut(block), &user_id).await;
    }
    let child1_update_at = sample[1].update_at;
    let parent_id = id_of(&sample, "parent");
    let subtree_names = async |query: SubtreeQuery| -> Vec<String> {
        let blocks = store
            .get_subtree(&board_id, parent_id, &query)
            .await
            .expect("the subtree is read");
        blocks.into_iter().map(|block| block.title).collect()
    };

    assert_eq!(
        subtree_names(SubtreeQuery::default()).await,
        ["parent", "child1", "child2"],
        "the whole subtree, oldest first"
    );
    let query = SubtreeQuery {
        before_update_at: Some(child1_update_at),
        ..SubtreeQuery::default()
    };
    assert_eq!(
        subtree_names(query).await,
        ["parent", "child1"],
        "blocks updated at or before the bound"
    );
    let query = SubtreeQuery {
        after_update_at: Some(child1_update_at),
        ..SubtreeQuery::default()
    };
    assert_eq!(
        subtree_names(query).await,
        ["child1", "child2"],
        "blocks updated at or after the bound"
    );
    let query = SubtreeQuery {
        limit: Some(1),
        ..query
    };
    assert_eq!(
        subtree_names(query).await,
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
    let Fixture {
        user_id,
        team_id,
        board_id,
    } = fixture(store).await;

    let counts = store
        .get_block_counts_by_type()
        .await
        .expect("the counts are read");
    assert!(
        counts.is_empty(),
        "a store without blocks has no counts: {counts:?}"
    );

    let mut sample = typed_sample_blocks(&board_id);
    common::insert_blocks(store, &mut sample, &user_id).await;
    let other_board_id = another_board(store, &team_id, &user_id).await;
    let mut other_board = Block {
        block_type: BlockType::from("test2"),
        ..block(&new_id(), &other_board_id)
    };
    store
        .insert_block(&mut other_board, &user_id)
        .await
        .expect("a block of another board is inserted");
    store
        .delete_block(id_of(&sample, "block5"), &user_id)
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
/// store writes text. Fields that reached a database that way must still be readable.
#[tokio::test]
async fn sqlite_fields_stored_as_a_blob_are_read() {
    if common::db_type() != DbType::Sqlite {
        // PostgreSQL has a JSON column there, which the original writes like this store.
        return;
    }
    let test = common::setup().await;
    let store = &test.store;
    let Fixture {
        user_id, board_id, ..
    } = fixture(store).await;
    let user_id = user_id.as_str();

    let card_id = new_id();
    let mut card = Block {
        block_type: BlockType::Card,
        fields: fields(json!({ "icon": "🎨", "contentOrder": ["a", ["b", "c"]] })),
        ..block(&card_id, &board_id)
    };
    store
        .insert_block(&mut card, user_id)
        .await
        .expect("the card is inserted");
    store
        .delete_block(&card_id, user_id)
        .await
        .expect("the card is deleted");
    store
        .undelete_block(&card_id, user_id)
        .await
        .expect("the card is undeleted");

    // Rewrite the fields the way the original's driver stores them.
    let mut raw = sqlite_connection(&test).await;
    for table in ["blocks", "blocks_history"] {
        let rewritten = sqlx::query(sqlx::AssertSqlSafe(format!(
            "UPDATE {table} SET fields = CAST(fields AS BLOB)"
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

    let stored = existing_block(store, &card_id).await;
    assert_eq!(
        stored.fields, card.fields,
        "the fields of a block are read from a blob"
    );
    let history = store
        .get_block_history(&card_id, &HistoryQuery::default())
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
        .delete_block(&card_id, user_id)
        .await
        .expect("a block with blob fields is deleted");
    store
        .undelete_block(&card_id, user_id)
        .await
        .expect("a block with blob fields is undeleted");
    let restored = existing_block(store, &card_id).await;
    assert_eq!(
        restored.fields, card.fields,
        "the fields survive a delete and an undelete"
    );

    test.teardown().await;
}

/// Not in the original: a block is not stored with `properties` that are no well-formed
/// card property values, whether it is inserted or patched, and the stored block stays
/// as it was.
#[tokio::test]
async fn blocks_with_malformed_properties_are_not_stored() {
    let test = common::setup().await;
    let store = &test.store;
    let Fixture {
        user_id, board_id, ..
    } = fixture(store).await;
    let user_id = user_id.as_str();
    let card_with = |id: &str, properties: Value| {
        let mut card = Block {
            block_type: BlockType::Card,
            ..block(id, &board_id)
        };
        card.fields.insert("properties".to_owned(), properties);
        card
    };

    for (what, properties) in [
        ("a number", json!({"p1": 3})),
        ("an object", json!({"p1": {"nested": true}})),
        ("a string", json!("no object")),
    ] {
        let id = new_id();
        let error = store
            .insert_block(&mut card_with(&id, properties.clone()), user_id)
            .await
            .expect_err("malformed properties");
        assert!(
            matches!(
                error,
                StoreError::InvalidBlock {
                    source: BlockError::InvalidCardProperties(_),
                    ..
                }
            ),
            "properties {properties}: got {error:?}"
        );
        assert!(
            store.get_block(&id).await.unwrap_err().is_not_found(),
            "{what}: nothing was stored"
        );
    }

    let fine_id = new_id();
    let well_formed = json!({"p1": "open", "p2": ["a", "b"], "p3": null});
    store
        .insert_block(&mut card_with(&fine_id, well_formed.clone()), user_id)
        .await
        .expect("well-formed properties are stored");
    let mut updated_fields = JsonMap::new();
    updated_fields.insert("properties".to_owned(), json!({"p1": 3}));
    let patch = BlockPatch {
        title: Some("renamed".to_owned()),
        updated_fields,
        ..BlockPatch::default()
    };
    store
        .patch_block(&fine_id, &patch, user_id)
        .await
        .expect_err("a patch to malformed properties");
    let stored = store.get_block(&fine_id).await.unwrap();
    assert_eq!(stored.fields["properties"], well_formed, "unchanged");
    assert_eq!(stored.title, "", "and so is the rest of the block");

    test.teardown().await;
}
