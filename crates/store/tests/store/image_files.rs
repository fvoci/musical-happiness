//! A write that makes a block show an image asks for the image's file in its own
//! transaction, and the collector of unreferenced files takes no record a block shows.

use std::time::Duration;

use fvoci_model::{
    Block, BlockError, BlockPatch, BlockType, Board, BoardType, BoardsAndBlocks, BodyChange,
    FileInfo, new_id,
};
use fvoci_store::{DbType, DuplicateAs, NewImages, StoreError};
use serde_json::json;

use crate::common;

/// A record of a file of the board, unreferenced since long ago.
fn record_of(board_id: &str, stored_name: &str) -> FileInfo {
    FileInfo {
        id: new_id(),
        board_id: board_id.to_owned(),
        stored_name: stored_name.to_owned(),
        create_at: 1,
        path: format!("boards/{board_id}/{stored_name}"),
        name: "upload.png".to_owned(),
        extension: ".png".to_owned(),
        size: 7,
    }
}

/// A body that shows the files.
fn body_showing(names: &[&str]) -> fvoci_model::Body {
    let mut content =
        vec![json!({"type": "paragraph", "content": [{"type": "text", "text": "x"}]})];
    content.extend(
        names
            .iter()
            .map(|name| json!({"type": "image", "attrs": {"fileId": name}})),
    );
    fvoci_model::Body::parse(json!({"type": "doc", "content": content}))
        .expect("the body may be stored")
        .expect("the body is not empty")
}

/// A card of the board whose body shows the files.
fn card_showing(board_id: &str, names: &[&str]) -> Block {
    Block {
        id: new_id(),
        board_id: board_id.to_owned(),
        block_type: BlockType::Card,
        body: Some(body_showing(names)),
        ..Block::default()
    }
}

fn is_image_without_file(error: &StoreError) -> bool {
    matches!(
        error,
        StoreError::InvalidBlock {
            source: BlockError::ImageWithoutFile,
            ..
        }
    )
}

/// What a client writes may show an image only of a file its board has: a new card, a
/// patch, and a new board's blocks. An image the block showed already is not asked
/// again, nor are the images of a copy, which a duplicate points at copies afterwards.
#[tokio::test]
async fn a_write_that_shows_an_image_newly_needs_the_boards_file() {
    let test = common::setup().await;
    let user_id = common::user(&test, "kelly.kapoor").await;
    let workspace_id = common::workspace(&test).await;
    let boards = common::create_test_boards(&test, &workspace_id, &user_id, 2).await;
    let (board_id, other_board) = (&boards[0].id, &boards[1].id);
    test.save_file_info(&record_of(board_id, "7kept.png"))
        .await
        .expect("a file of the board");
    test.save_file_info(&record_of(other_board, "7other.png"))
        .await
        .expect("a file of another board");

    for name in ["7missing.png", "7other.png"] {
        let mut card = card_showing(board_id, &["7kept.png", name]);
        let error = test
            .insert_block(&mut card, &user_id)
            .await
            .expect_err("an image of no file of the board");
        assert!(is_image_without_file(&error), "{name}: {error:?}");
        assert!(
            test.get_block(&card.id).await.is_err(),
            "{name}: nothing is stored"
        );
    }

    let mut card = card_showing(board_id, &["7kept.png"]);
    common::insert_blocks(&test, std::slice::from_mut(&mut card), &user_id).await;
    let patch = BlockPatch {
        body: BodyChange::Set(body_showing(&["7kept.png", "7missing.png"])),
        ..BlockPatch::default()
    };
    let error = test
        .patch_block(&card.id, &patch, &user_id)
        .await
        .expect_err("a patch that shows an image of no file");
    assert!(is_image_without_file(&error), "{error:?}");

    // The record of a file the card shows goes, as a wrong call of the store could
    // make it: the card is written all the same, and so is its copy.
    test.execute_unchecked(&format!(
        "DELETE FROM file_info WHERE board_id = '{board_id}' AND name = '7kept.png'"
    ))
    .await
    .expect("the record goes");
    let retitled = BlockPatch {
        title: Some("retitled".to_owned()),
        ..BlockPatch::default()
    };
    test.patch_block(&card.id, &retitled, &user_id)
        .await
        .expect("an image the card showed already is not asked again");
    test.duplicate_block(board_id, &card.id, &user_id, DuplicateAs::Copy)
        .await
        .expect("a copy shows what its original shows");

    let new_board = Board {
        id: new_id(),
        workspace_id: workspace_id.clone(),
        board_type: BoardType::Open,
        ..Board::default()
    };
    let new = |new_images| {
        let board = Board {
            id: new_id(),
            ..new_board.clone()
        };
        let card = card_showing(&board.id, &["7archived.png"]);
        (
            BoardsAndBlocks {
                boards: vec![board],
                blocks: vec![card],
            },
            new_images,
        )
    };
    let (mut checked, new_images) = new(NewImages::Recorded);
    let error = test
        .create_boards_and_blocks(&mut checked, &user_id, new_images)
        .await
        .expect_err("a new board has no files");
    assert!(is_image_without_file(&error), "{error:?}");
    let (mut imported, new_images) = new(NewImages::Unchecked);
    test.create_boards_and_blocks(&mut imported, &user_id, new_images)
        .await
        .expect("an import points its images at its files afterwards");

    test.teardown().await;
}

/// A record whose mark says that no block shows it is not collected while one does:
/// a write that showed it as another took the mark away may leave such a mark.
#[tokio::test]
async fn the_collector_takes_no_record_that_a_block_shows() {
    let test = common::setup().await;
    let user_id = common::user(&test, "oscar.martinez").await;
    let workspace_id = common::workspace(&test).await;
    let boards = common::create_test_boards(&test, &workspace_id, &user_id, 1).await;
    let board_id = &boards[0].id;
    test.save_file_info(&record_of(board_id, "7shown.png"))
        .await
        .expect("a file of the board");
    let mut card = card_showing(board_id, &["7shown.png"]);
    common::insert_blocks(&test, std::slice::from_mut(&mut card), &user_id).await;
    test.execute_unchecked("UPDATE file_info SET unreferenced_at = 1")
        .await
        .expect("a mark left behind");

    let collected = test
        .system()
        .collect_unreferenced_files(i64::MAX)
        .await
        .expect("collected");
    assert!(collected.is_empty(), "{collected:?}");
    test.get_file_info(board_id, "7shown.png")
        .await
        .expect("the record stays");

    test.teardown().await;
}

/// PostgreSQL: the lock a write takes on the record of an image it shows newly keeps
/// the collector from deleting the record until the write ends.
#[tokio::test]
async fn the_collector_waits_for_a_write_that_holds_an_images_record() {
    if common::db_type() != DbType::Postgres {
        return;
    }
    let test = common::setup().await;
    let user_id = common::user(&test, "angela.martin").await;
    let workspace_id = common::workspace(&test).await;
    let boards = common::create_test_boards(&test, &workspace_id, &user_id, 1).await;
    let board_id = boards[0].id.clone();
    test.save_file_info(&record_of(&board_id, "7held.png"))
        .await
        .expect("a file of the board");

    // The statement of the write, on a connection of the test's own.
    let mut writer = common::connect_past_the_store(&test.dbconfig()).await;
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "BEGIN; SELECT name FROM file_info WHERE board_id = '{board_id}' \
         AND name IN ('7held.png') FOR KEY SHARE"
    )))
    .execute(&mut writer)
    .await
    .expect("the record is held");
    let system = test.system();
    let collector = tokio::spawn(async move { system.collect_unreferenced_files(i64::MAX).await });
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(
        !collector.is_finished(),
        "the collector waits for the write"
    );
    sqlx::raw_sql("COMMIT")
        .execute(&mut writer)
        .await
        .expect("the write ends");
    let collected = tokio::time::timeout(Duration::from_secs(10), collector)
        .await
        .expect("the collector goes on once the write ended")
        .expect("the collector finishes")
        .expect("collected");
    assert_eq!(collected.len(), 1, "no block shows it: {collected:?}");

    test.teardown().await;
}
