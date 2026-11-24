//! The step of migration 42: retired blocks of type `board` go with what is
//! below them, live and in the trash, of every organization, and what they leave behind
//! with them; the other blocks stay.

use fvoci_model::{Block, BlockType, new_id};
use fvoci_store::{DbType, Store, Tenant};

use crate::common::{self, TestStore};
use crate::setup::save_files;

/// A block of the board, of the type, below the parent (`""` for none).
fn block(board_id: &str, block_type: &str, parent_id: &str, fields: serde_json::Value) -> Block {
    Block {
        id: new_id(),
        container_id: board_id.to_owned(),
        parent_id: parent_id.to_owned(),
        block_type: BlockType::from(block_type),
        fields: serde_json::from_value(fields).expect("the fields are a JSON object"),
        ..Block::default()
    }
}

/// Stores the block and returns it.
async fn stored(store: &Store, mut block: Block, user_id: &str) -> Block {
    common::insert_blocks(store, std::slice::from_mut(&mut block), user_id).await;
    block
}

/// Stores a legacy card directly below a legacy `board` block, bypassing current
/// card-hierarchy validation to seed pre-migration database state.
async fn stored_card_below_legacy(
    test: &TestStore,
    store: &Store,
    board_id: &str,
    legacy_id: &str,
    fields: serde_json::Value,
    user_id: &str,
) -> Block {
    let mut card = stored(store, block(board_id, "card", "", fields), user_id).await;
    common::execute_raw(
        &test.database_connection(),
        &format!(
            "UPDATE blocks SET parent_id = '{legacy_id}' WHERE id = '{}'",
            card.id
        ),
    )
    .await;
    card.parent_id = legacy_id.to_owned();
    card
}

/// A test store with a user and one board: the store, the user and the board.
async fn a_board() -> (TestStore, String, String) {
    let test = common::setup().await;
    let user_id = common::user(&test, "migrated").await;
    let workspace_id = common::workspace(&test).await;
    let boards = common::create_test_boards(&test, &workspace_id, &user_id, 1).await;
    let board_id = boards[0].id.clone();
    (test, user_id, board_id)
}

/// Takes the step as a server that starts after migration 42 does, and returns the
/// store of the test's organization that then connects.
async fn after_the_step(test: &TestStore) -> Store {
    test.store.close().await;
    common::execute_raw(
        &test.database_connection(),
        "INSERT INTO data_steps (name) VALUES ('board_blocks')",
    )
    .await;
    test.reconnect().await
}

/// Asserts that the block is neither on its board nor in the trash.
async fn assert_gone(store: &Store, block: &Block, user_id: &str) {
    let error = store
        .get_block(&block.id)
        .await
        .expect_err("not on the board");
    assert!(error.is_not_found(), "{error:?}");
    let error = store
        .undelete_block(&block.id, user_id)
        .await
        .expect_err("nor in the trash");
    assert!(error.is_not_found(), "{error:?}");
}

/// What is below a board block, two levels deep, goes with it, and so does a board block
/// in the trash with what went there with it; the files only they showed are left to be
/// collected. A card with its attachment, and a card in the trash, stay.
#[tokio::test]
async fn the_step_deletes_them_with_what_is_below_them() {
    let (test, user_id, board) = a_board().await;
    save_files(&test, &board, &["42kept.png", "42gone.png"]).await;
    let none = serde_json::json!({});
    let card = stored(&test, block(&board, "card", "", none.clone()), &user_id).await;
    let file = |name: &str| serde_json::json!({ "fileId": name });
    let attachment = block(&board, "attachment", &card.id, file("42kept.png"));
    let attachment = stored(&test, attachment, &user_id).await;
    let trashed_card = stored(&test, block(&board, "card", "", none.clone()), &user_id).await;
    let legacy = stored(&test, block(&board, "board", "", none.clone()), &user_id).await;
    let below =
        stored_card_below_legacy(&test, &test, &board, &legacy.id, none.clone(), &user_id).await;
    let further = block(&board, "attachment", &below.id, file("42gone.png"));
    let further = stored(&test, further, &user_id).await;
    let trashed = stored(&test, block(&board, "board", "", none.clone()), &user_id).await;
    let trashed_below =
        stored_card_below_legacy(&test, &test, &board, &trashed.id, none, &user_id).await;
    for id in [&trashed_card.id, &trashed.id] {
        test.delete_block(id, &user_id).await.expect("to the trash");
    }

    let store = after_the_step(&test).await;
    store.get_block(&card.id).await.expect("the card stays");
    store
        .get_block(&attachment.id)
        .await
        .expect("its attachment stays");
    for gone in [&legacy, &below, &further, &trashed, &trashed_below] {
        assert_gone(&store, gone, &user_id).await;
    }
    let collected: Vec<String> = store
        .collect_unreferenced_files(crate::blocks::now_millis() + 3_600_000)
        .await
        .expect("the files no block shows are collected")
        .into_iter()
        .map(|file| file.stored_name)
        .collect();
    assert_eq!(collected, ["42gone.png"], "the gone attachment's file only");
    store
        .undelete_block(&trashed_card.id, &user_id)
        .await
        .expect("the card in the trash stays there and comes back");
    store.close().await;
    let left = common::try_execute_raw(&test.database_connection(), "DELETE FROM deleted_blocks")
        .await
        .expect("the trash is there");
    assert_eq!(
        left, 0,
        "the board block in the trash and the card below it left"
    );

    test.teardown().await;
}

/// A card deleted on its own below a live board block, which would have nothing to come
/// back to, leaves the trash with it; and the search server is to be told of the board,
/// though nothing kept the list of what it is to be told when the step was done.
#[tokio::test]
async fn a_block_trashed_below_a_board_block_goes_and_the_board_is_noted_for_the_search() {
    let (test, user_id, board) = a_board().await;
    let none = serde_json::json!({});
    let legacy = stored(&test, block(&board, "board", "", none.clone()), &user_id).await;
    let below =
        stored_card_below_legacy(&test, &test, &board, &legacy.id, none.clone(), &user_id).await;
    let further = stored(&test, block(&board, "card", &below.id, none), &user_id).await;
    test.delete_block(&below.id, &user_id)
        .await
        .expect("the card goes to the trash on its own");
    let noted = test.search_outbox_counts().await.expect("the list is read");
    assert_eq!(noted.waiting, 0, "nothing noted before the step");

    let store = after_the_step(&test).await;
    for gone in [&legacy, &below, &further] {
        assert_gone(&store, gone, &user_id).await;
    }
    let noted = store
        .search_outbox_counts()
        .await
        .expect("the list is read");
    assert_eq!(noted.waiting, 1, "the board is to be sent again");
    store.close().await;
    let left = common::try_execute_raw(&test.database_connection(), "DELETE FROM deleted_blocks")
        .await
        .expect("the trash is there");
    assert_eq!(left, 0, "the card and the one below it left the trash");

    test.teardown().await;
}

/// On PostgreSQL, which holds several organizations, the step goes through every one
/// of them, and names a block by its organization: another organization's board block
/// goes with what is below it, and its card stays, as the first organization's do; and
/// a card of the trash with the identifier of a board block of the trash of another
/// organization (the trash's key is the identifier and the organization) stays.
#[tokio::test]
async fn every_organization_loses_its_board_blocks_and_only_its_own() {
    if common::db_type() != DbType::Postgres {
        return;
    }
    let (test, user_id, board) = a_board().await;
    let (other_org, other_workspace) =
        common::organization(&test.scoped(Tenant::Unset), "another").await;
    let other = test.for_org(&other_org).expect("an identifier");
    let other_boards = common::create_test_boards(&other, &other_workspace, &user_id, 1).await;
    let other_board = &other_boards[0].id;
    let none = serde_json::json!({});
    let mut gone = Vec::new();
    let mut kept = Vec::new();
    for (org_id, store, board) in [
        (&test.org_id, &*test, &board),
        (&other_org, &other, other_board),
    ] {
        let legacy = stored(store, block(board, "board", "", none.clone()), &user_id).await;
        let below =
            stored_card_below_legacy(&test, store, board, &legacy.id, none.clone(), &user_id).await;
        let card = stored(store, block(board, "card", "", none.clone()), &user_id).await;
        gone.push((org_id.clone(), [legacy, below]));
        kept.push((org_id.clone(), card));
    }
    // A board block goes to the first organization's trash; a card of the other takes
    // its identifier, which no live block has then, and goes to the other's trash.
    let trashed = stored(&test, block(&board, "board", "", none.clone()), &user_id).await;
    test.delete_block(&trashed.id, &user_id)
        .await
        .expect("to the trash");
    let same_id = Block {
        id: trashed.id.clone(),
        ..block(other_board, "card", "", none)
    };
    let same_id = stored(&other, same_id, &user_id).await;
    other
        .delete_block(&same_id.id, &user_id)
        .await
        .expect("to the trash");

    let store = after_the_step(&test).await;
    for (org_id, blocks) in &gone {
        let scoped = store.for_org(org_id).expect("an identifier");
        for block in blocks {
            assert_gone(&scoped, block, &user_id).await;
        }
    }
    for (org_id, card) in &kept {
        let scoped = store.for_org(org_id).expect("an identifier");
        scoped.get_block(&card.id).await.expect("the card stays");
    }
    assert_gone(&store, &trashed, &user_id).await;
    let other = store.for_org(&other_org).expect("an identifier");
    other
        .undelete_block(&same_id.id, &user_id)
        .await
        .expect("the other organization's card stays in its trash and comes back");
    store.close().await;

    test.teardown().await;
}
