//! Port of `server/services/store/storetests/files.go`, followed by tests of what the
//! original suite does not cover.

use std::time::{SystemTime, UNIX_EPOCH};

use fvoci_model::{Block, BlockType, FileInfo, new_id};
use fvoci_store::StoreError;
use serde_json::json;

use crate::common;

fn now_millis() -> i64 {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("the clock is past the epoch")
        .as_millis();
    i64::try_from(millis).expect("the current time fits in 64 bits")
}

/// `StoreTestFileStore`.
#[tokio::test]
async fn file_store() {
    let test = common::setup().await;
    let store = &test.store;
    let user_id = common::user(store, "michael.scott").await;
    let team_id = common::team(store).await;
    let boards = common::create_test_boards(store, &team_id, &user_id, 1).await;
    let board_id = &boards[0].id;

    // "should save and retrieve fileinfo"
    let file_info = FileInfo {
        id: new_id(),
        board_id: board_id.clone(),
        stored_name: "7salesreport.sales".to_owned(),
        create_at: now_millis(),
        name: "Dunder Mifflin Sales Report 2022".to_owned(),
        extension: ".sales".to_owned(),
        size: 112_233,
        delete_at: 0,
        ..FileInfo::default()
    };

    store.save_file_info(&file_info).await.unwrap();

    // The original asks for the record by its identifier; here a file is asked for
    // through its board, by the name it is stored under.
    let retrieved_file_info = store
        .get_file_info(board_id, "7salesreport.sales")
        .await
        .unwrap();
    assert_eq!(retrieved_file_info.id, file_info.id);
    assert_eq!(retrieved_file_info.board_id, *board_id);
    assert_eq!(retrieved_file_info.stored_name, "7salesreport.sales");
    assert_eq!(retrieved_file_info.name, "Dunder Mifflin Sales Report 2022");
    assert_eq!(retrieved_file_info.extension, ".sales");
    assert_eq!(retrieved_file_info.size, 112_233);
    assert_eq!(retrieved_file_info.delete_at, 0);
    assert!(!retrieved_file_info.archived);

    // "should return an error on not found"
    let error = store
        .get_file_info(board_id, "nonexistent")
        .await
        .unwrap_err();
    assert!(matches!(error, StoreError::NotFound(_)), "got {error:?}");

    test.teardown().await;
}

/// Not in the original suite: which members are stored, that a file is always saved as
/// not archived and not deleted, and what a board may hold one of.
#[tokio::test]
async fn file_info_stores_a_subset_of_its_members() {
    let test = common::setup().await;
    let store = &test.store;
    let user_id = common::user(store, "pam.beesly").await;
    let team_id = common::team(store).await;
    let boards = common::create_test_boards(store, &team_id, &user_id, 1).await;
    let board_id = &boards[0].id;

    let file_info = FileInfo {
        id: new_id(),
        board_id: board_id.clone(),
        stored_name: "7abc.png".to_owned(),
        create_at: 1_700_000_000_000,
        delete_at: 1_700_000_000_002,
        path: "boards/20240101/7abc.png".to_owned(),
        name: "picture.png".to_owned(),
        extension: ".png".to_owned(),
        size: 5,
        archived: true,
    };
    store.save_file_info(&file_info).await.unwrap();

    let stored = store.get_file_info(board_id, "7abc.png").await.unwrap();
    assert_eq!(
        stored,
        FileInfo {
            delete_at: 0,
            archived: false,
            ..file_info.clone()
        },
        "everything is stored but the deletion time and the archived flag: a file is \
         saved as neither"
    );

    // A board has one file under a stored name, and a record has one identifier.
    let same_name = FileInfo {
        id: new_id(),
        ..file_info.clone()
    };
    let error = store.save_file_info(&same_name).await.unwrap_err();
    assert!(
        matches!(error, StoreError::Database(_)),
        "a second file of the board under the stored name: got {error:?}"
    );
    let same_id = FileInfo {
        stored_name: "7def.png".to_owned(),
        ..file_info.clone()
    };
    let error = store.save_file_info(&same_id).await.unwrap_err();
    assert!(
        matches!(error, StoreError::Database(_)),
        "a second record with the identifier: got {error:?}"
    );
    let stored_again = store.get_file_info(board_id, "7abc.png").await.unwrap();
    assert_eq!(stored_again, stored, "the refused records changed nothing");

    // The stored name is compared exactly.
    let missing = store.get_file_info(board_id, "7ABC.PNG").await.unwrap_err();
    assert_eq!(missing.to_string(), "{file} not found");

    // Text that is not an identifier names no board: nothing is found under it, and
    // nothing is stored under it.
    let missing = store
        .get_file_info("not-a-board", "7abc.png")
        .await
        .unwrap_err();
    assert!(
        matches!(missing, StoreError::NotFound(_)),
        "got {missing:?}"
    );
    let no_board = FileInfo {
        id: new_id(),
        board_id: "not-a-board".to_owned(),
        ..file_info.clone()
    };
    let error = store.save_file_info(&no_board).await.unwrap_err();
    assert!(
        matches!(&error, StoreError::InvalidId(refused) if refused == "not-a-board"),
        "got {error:?}"
    );
    let no_id = FileInfo {
        id: "not-an-id".to_owned(),
        stored_name: "7ghi.png".to_owned(),
        ..file_info.clone()
    };
    let error = store.save_file_info(&no_id).await.unwrap_err();
    assert!(
        matches!(&error, StoreError::InvalidId(refused) if refused == "not-an-id"),
        "got {error:?}"
    );

    test.teardown().await;
}

/// Not in the original, where a file is found by its name alone: a file is one board's.
/// Another board does not find it, may hold a file of its own under the same stored
/// name, and deleting that board's block deletes that board's file only.
#[tokio::test]
async fn a_file_is_found_through_its_own_board_only() {
    let test = common::setup().await;
    let store = &test.store;
    let user_id = common::user(store, "jim.halpert").await;
    let team_id = common::team(store).await;
    let boards = common::create_test_boards(store, &team_id, &user_id, 3).await;
    let (own, other, third) = (&boards[0].id, &boards[1].id, &boards[2].id);
    let stored_name = "7shared.png";

    let file_of = |board_id: &str, uploaded_as: &str| FileInfo {
        id: new_id(),
        board_id: board_id.to_owned(),
        stored_name: stored_name.to_owned(),
        create_at: now_millis(),
        path: format!("boards/{board_id}/{stored_name}"),
        name: uploaded_as.to_owned(),
        extension: ".png".to_owned(),
        size: 7,
        ..FileInfo::default()
    };

    let own_file = file_of(own, "mine.png");
    store.save_file_info(&own_file).await.unwrap();

    let found = store.get_file_info(own, stored_name).await.unwrap();
    assert_eq!(found, own_file, "the board finds its file");
    for stranger in [other, third] {
        let error = store
            .get_file_info(stranger, stored_name)
            .await
            .unwrap_err();
        assert!(
            matches!(error, StoreError::NotFound(_)),
            "another board does not find the file: got {error:?}"
        );
    }

    // The stored name is taken within a board, not across boards.
    let other_file = file_of(other, "theirs.png");
    store
        .save_file_info(&other_file)
        .await
        .expect("another board holds a file under the same stored name");
    let found = store.get_file_info(own, stored_name).await.unwrap();
    assert_eq!(found, own_file, "each board finds its own file");
    let found = store.get_file_info(other, stored_name).await.unwrap();
    assert_eq!(found, other_file, "each board finds its own file");
    let error = store.get_file_info(third, stored_name).await.unwrap_err();
    assert!(
        matches!(error, StoreError::NotFound(_)),
        "a board without the file still finds none: got {error:?}"
    );

    // Deleting the image block of one board marks that board's file as deleted, and
    // leaves alone the file another board has under the same name.
    let mut image = Block {
        id: new_id(),
        board_id: other.clone(),
        block_type: BlockType::Image,
        created_by: user_id.clone(),
        fields: serde_json::from_value(json!({ "fileId": stored_name }))
            .expect("the fields are a JSON object"),
        ..Block::default()
    };
    store.insert_block(&mut image, &user_id).await.unwrap();
    store.delete_block(&image.id, &user_id).await.unwrap();

    let deleted = store.get_file_info(other, stored_name).await.unwrap();
    assert_ne!(
        deleted.delete_at, 0,
        "the file of the block's board is deleted"
    );
    let kept = store.get_file_info(own, stored_name).await.unwrap();
    assert_eq!(kept, own_file, "the file of the other board is untouched");

    test.teardown().await;
}
