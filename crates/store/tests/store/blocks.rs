//! Port of `server/services/store/storetests/blocks.go`.
//!
//! There is one test per test function of the original, because the subtests of one
//! function share a store and run in order. Each subtest is a section headed by its name
//! in quotes. The original keeps a history of every block and sleeps a millisecond between
//! writes so that two history rows of one block cannot collide. This store keeps no
//! history: its tables hold what exists and a deleted block is in the trash. So the test
//! of the history (`testGetBlockMetadata`), the test of the subtree read that goes
//! through it (`testGetSubTree2`) and those sleeps are not ported.
//!
//! The original names its users, boards and blocks with made-up text such as `user-id-1`,
//! `board-id` and `block1`, and none of them has to exist. Here an identifier is a UUID,
//! and what a block refers to (its board, the users who write it) is stored before the
//! block. So every test first stores its users, its team and its boards, and the sample
//! blocks carry the names the original gives them as their titles.
//!
//! After the ported suite come tests the original does not have: concurrent writers,
//! behavior of the block operations that its suite leaves uncovered, what this store
//! does differently (the trash, parents that must be blocks of the same board, the order
//! of lists), and callers that delete and restore the same blocks at the same time.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use fvoci_model::{
    BLOCK_FIELDS_MAX_CHARS, BLOCK_TITLE_MAX_CHARS, BODY_VERSION, Block, BlockError, BlockPatch,
    BlockPatchBatch, BlockType, BoardsAndBlocksError, Body, BodyChange, FileInfo, JsonMap,
    SubscriberType, Subscription, new_id,
};
use fvoci_store::{BlockFilter, DbType, DeletedBlock, DuplicateAs, Paging, Store, StoreError};
use serde_json::{Value, json};
use sqlx::Connection as _;
use sqlx::sqlite::SqliteConnection;
use tokio::sync::Barrier;
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

/// Stores the record of a file that a board has under the given stored name, and
/// returns it.
async fn save_file(store: &Store, board_id: &str, stored_name: &str) -> FileInfo {
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
    file_info
}

/// Asserts that every one of the file records is stored as it was saved.
async fn assert_file_records_are(store: &Store, files: &[FileInfo], when: &str) {
    for file in files {
        let stored = store
            .get_file_info(&file.board_id, &file.stored_name)
            .await
            .unwrap_or_else(|error| {
                panic!(
                    "file {} of board {} should have a record {when}: {error:?}",
                    file.stored_name, file.board_id
                )
            });
        assert_eq!(
            &stored, file,
            "the record of file {} is as it was saved, {when}",
            file.stored_name
        );
    }
}

/// `get_deleted_block` followed by a check that the block is in the trash.
async fn block_in_trash(store: &Store, block_id: &str) -> DeletedBlock {
    store
        .get_deleted_block(block_id)
        .await
        .unwrap_or_else(|error| panic!("block {block_id} should be in the trash: {error:?}"))
}

/// Asserts that no block of the identifier is in the trash.
async fn assert_not_in_trash(store: &Store, block_id: &str) {
    match store.get_deleted_block(block_id).await {
        Ok(deleted) => panic!("block {block_id} should not be in the trash, got {deleted:?}"),
        Err(error) => assert_not_found(&error),
    }
}

/// `undelete_block` followed by a check that it found nothing to restore.
async fn assert_undelete_finds_nothing(store: &Store, block_id: &str, user_id: &str) {
    match store.undelete_block(block_id, user_id).await {
        Ok(block) => panic!("block {block_id:?} should not be restored, got {block:?}"),
        Err(error) => assert_not_found(&error),
    }
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
    // Not in the original: the stored block keeps the creator it has.
    let stored = existing_block(store, &id2).await;
    assert_eq!(
        (stored.created_by.as_str(), stored.title.as_str()),
        (user2.as_str(), "New Title"),
        "an update changes the title and not the stored creator"
    );

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
    // Not in the original: the argument says what was stored.
    assert_eq!(
        (tampered.create_at, tampered.update_at),
        (retrieved.create_at, retrieved.update_at),
        "the argument has the times that were stored, not the ones given"
    );

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

/// A block's `update_at` moves on with every change, even when the clock has not: a
/// patch made against the time a block was read at must see every change after it.
#[tokio::test]
async fn update_at_moves_on_with_every_change() {
    let test = common::setup().await;
    let store = &test.store;
    let Fixture { board_id, .. } = fixture(store).await;
    let [user_id] = users(store).await;
    let block_id = new_id();
    let mut block = block(&block_id, &board_id);
    store
        .insert_block(&mut block, &user_id)
        .await
        .expect("the block is inserted");

    // A time an hour ahead of the clock, as a block written by a clock that ran fast.
    let ahead = existing_block(store, &block_id).await.update_at + 3_600_000;
    common::execute_raw(
        &test.dbconfig(),
        &format!("UPDATE blocks SET update_at = {ahead} WHERE id = '{block_id}'"),
    )
    .await;
    store
        .patch_block(&block_id, &title_patch("once"), &user_id)
        .await
        .expect("the block is patched");
    let once = existing_block(store, &block_id).await.update_at;
    assert_eq!(
        once,
        ahead + 1,
        "the patch moved update_at past the stored one"
    );

    store
        .patch_block(&block_id, &title_patch("twice"), &user_id)
        .await
        .expect("the block is patched again");
    let twice = existing_block(store, &block_id).await.update_at;
    assert_eq!(twice, once + 1, "a second patch moved it on again");

    test.teardown().await;
}

/// The store keeps beside a body the version of the allowlist it was checked against
/// and its words for searching, and clears both with the body.
#[tokio::test]
async fn a_body_is_stored_with_its_version_and_text() {
    let test = common::setup().await;
    let store = &test.store;
    let Fixture { board_id, .. } = fixture(store).await;
    let [user_id] = users(store).await;
    let document = json!({"type": "doc", "content": [
        {"type": "heading", "attrs": {"level": 1}, "content": [{"type": "text", "text": "Plan"}]},
        {"type": "paragraph", "content": [
            {"type": "text", "text": "x is "}, {"type": "inlineMath", "attrs": {"latex": "a^2"}},
        ]},
        {"type": "blockMath", "attrs": {"latex": "b"}},
    ]});
    let block_id = new_id();
    let mut card = Block {
        block_type: BlockType::Card,
        body: Body::parse(document).expect("the body is allowed"),
        ..block(&block_id, &board_id)
    };
    store
        .insert_block(&mut card, &user_id)
        .await
        .expect("the card is inserted");

    // What the store has no reading for, read through the title.
    let derived = async || {
        common::execute_raw(
            &test.dbconfig(),
            &format!(
                "UPDATE blocks SET title = COALESCE(CAST(body_version AS TEXT), '-') || '|' || \
                 COALESCE(body_text, '-') WHERE id = '{block_id}'"
            ),
        )
        .await;
        existing_block(store, &block_id).await.title
    };
    assert_eq!(derived().await, format!("{BODY_VERSION}|Plan\nx is a^2\nb"));

    let clear = BlockPatch {
        body: BodyChange::Clear,
        ..BlockPatch::default()
    };
    store
        .patch_block(&block_id, &clear, &user_id)
        .await
        .expect("the body is cleared");
    assert_eq!(derived().await, "-|-");

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
    // Not in the original: the block is in the trash now, and the other two are not.
    assert_no_block(store, block1_id).await;
    let deleted = block_in_trash(store, block1_id).await;
    assert_eq!(
        deleted.deleted_by, user_id,
        "the trash names who deleted the block"
    );
    let blocks = board_blocks(store, &board_id).await;
    assert_eq!(
        sorted_names(&blocks),
        ["block2", "block3"],
        "the other blocks are still there"
    );

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
    assert_eq!(
        block_in_trash(store, block1_id).await,
        deleted,
        "deleting the block again left what is in the trash as it was"
    );

    common::delete_blocks(store, &blocks_to_insert, user_id).await;
    test.teardown().await;
}

/// `testUndeleteBlock`.
///
/// The original succeeds without doing anything when it is asked to undelete a block
/// that is not deleted or that never existed. Here that is a not-found error: there is
/// no such block in the trash.
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

    assert_undelete_finds_nothing(store, block1_id, user_id).await;
    existing_block(store, block1_id).await;

    // "from not existing id"
    let missing = new_id();
    assert_undelete_finds_nothing(store, &missing, user_id).await;
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
                .get_blocks(&BlockFilter::board(&board_id).with_parent(&shared_id))
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
/// against the blocks of the board and the trash afterwards: the block of a round that
/// ended by deleting is in the trash as the task left it, and the block of any other
/// round exists as the task left it.
async fn assert_blocks_of_a_writer_add_up(
    store: &Store,
    task: usize,
    writer_id: &str,
    own_ids: &[String],
    blocks: &[Block],
) {
    for (round, own_id) in own_ids.iter().enumerate() {
        let existing = blocks.iter().find(|block| &block.id == own_id);
        if round % 2 == 0 {
            assert!(
                existing.is_none(),
                "the block of task {task}, round {round} was deleted: {existing:?}"
            );
            let deleted = block_in_trash(store, own_id).await;
            assert_eq!(
                (
                    deleted.deleted_by.as_str(),
                    deleted.block.title.as_str(),
                    deleted.block.modified_by.as_str()
                ),
                (writer_id, "patched", writer_id),
                "the block of task {task}, round {round} is in the trash as its task left it"
            );
            assert_ne!(
                deleted.block.delete_at, 0,
                "the block of task {task}, round {round} has a deletion time"
            );
        } else {
            let existing = existing
                .unwrap_or_else(|| panic!("the block of task {task}, round {round} should exist"));
            assert_eq!(
                (
                    existing.title.as_str(),
                    existing.created_by.as_str(),
                    existing.modified_by.as_str()
                ),
                ("kept", writer_id, writer_id),
                "the block of task {task}, round {round} exists as its task left it"
            );
            assert_not_in_trash(store, own_id).await;
        }
    }
}

/// Many tasks insert, patch and delete blocks of one board through clones of one store
/// at the same time. Every operation must succeed, and afterwards the blocks and the
/// trash must add up.
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
    for (task, (writer_id, own_ids)) in writer_ids.iter().zip(&own_ids).enumerate() {
        // A task owns what it works with.
        let writer = Writer {
            store: store.clone(),
            task,
            user_id: writer_id.clone(),
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
    for (task, (writer_id, own_ids)) in writer_ids.iter().zip(&own_ids).enumerate() {
        assert_blocks_of_a_writer_add_up(store, task, writer_id, own_ids, &blocks).await;
    }

    // The shared block is as one of the patches left it, whole: the title a task gave it
    // and that task's user as modifier.
    let shared = existing_block(store, &shared_id).await;
    let last_writer = shared
        .title
        .strip_prefix("task ")
        .and_then(|rest| rest.split_once(", round "))
        .and_then(|(task, _)| task.parse::<usize>().ok())
        .and_then(|task| writer_ids.get(task))
        .unwrap_or_else(|| panic!("the shared block has the title of a patch: {shared:?}"));
    assert_eq!(
        &shared.modified_by, last_writer,
        "the shared block names the user of its last patch, {:?}, as modifier",
        shared.title
    );
    assert_eq!(
        shared.created_by, user_id,
        "the shared block keeps its creator"
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
    let before = board_blocks(store, board_id).await;

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
    let after = board_blocks(store, board_id).await;
    assert_eq!(
        after, before,
        "the rejected batch left both blocks as they were, modifier and time included"
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
    let before = board_blocks(store, &board_id).await;

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
    assert_eq!(
        blocks, before,
        "the blocks that exist are as they were, modifier and time included"
    );
    assert_not_in_trash(store, &missing).await;

    test.teardown().await;
}

/// Undeleting a block that is not deleted finds nothing in the trash under its
/// identifier and writes nothing. (The original succeeds, and writes nothing either.)
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
    let child_before = block_in_trash(store, child_id).await;

    assert_undelete_finds_nothing(store, parent_id, &someone_else_id).await;

    let after = existing_block(store, parent_id).await;
    assert_eq!(after, before, "the block is untouched");
    assert_no_block(store, child_id).await;
    assert_eq!(
        block_in_trash(store, child_id).await,
        child_before,
        "the deleted child is in the trash as it was"
    );

    test.teardown().await;
}

/// The original lets a block have the empty identifier, and deleting or undeleting that
/// block takes every block of its board along, not only its children
/// (`server/services/store/sqlstore/blocks.go:450`, `:522`, `:929`, `:1017`). Here the
/// empty string is no identifier: no block is stored under it, deleting it touches
/// nothing, and undeleting it finds nothing in the trash.
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

    assert_not_in_trash(store, "").await;

    // Some of the board's blocks are in the trash, where undeleting the empty identifier
    // must leave them.
    let child1_id = id_of(&sample, "child1");
    store
        .delete_block(child1_id, user_id)
        .await
        .expect("a block and what is below it are deleted");
    let left = board_blocks(store, &board_id).await;
    assert_eq!(
        sorted_names(&left),
        ["child2", "grandchild2", "parent"],
        "child1 took what is below it along"
    );

    assert_undelete_finds_nothing(store, "", user_id).await;
    let blocks = board_blocks(store, &board_id).await;
    assert_eq!(
        blocks, left,
        "undeleting the empty identifier brought nothing back and changed nothing"
    );
    block_in_trash(store, child1_id).await;

    test.teardown().await;
}

/// Deleting an image or an attachment, or a card that holds one, succeeds and leaves the
/// record of the file alone: a file is its board's, and its record goes and comes back
/// with the board. The original fails to delete such blocks when it runs standalone,
/// because it updates a table that only its host application has
/// (`server/services/store/sqlstore/blocks.go:429`, `:986`).
#[tokio::test]
async fn deleting_blocks_that_hold_files_leaves_the_file_records() {
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
    let mut files = Vec::new();
    for stored_name in ["direct.png", "image.png", "attachment.pdf", "untouched.png"] {
        files.push(save_file(store, board_id, stored_name).await);
    }
    // Another board has a file under the name of one whose block is deleted here.
    let other_board_id = another_board(store, &team_id, user_id).await;
    files.push(save_file(store, &other_board_id, "direct.png").await);

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

    assert_file_records_are(store, &files, "after their blocks were deleted").await;

    // The blocks come back to the records they named.
    store
        .undelete_block(direct_id, user_id)
        .await
        .expect("the image block is restored");
    store
        .undelete_block(&cards[1].id, user_id)
        .await
        .expect("the card that holds files is restored");
    let restored = board_blocks(store, board_id).await;
    assert_eq!(
        restored.len(),
        cards.len() + blocks.len(),
        "both cards and the four blocks that name files exist again: {restored:?}"
    );
    assert_file_records_are(store, &files, "after their blocks were restored").await;

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

/// Rewrites a JSON column of a SQLite table as blobs, on a connection of its own.
async fn rewrite_as_blobs(test: &common::TestStore, table: &str, column: &str) {
    let mut raw = sqlite_connection(test).await;
    let rewritten = sqlx::query(sqlx::AssertSqlSafe(format!(
        "UPDATE {table} SET {column} = CAST({column} AS BLOB)"
    )))
    .execute(&mut raw)
    .await
    .expect("the column is rewritten as blobs");
    assert_ne!(
        rewritten.rows_affected(),
        0,
        "the {table} table has rows to rewrite"
    );
    raw.close()
        .await
        .expect("the second connection closes cleanly");
}

/// This store writes JSON to SQLite as text, and SQLite keeps JSON that was bound as
/// bytes as a blob, as the original's driver binds it. The fields of a block and a block
/// in the trash are read from either.
#[tokio::test]
async fn sqlite_fields_stored_as_a_blob_are_read() {
    if common::db_type() != DbType::Sqlite {
        // PostgreSQL has JSON columns there, which hold JSON and nothing else.
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

    rewrite_as_blobs(&test, "blocks", "fields").await;
    let stored = existing_block(store, &card_id).await;
    assert_eq!(
        stored.fields, card.fields,
        "the fields of a block are read from a blob"
    );

    // Deleting reads the fields and writes the whole block into the trash.
    store
        .delete_block(&card_id, user_id)
        .await
        .expect("a block with blob fields is deleted");
    rewrite_as_blobs(&test, "deleted_blocks", "block").await;
    let deleted = block_in_trash(store, &card_id).await;
    assert_eq!(
        deleted.block.fields, card.fields,
        "a block in the trash is read from a blob"
    );

    store
        .undelete_block(&card_id, user_id)
        .await
        .expect("a block that is a blob in the trash is undeleted");
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

/// A card with what is on it, down to the third level below it, and a second card with
/// one text block, which no deletion of the first card may touch: the name of each
/// block, the name of its parent and its type.
const CARD_TREE: [(&str, &str, &str); 7] = [
    ("card", "", "card"),
    ("text", "card", "text"),
    ("comment", "card", "comment"),
    ("below", "text", "text"),
    ("deepest", "below", "text"),
    ("other card", "", "card"),
    ("other text", "other card", "text"),
];

/// The blocks of [`CARD_TREE`] that are the first card and everything below it.
const CARD_AND_BELOW: [&str; 5] = ["card", "text", "comment", "below", "deepest"];

/// Stores the blocks of [`CARD_TREE`] on a board and returns them as they were stored.
async fn insert_card_tree(store: &Store, board_id: &str, user_id: &str) -> Vec<Block> {
    let mut tree = named_blocks(board_id, &CARD_TREE);
    common::insert_blocks(store, &mut tree, user_id).await;
    tree
}

/// The block that [`named_blocks`] made under the given name.
#[track_caller]
fn block_named<'a>(blocks: &'a [Block], name: &str) -> &'a Block {
    blocks
        .iter()
        .find(|block| block.title == name)
        .unwrap_or_else(|| panic!("no block is named {name}"))
}

/// The titles of the blocks, in the order they came.
fn names(blocks: &[Block]) -> Vec<&str> {
    blocks.iter().map(|block| block.title.as_str()).collect()
}

/// Asserts that the named blocks of a tree exist again as they were stored, parents
/// included, except that `restorer_id` modified them at `restored_at`, and that none of
/// them is in the trash any more.
async fn assert_restored(
    store: &Store,
    tree: &[Block],
    restored: &[&str],
    restorer_id: &str,
    restored_at: i64,
) {
    for name in restored {
        let stored = block_named(tree, name);
        let expected = Block {
            modified_by: restorer_id.to_owned(),
            update_at: restored_at,
            ..stored.clone()
        };
        assert_eq!(
            existing_block(store, &stored.id).await,
            expected,
            "{name} is back as it was, with the restoring user as modifier at the time of \
             the restore"
        );
        assert_not_in_trash(store, &stored.id).await;
    }
}

/// The blocks of a tree that are in the trash under the given names, as the trash has
/// them.
async fn trash_of(store: &Store, tree: &[Block], deleted: &[&str]) -> Vec<DeletedBlock> {
    let mut in_trash = Vec::with_capacity(deleted.len());
    for name in deleted {
        in_trash.push(block_in_trash(store, id_of(tree, name)).await);
    }
    in_trash
}

/// Deleting a block takes everything below it along, at any depth, as one deletion: each
/// of them is in the trash as it was, with the one deletion time and the deleting user.
#[tokio::test]
async fn delete_block_takes_everything_below_it_along_as_one_deletion() {
    let test = common::setup().await;
    let store = &test.store;
    let Fixture {
        user_id, board_id, ..
    } = fixture(store).await;
    let deleter_id = common::user(store, "deleter").await;
    let tree = insert_card_tree(store, &board_id, &user_id).await;

    store
        .delete_block(id_of(&tree, "card"), &deleter_id)
        .await
        .expect("the card is deleted");

    let left = board_blocks(store, &board_id).await;
    assert_eq!(
        names(&left),
        ["other card", "other text"],
        "the other card and what is on it are all that is left"
    );
    for name in ["other card", "other text"] {
        let stored = block_named(&tree, name);
        assert_eq!(
            &existing_block(store, &stored.id).await,
            stored,
            "{name} is as it was"
        );
        assert_not_in_trash(store, &stored.id).await;
    }

    let deleted_at = block_in_trash(store, id_of(&tree, "card"))
        .await
        .block
        .delete_at;
    assert_within_a_second(now_millis(), deleted_at, "the deletion time of the card");
    for name in CARD_AND_BELOW {
        let stored = block_named(&tree, name);
        assert_no_block(store, &stored.id).await;
        let deleted = block_in_trash(store, &stored.id).await;
        assert_eq!(
            deleted.deleted_by, deleter_id,
            "the trash names who deleted {name}"
        );
        let expected = Block {
            delete_at: deleted_at,
            ..stored.clone()
        };
        assert_eq!(
            deleted.block, expected,
            "{name} is in the trash as it was, with the deletion time of the card"
        );
    }

    test.teardown().await;
}

/// Undeleting a card brings back everything that was deleted with it, each on the block
/// it was on, and returns the card.
#[tokio::test]
async fn undelete_block_of_a_card_brings_back_everything_deleted_with_it() {
    let test = common::setup().await;
    let store = &test.store;
    let Fixture {
        user_id, board_id, ..
    } = fixture(store).await;
    let restorer_id = common::user(store, "restorer").await;
    let tree = insert_card_tree(store, &board_id, &user_id).await;
    let card_id = id_of(&tree, "card");

    store
        .delete_block(card_id, &user_id)
        .await
        .expect("the card is deleted");
    let card = store
        .undelete_block(card_id, &restorer_id)
        .await
        .expect("the card is restored");

    assert_eq!(card.id, card_id, "the restored card is returned");
    assert_eq!(
        card,
        existing_block(store, card_id).await,
        "the card is returned as it is stored"
    );
    assert_within_a_second(now_millis(), card.update_at, "the time of the restore");
    assert_restored(store, &tree, &CARD_AND_BELOW, &restorer_id, card.update_at).await;

    let blocks = board_blocks(store, &board_id).await;
    assert_eq!(
        sorted_names(&blocks),
        sorted_names(&tree),
        "the board has every block once"
    );
    for name in ["other card", "other text"] {
        let stored = block_named(&tree, name);
        assert_eq!(
            &existing_block(store, &stored.id).await,
            stored,
            "{name}, which was not deleted, is as it was"
        );
    }

    test.teardown().await;
}

/// Undeleting a block that was deleted along with its card brings back the whole
/// deletion, the card included, and returns the block that was asked for.
#[tokio::test]
async fn undelete_block_of_a_child_brings_back_the_whole_deletion() {
    let test = common::setup().await;
    let store = &test.store;
    let Fixture {
        user_id, board_id, ..
    } = fixture(store).await;
    let restorer_id = common::user(store, "restorer").await;
    let tree = insert_card_tree(store, &board_id, &user_id).await;
    let deepest_id = id_of(&tree, "deepest");

    store
        .delete_block(id_of(&tree, "card"), &user_id)
        .await
        .expect("the card is deleted");
    let deepest = store
        .undelete_block(deepest_id, &restorer_id)
        .await
        .expect("the block at the bottom is restored");

    assert_eq!(deepest.id, deepest_id, "the block asked for is returned");
    assert_eq!(
        deepest,
        existing_block(store, deepest_id).await,
        "the block is returned as it is stored"
    );
    assert_restored(
        store,
        &tree,
        &CARD_AND_BELOW,
        &restorer_id,
        deepest.update_at,
    )
    .await;
    let blocks = board_blocks(store, &board_id).await;
    assert_eq!(
        sorted_names(&blocks),
        sorted_names(&tree),
        "the board has every block once"
    );

    test.teardown().await;
}

/// A block that was deleted on its own has nothing to come back to while the card it was
/// on is in the trash: undeleting it finds nothing and changes nothing. Restoring the
/// card does not bring it back, since it went with another deletion; after that it can
/// be restored.
#[tokio::test]
async fn a_child_deleted_before_its_card_comes_back_only_after_the_card() {
    let test = common::setup().await;
    let store = &test.store;
    let Fixture {
        user_id, board_id, ..
    } = fixture(store).await;
    let restorer_id = common::user(store, "restorer").await;
    let tree = insert_card_tree(store, &board_id, &user_id).await;
    let card_id = id_of(&tree, "card");
    let text_id = id_of(&tree, "text");

    // The text goes with what is below it, then the card with the comment that is left.
    store
        .delete_block(text_id, &user_id)
        .await
        .expect("the text is deleted");
    store
        .delete_block(card_id, &user_id)
        .await
        .expect("the card is deleted");
    let left = board_blocks(store, &board_id).await;
    assert_eq!(
        names(&left),
        ["other card", "other text"],
        "the card and everything that was on it are gone"
    );
    let trash = trash_of(store, &tree, &CARD_AND_BELOW).await;

    // The text itself, and a block that went with it.
    for name in ["text", "deepest"] {
        assert_undelete_finds_nothing(store, id_of(&tree, name), &restorer_id).await;
        assert_eq!(
            board_blocks(store, &board_id).await,
            left,
            "undeleting {name} while the card is in the trash brought nothing back"
        );
        assert_eq!(
            trash_of(store, &tree, &CARD_AND_BELOW).await,
            trash,
            "undeleting {name} while the card is in the trash left the trash as it was"
        );
    }

    let card = store
        .undelete_block(card_id, &restorer_id)
        .await
        .expect("the card is restored");
    assert_restored(
        store,
        &tree,
        &["card", "comment"],
        &restorer_id,
        card.update_at,
    )
    .await;
    assert_eq!(
        trash_of(store, &tree, &["text", "below", "deepest"]).await,
        trash_of_names(&trash, &tree, &["text", "below", "deepest"]),
        "what was deleted before the card is still in the trash as it was"
    );
    for name in ["text", "below", "deepest"] {
        assert_no_block(store, id_of(&tree, name)).await;
    }

    let restored_text = store
        .undelete_block(text_id, &restorer_id)
        .await
        .expect("the text is restored once its card exists");
    assert_eq!(
        restored_text.parent_id, card_id,
        "the text is back on its card"
    );
    assert_restored(
        store,
        &tree,
        &["text", "below", "deepest"],
        &restorer_id,
        restored_text.update_at,
    )
    .await;
    let blocks = board_blocks(store, &board_id).await;
    assert_eq!(
        sorted_names(&blocks),
        sorted_names(&tree),
        "the board has every block once"
    );

    test.teardown().await;
}

/// The entries of `trash` for the named blocks of a tree, in the order of the names.
fn trash_of_names(trash: &[DeletedBlock], tree: &[Block], deleted: &[&str]) -> Vec<DeletedBlock> {
    deleted
        .iter()
        .map(|name| {
            let id = id_of(tree, name);
            trash
                .iter()
                .find(|entry| entry.block.id == id)
                .unwrap_or_else(|| panic!("{name} was in the trash"))
                .clone()
        })
        .collect()
}

/// Undeleting finds nothing under the identifier of a block that exists, under one that
/// nothing has and under text that is no identifier, and writes nothing.
#[tokio::test]
async fn undelete_block_of_what_is_not_in_the_trash_is_not_found_and_writes_nothing() {
    let test = common::setup().await;
    let store = &test.store;
    let Fixture {
        user_id, board_id, ..
    } = fixture(store).await;
    let restorer_id = common::user(store, "restorer").await;
    let tree = insert_card_tree(store, &board_id, &user_id).await;

    // Something is in the trash, which none of the calls may bring back.
    store
        .delete_block(id_of(&tree, "other card"), &user_id)
        .await
        .expect("the other card is deleted");
    let blocks = board_blocks(store, &board_id).await;
    let trash = trash_of(store, &tree, &["other card", "other text"]).await;

    let unknown_id = new_id();
    for (what, block_id) in [
        ("a block that exists", id_of(&tree, "card")),
        ("a block below one that exists", id_of(&tree, "deepest")),
        ("an identifier that nothing has", unknown_id.as_str()),
        ("text that is no identifier", "no-identifier"),
        ("no text at all", ""),
    ] {
        match store.undelete_block(block_id, &restorer_id).await {
            Ok(block) => panic!("undeleting {what} should find nothing, got {block:?}"),
            Err(error) => assert!(
                matches!(error, StoreError::NotFound(_)),
                "undeleting {what} should be a not-found error, got {error:?}"
            ),
        }
        assert_eq!(
            board_blocks(store, &board_id).await,
            blocks,
            "undeleting {what} left the blocks as they were, modifier and time included"
        );
        assert_eq!(
            trash_of(store, &tree, &["other card", "other text"]).await,
            trash,
            "undeleting {what} left the trash as it was"
        );
        assert_not_in_trash(store, block_id).await;
    }

    test.teardown().await;
}

/// A subscription of a user to a block.
fn subscription(block: &Block, subscriber_id: &str) -> Subscription {
    Subscription {
        block_type: block.block_type.clone(),
        block_id: block.id.clone(),
        subscriber_type: SubscriberType::User,
        subscriber_id: subscriber_id.to_owned(),
        notified_at: 0,
        create_at: 0,
        delete_at: 0,
    }
}

/// The identifiers of the blocks a user is subscribed to, sorted.
async fn subscribed_block_ids(store: &Store, subscriber_id: &str) -> Vec<String> {
    let mut block_ids: Vec<String> = store
        .get_subscriptions(subscriber_id)
        .await
        .expect("the subscriptions of a user are read")
        .into_iter()
        .map(|subscription| subscription.block_id)
        .collect();
    block_ids.sort();
    block_ids
}

/// The subscriptions to a block end when the block is deleted, on its own or along with
/// the block it is on, and restoring the block does not bring them back.
#[tokio::test]
async fn subscriptions_to_a_deleted_block_end_and_do_not_come_back() {
    let test = common::setup().await;
    let store = &test.store;
    let Fixture {
        user_id, board_id, ..
    } = fixture(store).await;
    let subscriber_id = common::user(store, "subscriber").await;
    let tree = insert_card_tree(store, &board_id, &user_id).await;
    let card_id = id_of(&tree, "card");
    let other_card_id = id_of(&tree, "other card");

    // The card, a block that goes with it, and a card that stays. Two users subscribe to
    // the card.
    for name in ["card", "deepest", "other card"] {
        store
            .create_subscription(subscription(block_named(&tree, name), &subscriber_id))
            .await
            .expect("the subscriber subscribes");
    }
    store
        .create_subscription(subscription(block_named(&tree, "card"), &user_id))
        .await
        .expect("a second user subscribes to the card");

    store
        .delete_block(card_id, &user_id)
        .await
        .expect("the card is deleted");
    let assert_ended = async |when: &str| {
        for name in ["card", "deepest"] {
            let block_id = id_of(&tree, name);
            let error = store
                .get_subscription(block_id, &subscriber_id)
                .await
                .expect_err("the subscription has ended");
            assert!(
                matches!(error, StoreError::NotFound(_)),
                "the subscription to {name} is not found {when}: {error:?}"
            );
            let subscribers = store
                .get_subscribers_for_block(block_id)
                .await
                .expect("the subscribers of a block are read");
            assert!(
                subscribers.is_empty(),
                "{name} has no subscribers {when}: {subscribers:?}"
            );
        }
        assert_eq!(
            subscribed_block_ids(store, &subscriber_id).await,
            [other_card_id],
            "the subscription to the card that was not deleted is the one left {when}"
        );
        assert!(
            subscribed_block_ids(store, &user_id).await.is_empty(),
            "the second user has no subscription left {when}"
        );
    };
    assert_ended("after the card was deleted").await;

    store
        .undelete_block(card_id, &user_id)
        .await
        .expect("the card is restored");
    existing_block(store, id_of(&tree, "deepest")).await;
    assert_ended("after the card was restored").await;

    // The restored block can be subscribed to like any other.
    store
        .create_subscription(subscription(block_named(&tree, "card"), &subscriber_id))
        .await
        .expect("the subscriber subscribes to the restored card");
    let mut expected = strings([card_id, other_card_id]);
    expected.sort();
    assert_eq!(
        subscribed_block_ids(store, &subscriber_id).await,
        expected,
        "the new subscription to the restored card is there"
    );

    test.teardown().await;
}

/// The parent of a block is a block of the same board that exists. A block with any
/// other parent is refused as invalid, whether it is new or exists, and nothing is
/// written.
#[tokio::test]
async fn insert_block_refuses_a_parent_that_is_not_a_block_of_the_board() {
    let test = common::setup().await;
    let store = &test.store;
    let Fixture {
        user_id,
        team_id,
        board_id,
    } = fixture(store).await;
    let user_id = user_id.as_str();
    let other_board_id = another_board(store, &team_id, user_id).await;

    let mut sample = named_blocks(&board_id, &[("card", "", "card"), ("deleted", "", "card")]);
    common::insert_blocks(store, &mut sample, user_id).await;
    let card_id = id_of(&sample, "card");
    let deleted_id = id_of(&sample, "deleted");
    store
        .delete_block(deleted_id, user_id)
        .await
        .expect("a card is deleted");
    let mut foreign = Block {
        block_type: BlockType::Card,
        ..block(&new_id(), &other_board_id)
    };
    store
        .insert_block(&mut foreign, user_id)
        .await
        .expect("a card of another board is inserted");
    let blocks = board_blocks(store, &board_id).await;
    let other_blocks = board_blocks(store, &other_board_id).await;

    let unknown_id = new_id();
    let own_id = new_id();
    for (what, child_id, parent_id) in [
        ("a block of another board", new_id(), foreign.id.as_str()),
        ("a block that does not exist", new_id(), unknown_id.as_str()),
        ("the block itself", own_id.clone(), own_id.as_str()),
        ("a block in the trash", new_id(), deleted_id),
        ("text that is no identifier", new_id(), "no-identifier"),
        // A block that exists is refused such a parent too.
        (
            "itself, for a block that exists",
            card_id.to_owned(),
            card_id,
        ),
        (
            "a block of another board, for a block that exists",
            card_id.to_owned(),
            foreign.id.as_str(),
        ),
    ] {
        let mut child = Block {
            parent_id: parent_id.to_owned(),
            block_type: BlockType::Text,
            title: "refused".to_owned(),
            ..block(&child_id, &board_id)
        };
        match store.insert_block(&mut child, user_id).await {
            Ok(()) => panic!("a block whose parent is {what} should be refused"),
            Err(error) => assert!(
                matches!(
                    &error,
                    StoreError::InvalidBlock { id, source: BlockError::ParentNotOnBoard }
                        if *id == child_id
                ),
                "a block whose parent is {what} should be refused over its parent, got \
                 {error:?}"
            ),
        }
        assert_eq!(
            board_blocks(store, &board_id).await,
            blocks,
            "the block whose parent is {what} was not stored and changed nothing"
        );
        assert_eq!(
            board_blocks(store, &other_board_id).await,
            other_blocks,
            "the other board is as it was after a block whose parent is {what}"
        );
    }

    test.teardown().await;
}

/// A block that names its board as its parent is directly on its board: it is stored,
/// and comes back, without a parent, when it is inserted and when it is updated. Asked
/// for by the same name, the blocks directly on the board are the ones found.
#[tokio::test]
async fn a_block_that_names_its_board_as_parent_is_stored_without_a_parent() {
    let test = common::setup().await;
    let store = &test.store;
    let Fixture {
        user_id, board_id, ..
    } = fixture(store).await;

    let card_id = new_id();
    let mut card = Block {
        parent_id: board_id.clone(),
        block_type: BlockType::Card,
        ..block(&card_id, &board_id)
    };
    for (written, title) in [("inserted", "new"), ("updated", "renamed")] {
        card.parent_id.clone_from(&board_id);
        title.clone_into(&mut card.title);
        store
            .insert_block(&mut card, &user_id)
            .await
            .unwrap_or_else(|error| {
                panic!("a block with its board as parent should be {written}: {error:?}")
            });
        assert_eq!(
            card.parent_id, "",
            "the block that was {written} comes back without a parent"
        );
        let stored = existing_block(store, &card_id).await;
        assert_eq!(
            (stored.parent_id.as_str(), stored.title.as_str()),
            ("", title),
            "the block that was {written} is stored without a parent"
        );
    }

    // A batch does the same, and a block of the batch may be on such a block.
    let mut batch = [
        Block {
            parent_id: board_id.clone(),
            block_type: BlockType::Card,
            ..block(&new_id(), &board_id)
        },
        Block {
            parent_id: card_id.clone(),
            block_type: BlockType::Text,
            ..block(&new_id(), &board_id)
        },
    ];
    store
        .insert_blocks(&mut batch, &user_id)
        .await
        .expect("a batch with a block that names its board as parent is stored");
    assert_eq!(
        existing_block(store, &batch[0].id).await.parent_id,
        "",
        "the card of the batch is stored without a parent"
    );
    assert_eq!(
        existing_block(store, &batch[1].id).await.parent_id,
        card_id,
        "the text of the batch is on the card"
    );
    let on_the_board = store
        .get_blocks(&BlockFilter::board(&board_id).with_parent(&board_id))
        .await
        .expect("the query runs");
    let mut cards = vec![card_id.clone(), batch[0].id.clone()];
    cards.sort();
    assert_eq!(
        common::sorted_ids(&on_the_board),
        cards,
        "the two cards are on the board, the text is on a card"
    );

    test.teardown().await;
}

/// A batch may give children before their parents, at any depth: all of it is stored.
/// A batch with a block whose parent is neither stored nor in the batch stores nothing.
#[tokio::test]
async fn insert_blocks_stores_children_given_before_their_parents() {
    let test = common::setup().await;
    let store = &test.store;
    let Fixture {
        user_id, board_id, ..
    } = fixture(store).await;

    // The deepest block first and the card it is below last.
    let mut batch: Vec<Block> = named_blocks(&board_id, &CARD_TREE)
        .into_iter()
        .rev()
        .collect();
    assert_eq!(
        names(&batch)[..2],
        ["other text", "other card"],
        "the batch gives a child right before its parent"
    );
    let given_parents: Vec<String> = batch.iter().map(|block| block.parent_id.clone()).collect();
    store
        .insert_blocks(&mut batch, &user_id)
        .await
        .expect("a batch with children before their parents is stored");

    let stored = board_blocks(store, &board_id).await;
    assert_eq!(
        stored.len(),
        CARD_TREE.len(),
        "every block of the batch is stored once: {stored:?}"
    );
    for (block, given_parent) in batch.iter().zip(&given_parents) {
        assert_eq!(
            &existing_block(store, &block.id).await,
            block,
            "{} is stored as the batch says it was",
            block.title
        );
        assert_eq!(
            &block.parent_id, given_parent,
            "{} is on the block it was given",
            block.title
        );
    }

    // All or none: the first two blocks alone would be fine.
    let mut late: Vec<Block> = named_blocks(
        &board_id,
        &[
            ("late card", "", "card"),
            ("late text", "late card", "text"),
        ],
    )
    .into_iter()
    .rev()
    .collect();
    late.push(Block {
        parent_id: new_id(),
        block_type: BlockType::Text,
        title: "orphan".to_owned(),
        ..block(&new_id(), &board_id)
    });
    let error = store
        .insert_blocks(&mut late, &user_id)
        .await
        .expect_err("a batch with a block on a block that does not exist is refused");
    assert_invalid_block(&error, &BlockError::ParentNotOnBoard);
    assert_eq!(
        board_blocks(store, &board_id).await,
        stored,
        "the refused batch stored none of its blocks"
    );

    test.teardown().await;
}

/// Every list of blocks comes oldest first, by creation time and then by identifier,
/// whatever the order in which the blocks were written and asked for.
#[tokio::test]
async fn lists_of_blocks_are_ordered_by_creation_time_then_identifier() {
    /// The children of the card: which of the identifiers each has, counted from the
    /// smallest, its creation time and its type. They are written in this order, which is
    /// neither that of the identifiers nor that of the creation times.
    const CHILDREN: [(usize, i64, &str); 6] = [
        (2, 4000, "comment"),
        (4, 3000, "text"),
        (1, 5000, "text"),
        (5, 2000, "text"),
        (0, 4000, "text"),
        (3, 3000, "comment"),
    ];
    /// The children by the number of their identifier, oldest first; of two that are
    /// equally old the one with the smaller identifier first.
    const OLDEST_FIRST: [&str; 6] = ["5", "3", "4", "0", "2", "1"];

    let test = common::setup().await;
    let store = &test.store;
    let Fixture {
        user_id, board_id, ..
    } = fixture(store).await;
    let board = || BlockFilter::board(&board_id);

    let mut ids: Vec<String> = (0..CHILDREN.len()).map(|_| new_id()).collect();
    ids.sort();
    // The card is written first and is the youngest, with the greatest identifier.
    let card_id = new_id();
    let mut blocks = vec![Block {
        block_type: BlockType::Card,
        title: "card".to_owned(),
        ..block(&card_id, &board_id)
    }];
    blocks.extend(CHILDREN.into_iter().map(|(number, _, block_type)| Block {
        parent_id: card_id.clone(),
        block_type: BlockType::from(block_type),
        title: number.to_string(),
        ..block(&ids[number], &board_id)
    }));
    common::insert_blocks(store, &mut blocks, &user_id).await;

    // The store creates every block now, so the creation times are set past it: two
    // pairs of blocks are equally old, which the clock would make a matter of luck.
    let set_times: Vec<String> = CHILDREN
        .iter()
        .map(|&(number, create_at, _)| (ids[number].as_str(), create_at))
        .chain([(card_id.as_str(), 9000)])
        .map(|(id, create_at)| {
            format!("UPDATE blocks SET create_at = {create_at} WHERE id = '{id}';")
        })
        .collect();
    common::execute_raw(&test.dbconfig(), &set_times.join("\n")).await;

    let listed = async |filter: BlockFilter| -> Vec<String> {
        let blocks = store.get_blocks(&filter).await.expect("the query runs");
        blocks.into_iter().map(|block| block.title).collect()
    };
    let with_card = [&OLDEST_FIRST[..], &["card"]].concat();

    assert_eq!(
        listed(board()).await,
        with_card,
        "the blocks of a board, the card last although it was written first"
    );
    assert_eq!(
        listed(BlockFilter::default()).await,
        with_card,
        "all blocks there are"
    );
    assert_eq!(
        listed(board().with_parent(&card_id)).await,
        OLDEST_FIRST,
        "the blocks on a card"
    );
    assert_eq!(
        listed(board().with_type(BlockType::Text)).await,
        ["5", "4", "0", "1"],
        "the blocks of a type"
    );
    assert_eq!(
        listed(board().with_parent(&card_id).with_type(BlockType::Comment)).await,
        ["3", "2"],
        "the blocks of a type on a card"
    );
    for (page, expected) in [(0, &OLDEST_FIRST[..4]), (1, &OLDEST_FIRST[4..])] {
        let filter = BlockFilter {
            paging: Some(Paging { page, per_page: 4 }),
            ..board().with_parent(&card_id)
        };
        assert_eq!(
            listed(filter).await,
            expected,
            "page {page} of the blocks on a card, four per page"
        );
    }

    // Asked for from the smallest identifier to the greatest.
    let mut asked = ids.clone();
    asked.push(card_id.clone());
    let by_ids = store
        .get_blocks_by_ids(&asked)
        .await
        .expect("all seven blocks exist");
    assert_eq!(
        names(&by_ids),
        with_card,
        "the blocks with given identifiers"
    );

    test.teardown().await;
}

/// Purging the trash removes for good what was deleted before the given time and leaves
/// what was deleted later, which can still be restored, and what exists.
#[tokio::test]
async fn purge_trash_removes_what_was_deleted_before_the_cutoff() {
    let test = common::setup().await;
    let store = &test.store;
    let Fixture {
        user_id, board_id, ..
    } = fixture(store).await;
    let user_id = user_id.as_str();
    let tree = insert_card_tree(store, &board_id, user_id).await;
    let card_id = id_of(&tree, "card");
    let other_card_id = id_of(&tree, "other card");
    let mut kept = block(&new_id(), &board_id);
    store
        .insert_block(&mut kept, user_id)
        .await
        .expect("a block that stays is inserted");

    store
        .delete_block(card_id, user_id)
        .await
        .expect("the card is deleted");
    let older = block_in_trash(store, card_id).await.block.delete_at;
    // A later millisecond, so that the two deletions have different times.
    tokio::time::sleep(Duration::from_millis(2)).await;
    store
        .delete_block(other_card_id, user_id)
        .await
        .expect("the other card is deleted");
    let newer = block_in_trash(store, other_card_id).await.block.delete_at;
    assert!(
        newer > older,
        "the second deletion is later than the first: {newer} after {older}"
    );
    let newer_trash = trash_of(store, &tree, &["other card", "other text"]).await;

    let purged = store.purge_trash(older).await.expect("the trash is purged");
    assert_eq!(
        purged,
        common::purged_without_files(0),
        "what was deleted at the cutoff, and not before it, stays"
    );
    block_in_trash(store, card_id).await;

    let purged = store.purge_trash(newer).await.expect("the trash is purged");
    assert_eq!(
        purged,
        common::purged_without_files(5),
        "the card and the four blocks below it were deleted before the cutoff"
    );
    for name in CARD_AND_BELOW {
        let block_id = id_of(&tree, name);
        assert_not_in_trash(store, block_id).await;
        assert_no_block(store, block_id).await;
    }
    assert_undelete_finds_nothing(store, card_id, user_id).await;
    assert_undelete_finds_nothing(store, id_of(&tree, "deepest"), user_id).await;
    assert_eq!(
        trash_of(store, &tree, &["other card", "other text"]).await,
        newer_trash,
        "what was deleted at the cutoff is in the trash as it was"
    );
    assert_eq!(
        board_blocks(store, &board_id).await,
        [kept.clone()],
        "the block that exists is as it was"
    );

    let other_card = store
        .undelete_block(other_card_id, user_id)
        .await
        .expect("what the purge left in the trash is restored");
    assert_restored(
        store,
        &tree,
        &["other card", "other text"],
        user_id,
        other_card.update_at,
    )
    .await;

    let purged = store
        .purge_trash(now_millis() + 60_000)
        .await
        .expect("the empty trash is purged");
    assert_eq!(
        purged,
        common::purged_without_files(0),
        "nothing is left in the trash to purge"
    );
    let blocks = board_blocks(store, &board_id).await;
    assert_eq!(
        sorted_names(&blocks),
        ["", "other card", "other text"],
        "a purge removes nothing that exists"
    );

    test.teardown().await;
}

/// A block stays on its board: writing a block under an identifier that a block of
/// another board has is a not-found error and changes neither board. (The original
/// leaves the block alone as well, but reports success.)
#[tokio::test]
async fn insert_block_of_an_identifier_on_another_board_is_not_found() {
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
    let error = store
        .insert_block(&mut moved, &user_b)
        .await
        .expect_err("the block is not written under another board");
    assert_not_found(&error);
    let mut batch = [moved];
    let error = store
        .insert_blocks(&mut batch, &user_b)
        .await
        .expect_err("a batch does not write the block under another board either");
    assert_not_found(&error);

    let after = existing_block(store, &moving_id).await;
    assert_eq!(after, before, "the block is untouched");
    let blocks = board_blocks(store, &board_b).await;
    assert!(
        blocks.is_empty(),
        "the other board did not get the block: {blocks:?}"
    );

    test.teardown().await;
}

/// An update, by a write of the whole block or by a patch, leaves the stored creator and
/// creation time alone, whatever the written block says, and sets modifier and
/// modification time.
#[tokio::test]
async fn an_update_keeps_the_creator_and_the_creation_time() {
    let test = common::setup().await;
    let store = &test.store;
    let Fixture { board_id, .. } = fixture(store).await;
    let creator_id = common::user(store, "creator").await;
    let updater_id = common::user(store, "updater").await;

    let block_id = new_id();
    let mut new = block(&block_id, &board_id);
    store
        .insert_block(&mut new, &creator_id)
        .await
        .expect("the block is inserted");
    let created = existing_block(store, &block_id).await;

    for title in ["written", "patched"] {
        let before = existing_block(store, &block_id).await;
        // A later millisecond, so that the modification times can be told apart.
        tokio::time::sleep(Duration::from_millis(2)).await;
        if title == "written" {
            let mut update = Block {
                title: title.to_owned(),
                created_by: updater_id.clone(),
                create_at: 631_155_600_000,
                ..block(&block_id, &board_id)
            };
            store
                .insert_block(&mut update, &updater_id)
                .await
                .expect("the block is updated");
        } else {
            store
                .patch_block(&block_id, &title_patch(title), &updater_id)
                .await
                .expect("the block is patched");
        }

        let updated = existing_block(store, &block_id).await;
        assert_eq!(
            (updated.created_by.as_str(), updated.create_at),
            (creator_id.as_str(), created.create_at),
            "the block that was {title} keeps its creator and creation time"
        );
        assert_eq!(
            (updated.title.as_str(), updated.modified_by.as_str()),
            (title, updater_id.as_str()),
            "the block that was {title} has the new title and modifier"
        );
        assert!(
            updated.update_at > before.update_at,
            "the block that was {title} has a later modification time: {} after {}",
            updated.update_at,
            before.update_at
        );
    }

    test.teardown().await;
}

/// One batch may write the same block many times: the first write inserts it, the others
/// update it in the order given, and the block is in the state of the last write.
#[tokio::test]
async fn one_batch_writing_a_block_many_times_keeps_the_last_write() {
    const WRITES: usize = 100;

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

    let blocks = board_blocks(store, &board_id).await;
    assert_eq!(
        names(&blocks),
        [format!("write {}", WRITES - 1)],
        "the board has the block once, in the state of the last write"
    );

    test.teardown().await;
}

/// A card with a text block on it and a text block on that: the name of each block, the
/// name of its parent and its type.
const SMALL_TREE: [(&str, &str, &str); 3] = [
    ("card", "", "card"),
    ("text", "card", "text"),
    ("below", "text", "text"),
];

/// How many times each race below is run.
const RACE_ROUNDS: usize = 40;

/// How many calls compete in a round of a race below.
const RACERS: usize = 4;

/// Several callers delete the same card at the same time. Deleting what is deleted
/// already succeeds, so every call succeeds, and the card is in the trash once, with
/// everything below it, as one deletion.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_deletes_of_one_block_all_succeed_and_delete_it_once() {
    let test = common::setup().await;
    let store = &test.store;
    let Fixture {
        user_id, board_id, ..
    } = fixture(store).await;

    for round in 0..RACE_ROUNDS {
        let mut tree = named_blocks(&board_id, &SMALL_TREE);
        common::insert_blocks(store, &mut tree, &user_id).await;
        let card_id = id_of(&tree, "card");

        let start = Arc::new(Barrier::new(RACERS));
        let mut deletes = JoinSet::new();
        for _ in 0..RACERS {
            let (store, start) = (store.clone(), Arc::clone(&start));
            let (card_id, user_id) = (card_id.to_owned(), user_id.clone());
            deletes.spawn(async move {
                start.wait().await;
                store.delete_block(&card_id, &user_id).await
            });
        }
        while let Some(joined) = deletes.join_next().await {
            let outcome = joined.expect("a deleting task runs to its end");
            assert!(
                outcome.is_ok(),
                "round {round}: every delete of the card succeeds, got {outcome:?}"
            );
        }

        let blocks = board_blocks(store, &board_id).await;
        assert!(
            blocks.is_empty(),
            "round {round}: the card and what was below it are gone: {blocks:?}"
        );
        let deleted_at = block_in_trash(store, card_id).await.block.delete_at;
        for stored in &tree {
            let expected = Block {
                delete_at: deleted_at,
                ..stored.clone()
            };
            assert_eq!(
                block_in_trash(store, &stored.id).await.block,
                expected,
                "round {round}: {} is in the trash as it was, as part of one deletion",
                stored.title
            );
        }
    }

    test.teardown().await;
}

/// Several callers undelete the same deletion at the same time, some by its card and
/// some by a block that went with it. One of them restores it; the others find nothing
/// in the trash. Every block is back once.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_undeletes_of_one_deletion_restore_it_once() {
    let test = common::setup().await;
    let store = &test.store;
    let Fixture {
        user_id, board_id, ..
    } = fixture(store).await;

    for round in 0..RACE_ROUNDS {
        let mut tree = named_blocks(&board_id, &SMALL_TREE);
        common::insert_blocks(store, &mut tree, &user_id).await;
        store
            .delete_block(id_of(&tree, "card"), &user_id)
            .await
            .expect("the card is deleted");

        let start = Arc::new(Barrier::new(RACERS));
        let mut undeletes = JoinSet::new();
        for racer in 0..RACERS {
            let (store, start) = (store.clone(), Arc::clone(&start));
            let name = if racer % 2 == 0 { "card" } else { "below" };
            let (block_id, user_id) = (id_of(&tree, name).to_owned(), user_id.clone());
            undeletes.spawn(async move {
                start.wait().await;
                store.undelete_block(&block_id, &user_id).await
            });
        }
        let mut restored = 0;
        while let Some(joined) = undeletes.join_next().await {
            match joined.expect("an undeleting task runs to its end") {
                Ok(_) => restored += 1,
                Err(error) => assert!(
                    matches!(error, StoreError::NotFound(_)),
                    "round {round}: an undelete that comes too late finds nothing, got \
                     {error:?}"
                ),
            }
        }
        assert_eq!(
            restored, 1,
            "round {round}: one undelete restores the deletion"
        );

        let blocks = board_blocks(store, &board_id).await;
        assert_eq!(
            sorted_names(&blocks),
            ["below", "card", "text"],
            "round {round}: every block is back once"
        );
        for stored in &tree {
            assert_not_in_trash(store, &stored.id).await;
        }
        // The board is empty again for the next round.
        store
            .delete_block(id_of(&tree, "card"), &user_id)
            .await
            .expect("the card is deleted again");
    }

    test.teardown().await;
}

/// A block is put on a card while the card is deleted. Either it came first, and then
/// it went into the trash with the card, or the card was gone, and then it is refused
/// over its parent. It is never lost, and neither call fails for another reason.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_block_inserted_while_its_card_is_deleted_goes_with_the_card_or_is_refused() {
    let test = common::setup().await;
    let store = &test.store;
    let Fixture {
        user_id, board_id, ..
    } = fixture(store).await;

    for round in 0..RACE_ROUNDS {
        let mut tree = named_blocks(&board_id, &SMALL_TREE);
        common::insert_blocks(store, &mut tree, &user_id).await;
        let card_id = id_of(&tree, "card");
        let mut late = Block {
            parent_id: id_of(&tree, "below").to_owned(),
            block_type: BlockType::Text,
            title: "late".to_owned(),
            ..block(&new_id(), &board_id)
        };

        let start = Barrier::new(2);
        let (deleted, inserted) = tokio::join!(
            async {
                start.wait().await;
                store.delete_block(card_id, &user_id).await
            },
            async {
                start.wait().await;
                store.insert_block(&mut late, &user_id).await
            }
        );

        assert!(
            deleted.is_ok(),
            "round {round}: the card is deleted, got {deleted:?}"
        );
        let blocks = board_blocks(store, &board_id).await;
        assert!(
            blocks.is_empty(),
            "round {round}: the card and everything below it are gone: {blocks:?}"
        );
        for stored in &tree {
            block_in_trash(store, &stored.id).await;
        }
        match inserted {
            Ok(()) => {
                let in_trash = store.get_deleted_block(&late.id).await;
                assert!(
                    in_trash.is_ok(),
                    "round {round}: the block that was stored went into the trash with its \
                     card, got {in_trash:?}"
                );
            }
            Err(error) => {
                assert_invalid_block(&error, &BlockError::ParentNotOnBoard);
                assert_not_in_trash(store, &late.id).await;
            }
        }
    }

    test.teardown().await;
}

/// A block is not stored on a board that does not exist, on one that is in the trash or
/// under text that names no board: that is a not-found error, alone and in a batch, and
/// nothing of the batch is stored.
#[tokio::test]
async fn insert_block_on_a_board_that_does_not_exist_is_not_found() {
    let test = common::setup().await;
    let store = &test.store;
    let Fixture {
        user_id,
        team_id,
        board_id,
    } = fixture(store).await;
    let user_id = user_id.as_str();
    let mut card = Block {
        block_type: BlockType::Card,
        ..block(&new_id(), &board_id)
    };
    store
        .insert_block(&mut card, user_id)
        .await
        .expect("a card of the board that exists is inserted");
    let deleted_board_id = another_board(store, &team_id, user_id).await;
    store
        .delete_board(&deleted_board_id, user_id)
        .await
        .expect("a board is deleted");
    let all_blocks = async || {
        store
            .get_blocks(&BlockFilter::default())
            .await
            .expect("all blocks are read")
    };
    let before = all_blocks().await;

    let unknown_board_id = new_id();
    for (what, missing_board) in [
        ("a board that does not exist", unknown_board_id.as_str()),
        ("a board in the trash", deleted_board_id.as_str()),
        ("text that is no identifier", "no-identifier"),
    ] {
        // Directly on the board, and on a block that exists on another board.
        for parent_id in ["", card.id.as_str()] {
            let mut homeless = Block {
                parent_id: parent_id.to_owned(),
                ..block(&new_id(), missing_board)
            };
            match store.insert_block(&mut homeless, user_id).await {
                Ok(()) => panic!("a block on {what} should not be stored"),
                Err(error) => assert!(
                    matches!(error, StoreError::NotFound(_)),
                    "a block on {what} with the parent {parent_id:?} should be a not-found \
                     error, got {error:?}"
                ),
            }
            assert_no_block(store, &homeless.id).await;
            assert_not_in_trash(store, &homeless.id).await;
        }

        // The first block of the batch alone would be fine.
        let mut batch = [block(&new_id(), &board_id), block(&new_id(), missing_board)];
        match store.insert_blocks(&mut batch, user_id).await {
            Ok(()) => panic!("a batch with a block on {what} should not be stored"),
            Err(error) => assert!(
                matches!(error, StoreError::NotFound(_)),
                "a batch with a block on {what} should be a not-found error, got {error:?}"
            ),
        }
        assert_eq!(
            all_blocks().await,
            before,
            "nothing was stored for {what}, of the batch either"
        );
    }

    test.teardown().await;
}

/// A patch that puts a block on another block or, with the empty text, at the top of
/// its board.
fn parent_patch(parent_id: &str) -> BlockPatch {
    BlockPatch {
        parent_id: Some(parent_id.to_owned()),
        ..BlockPatch::default()
    }
}

/// The two ways of giving a block another parent.
#[derive(Debug, Clone, Copy)]
enum MoveBy {
    /// Writing the whole block with `insert_block`.
    Write,
    /// Patching the parent with `patch_block`.
    Patch,
}

/// Puts a block that exists on another block or, with the empty text, at the top of its
/// board.
async fn move_block(
    store: &Store,
    block_id: &str,
    parent_id: &str,
    user_id: &str,
    by: MoveBy,
) -> Result<(), StoreError> {
    match by {
        MoveBy::Write => {
            let mut block = existing_block(store, block_id).await;
            parent_id.clone_into(&mut block.parent_id);
            store.insert_block(&mut block, user_id).await
        }
        MoveBy::Patch => {
            store
                .patch_block(block_id, &parent_patch(parent_id), user_id)
                .await
        }
    }
}

/// Asserts that a move was refused because it would put the block below itself.
#[track_caller]
fn assert_refused_as_below_itself(outcome: &Result<(), StoreError>, moved_id: &str, what: &str) {
    assert!(
        matches!(
            outcome,
            Err(StoreError::InvalidBlock { id, source: BlockError::ParentBelowBlock })
                if id == moved_id
        ),
        "{what} should be refused as putting the block below itself, got {outcome:?}"
    );
}

/// The name of each block with the name of the block it is on, sorted by name.
fn names_with_parents(blocks: &[Block]) -> Vec<(&str, &str)> {
    let mut pairs: Vec<(&str, &str)> = blocks
        .iter()
        .map(|block| {
            let parent = blocks
                .iter()
                .find(|parent| parent.id == block.parent_id)
                .map_or("", |parent| parent.title.as_str());
            (block.title.as_str(), parent)
        })
        .collect();
    pairs.sort_unstable();
    pairs
}

/// A block is not moved onto a block below it, at any depth, by a write of the block, by
/// a patch or within a batch, and nothing changes. It may move onto any other block of
/// its board and to the top, and what is below it is what counts at the time.
#[tokio::test]
async fn an_update_that_would_put_a_block_below_itself_is_refused() {
    let test = common::setup().await;
    let store = &test.store;
    let Fixture {
        user_id, board_id, ..
    } = fixture(store).await;
    let user_id = user_id.as_str();

    let specs = [&SMALL_TREE[..], &[("other card", "", "card")]].concat();
    let mut tree = named_blocks(&board_id, &specs);
    common::insert_blocks(store, &mut tree, user_id).await;
    let id = |name: &str| id_of(&tree, name);
    let blocks = board_blocks(store, &board_id).await;

    for (moved, onto) in [("card", "text"), ("card", "below"), ("text", "below")] {
        for by in [MoveBy::Write, MoveBy::Patch] {
            let what = format!("moving {moved} onto {onto} by {by:?}");
            let outcome = move_block(store, id(moved), id(onto), user_id, by).await;
            assert_refused_as_below_itself(&outcome, id(moved), &what);
            assert_eq!(
                board_blocks(store, &board_id).await,
                blocks,
                "{what} changed nothing, modifier and time included"
            );
        }
    }

    // Two blocks at the top that one batch puts on each other.
    let mut batch = [
        Block {
            parent_id: id("other card").to_owned(),
            ..block_named(&tree, "card").clone()
        },
        Block {
            parent_id: id("card").to_owned(),
            ..block_named(&tree, "other card").clone()
        },
    ];
    let error = store
        .insert_blocks(&mut batch, user_id)
        .await
        .expect_err("a batch that puts two blocks on each other is refused");
    assert_invalid_block(&error, &BlockError::ParentBelowBlock);
    assert_eq!(
        board_blocks(store, &board_id).await,
        blocks,
        "the refused batch changed nothing"
    );

    // Moves that put no block below itself: up, to another card, to the top, and onto
    // a block that was below the moved one until it moved away.
    for (moved, onto, by) in [
        ("below", "card", MoveBy::Patch),
        ("text", "other card", MoveBy::Write),
        ("below", "", MoveBy::Patch),
        ("below", "", MoveBy::Write),
        ("card", "text", MoveBy::Write),
    ] {
        let parent_id = if onto.is_empty() { "" } else { id(onto) };
        move_block(store, id(moved), parent_id, user_id, by)
            .await
            .unwrap_or_else(|error| {
                panic!("moving {moved} onto {onto:?} by {by:?} should work: {error:?}")
            });
        assert_eq!(
            existing_block(store, id(moved)).await.parent_id,
            parent_id,
            "{moved} is on {onto:?} after moving by {by:?}"
        );
    }
    let moved = board_blocks(store, &board_id).await;
    assert_eq!(
        names_with_parents(&moved),
        [
            ("below", ""),
            ("card", "text"),
            ("other card", ""),
            ("text", "other card")
        ],
        "every block is where it was moved to, and the others where they were"
    );

    // The card is now two levels below the other card.
    for by in [MoveBy::Write, MoveBy::Patch] {
        let what = format!("moving the other card onto the card by {by:?}");
        let outcome = move_block(store, id("other card"), id("card"), user_id, by).await;
        assert_refused_as_below_itself(&outcome, id("other card"), &what);
        assert_eq!(
            board_blocks(store, &board_id).await,
            moved,
            "{what} changed nothing"
        );
    }

    test.teardown().await;
}

/// Two blocks at the top of a board are moved onto each other at the same time. One move
/// succeeds; the other is refused, because by then its block has the other below it. The
/// two never end up on each other, out of reach from the board.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_blocks_moved_onto_each_other_at_once_never_form_a_cycle() {
    let test = common::setup().await;
    let store = &test.store;
    let Fixture {
        user_id, board_id, ..
    } = fixture(store).await;

    for round in 0..RACE_ROUNDS {
        let mut pair = named_blocks(&board_id, &[("a", "", "card"), ("b", "", "card")]);
        common::insert_blocks(store, &mut pair, &user_id).await;
        // Both ways of moving a block take part, in every pairing.
        let ways = [
            [MoveBy::Write, MoveBy::Write],
            [MoveBy::Write, MoveBy::Patch],
            [MoveBy::Patch, MoveBy::Write],
            [MoveBy::Patch, MoveBy::Patch],
        ][round % 4];

        let start = Arc::new(Barrier::new(2));
        let mut racers = JoinSet::new();
        for (moved, onto, by) in [("a", "b", ways[0]), ("b", "a", ways[1])] {
            let (store, start) = (store.clone(), Arc::clone(&start));
            let (moved_id, onto_id) = (
                id_of(&pair, moved).to_owned(),
                id_of(&pair, onto).to_owned(),
            );
            let user_id = user_id.clone();
            racers.spawn(async move {
                start.wait().await;
                let outcome = move_block(&store, &moved_id, &onto_id, &user_id, by).await;
                (moved, moved_id, outcome)
            });
        }
        let mut on_the_other = Vec::new();
        while let Some(joined) = racers.join_next().await {
            let (moved, moved_id, outcome) = joined.expect("a moving task runs to its end");
            if outcome.is_ok() {
                on_the_other.push(moved);
            } else {
                assert_refused_as_below_itself(
                    &outcome,
                    &moved_id,
                    &format!("round {round}: the move of {moved} that came second"),
                );
            }
        }
        assert_eq!(
            on_the_other.len(),
            1,
            "round {round}, moving by {ways:?}: one of the two moves succeeds: \
             {on_the_other:?}"
        );

        let blocks = board_blocks(store, &board_id).await;
        let expected = if on_the_other == ["a"] {
            [("a", "b"), ("b", "")]
        } else {
            [("a", ""), ("b", "a")]
        };
        assert_eq!(
            names_with_parents(&blocks),
            expected,
            "round {round}: the block that was moved is on the other, which is at the top"
        );

        // Deleting the one at the top takes the other along: the board is empty again.
        let top = if on_the_other == ["a"] { "b" } else { "a" };
        store
            .delete_block(id_of(&pair, top), &user_id)
            .await
            .expect("the block at the top is deleted");
        let left = board_blocks(store, &board_id).await;
        assert!(
            left.is_empty(),
            "round {round}: both blocks went with the one at the top: {left:?}"
        );
    }

    test.teardown().await;
}

/// A block is put on a board, directly or on a card, while the board is deleted. Either
/// it came first, and then it is in the trash with the board and comes back with it, or
/// the board was gone, and then that is a not-found error and the block is nowhere. It
/// is never lost, and neither call fails for another reason.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_block_inserted_while_its_board_is_deleted_goes_with_the_board_or_is_not_found() {
    let test = common::setup().await;
    let store = &test.store;
    let Fixture {
        user_id, team_id, ..
    } = fixture(store).await;

    for round in 0..RACE_ROUNDS {
        let raced_board_id = another_board(store, &team_id, &user_id).await;
        let mut card = Block {
            block_type: BlockType::Card,
            title: "card".to_owned(),
            ..block(&new_id(), &raced_board_id)
        };
        store
            .insert_block(&mut card, &user_id)
            .await
            .expect("a card of the board is inserted");
        let late_id = new_id();
        let mut late = Block {
            parent_id: if round % 2 == 0 {
                card.id.clone()
            } else {
                String::new()
            },
            block_type: BlockType::Text,
            title: "late".to_owned(),
            ..block(&late_id, &raced_board_id)
        };

        let start = Arc::new(Barrier::new(2));
        let deleting = tokio::spawn({
            let (store, start) = (store.clone(), Arc::clone(&start));
            let (board_id, user_id) = (raced_board_id.clone(), user_id.clone());
            async move {
                start.wait().await;
                store.delete_board(&board_id, &user_id).await
            }
        });
        let inserting = tokio::spawn({
            let (store, start) = (store.clone(), Arc::clone(&start));
            let user_id = user_id.clone();
            async move {
                start.wait().await;
                store.insert_block(&mut late, &user_id).await
            }
        });
        let deleted = deleting.await.expect("the deleting task runs to its end");
        let inserted = inserting.await.expect("the inserting task runs to its end");

        assert!(
            deleted.is_ok(),
            "round {round}: the board is deleted, got {deleted:?}"
        );
        let blocks = board_blocks(store, &raced_board_id).await;
        assert!(
            blocks.is_empty(),
            "round {round}: the deleted board has no blocks: {blocks:?}"
        );
        block_in_trash(store, &card.id).await;
        assert_no_block(store, &late_id).await;
        let in_trash = store.get_deleted_block(&late_id).await;
        let expected_back = match &inserted {
            Ok(()) => {
                assert!(
                    in_trash.is_ok(),
                    "round {round}: the block that was stored went into the trash with its \
                     board, got {in_trash:?}"
                );
                vec!["card", "late"]
            }
            Err(error) => {
                assert!(
                    matches!(error, StoreError::NotFound(_)),
                    "round {round}: a block that comes after its board is gone is a \
                     not-found error, got {error:?}"
                );
                assert!(
                    matches!(in_trash, Err(StoreError::NotFound(_))),
                    "round {round}: the block that was refused is not in the trash, got \
                     {in_trash:?}"
                );
                vec!["card"]
            }
        };

        store
            .undelete_board(&raced_board_id, &user_id)
            .await
            .unwrap_or_else(|error| panic!("round {round}: the board is restored: {error:?}"));
        let back = board_blocks(store, &raced_board_id).await;
        assert_eq!(
            sorted_names(&back),
            expected_back,
            "round {round}: the board came back with what went into the trash with it"
        );
    }

    test.teardown().await;
}

/// A patch is applied to the block as the patch before it left it: of two patches at
/// once, one of the title and one of a field, both changes are there afterwards. Neither
/// writes back what it read of the other's part before the other wrote.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_patches_of_a_block_at_once_both_take_effect() {
    let test = common::setup().await;
    let store = &test.store;
    let Fixture {
        user_id, board_id, ..
    } = fixture(store).await;
    let mut card = Block {
        block_type: BlockType::Card,
        title: "before".to_owned(),
        fields: fields(json!({ "kept": "as it is", "mark": "before" })),
        ..block(&new_id(), &board_id)
    };
    store
        .insert_block(&mut card, &user_id)
        .await
        .expect("the card is inserted");

    for round in 0..2 * RACE_ROUNDS {
        let title = format!("title {round}");
        let mark = json!(format!("mark {round}"));
        let patches = [
            title_patch(&title),
            BlockPatch {
                updated_fields: fields(json!({ "mark": mark })),
                ..BlockPatch::default()
            },
        ];

        let start = Arc::new(Barrier::new(2));
        let mut racers = JoinSet::new();
        for patch in patches {
            let (store, start) = (store.clone(), Arc::clone(&start));
            let (card_id, user_id) = (card.id.clone(), user_id.clone());
            racers.spawn(async move {
                start.wait().await;
                store.patch_block(&card_id, &patch, &user_id).await
            });
        }
        for outcome in racers.join_all().await {
            outcome.unwrap_or_else(|error| panic!("round {round}: a patch failed: {error:?}"));
        }

        let stored = existing_block(store, &card.id).await;
        assert_eq!(
            (stored.title.as_str(), stored.fields.get("mark")),
            (title.as_str(), Some(&mark)),
            "round {round}: the title of one patch and the field of the other"
        );
        assert_eq!(
            stored.fields.get("kept"),
            Some(&json!("as it is")),
            "round {round}: what neither patch named is as it was"
        );
    }

    test.teardown().await;
}

/// Asked for the blocks whose parent is the board itself, the store answers with the
/// blocks directly on the board: the ones it stores without a parent. Not the blocks
/// below those, not the blocks of another board, and nothing if the board named as
/// parent is not the board that is asked about.
#[tokio::test]
async fn get_blocks_with_the_board_as_parent_returns_the_blocks_directly_on_the_board() {
    let test = common::setup().await;
    let store = &test.store;
    let Fixture {
        user_id,
        team_id,
        board_id,
    } = fixture(store).await;
    let other_board_id = another_board(store, &team_id, &user_id).await;

    // Two cards and a view at the top, one of the cards written with the board as its
    // parent; a text on a card and a checkbox on the text.
    let mut tree = named_blocks(
        &board_id,
        &[
            ("card", "", "card"),
            ("view", "", "view"),
            ("text", "card", "text"),
            ("checkbox", "text", "checkbox"),
        ],
    );
    common::insert_blocks(store, &mut tree, &user_id).await;
    let mut named_the_board = Block {
        parent_id: board_id.clone(),
        block_type: BlockType::Card,
        title: "second card".to_owned(),
        ..block(&new_id(), &board_id)
    };
    store
        .insert_block(&mut named_the_board, &user_id)
        .await
        .expect("a card that names its board as parent is stored");
    let mut elsewhere = named_blocks(&other_board_id, &[("other card", "", "card")]);
    common::insert_blocks(store, &mut elsewhere, &user_id).await;

    let on_the_board = BlockFilter::board(&board_id).with_parent(&board_id);
    let blocks = store
        .get_blocks(&on_the_board)
        .await
        .expect("the query runs");
    assert_eq!(
        sorted_names(&blocks),
        ["card", "second card", "view"],
        "exactly the blocks directly on the board"
    );
    assert!(
        blocks.iter().all(|block| block.parent_id.is_empty()),
        "none of them has a parent: {blocks:?}"
    );

    // The other filters narrow it as they narrow any list.
    let cards = store
        .get_blocks(&on_the_board.clone().with_type(BlockType::Card))
        .await
        .expect("the query runs");
    assert_eq!(
        sorted_names(&cards),
        ["card", "second card"],
        "of them, the cards"
    );

    // A block as parent still means that block.
    let on_the_card = store
        .get_blocks(&BlockFilter::board(&board_id).with_parent(id_of(&tree, "card")))
        .await
        .expect("the query runs");
    assert_eq!(
        sorted_names(&on_the_card),
        ["text"],
        "the block on the card"
    );

    // Each board has its own, and one board is not the parent of another's blocks.
    let on_the_other = store
        .get_blocks(&BlockFilter::board(&other_board_id).with_parent(&other_board_id))
        .await
        .expect("the query runs");
    assert_eq!(
        sorted_names(&on_the_other),
        ["other card"],
        "the other board's"
    );
    let mixed = store
        .get_blocks(&BlockFilter::board(&board_id).with_parent(&other_board_id))
        .await
        .expect("the query runs");
    assert!(
        mixed.is_empty(),
        "no block of the board is on another board: {mixed:?}"
    );

    test.teardown().await;
}
