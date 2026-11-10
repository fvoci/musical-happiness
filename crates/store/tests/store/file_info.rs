//! Port of `server/services/store/storetests/files.go`, followed by tests of what the
//! original suite does not cover: a file record is one board's, and is there for as long
//! as its board is.

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

    // "should return an error on not found"
    let error = store
        .get_file_info(board_id, "nonexistent")
        .await
        .unwrap_err();
    assert!(matches!(error, StoreError::NotFound(_)), "got {error:?}");

    test.teardown().await;
}

/// Not in the original suite: every member of a record is stored, and what a board may
/// hold one of.
#[tokio::test]
async fn file_info_stores_every_member() {
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
        path: "boards/20240101/7abc.png".to_owned(),
        name: "picture.png".to_owned(),
        extension: ".png".to_owned(),
        size: 5,
    };
    store.save_file_info(&file_info).await.unwrap();

    let stored = store.get_file_info(board_id, "7abc.png").await.unwrap();
    assert_eq!(
        stored, file_info,
        "every member comes back as it was saved, the given creation time included"
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
/// Another board does not find it and may hold a file of its own under the same stored
/// name. Deleting the block that shows a file leaves the records of both boards.
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

    // Deleting the image block of one board leaves both records as they are: nothing
    // marks a file record deleted.
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

    let kept = store.get_file_info(other, stored_name).await.unwrap();
    assert_eq!(
        kept, other_file,
        "the file record of the block's board outlives the block"
    );
    let kept = store.get_file_info(own, stored_name).await.unwrap();
    assert_eq!(kept, own_file, "the file of the other board is untouched");

    test.teardown().await;
}

/// A file record of the board under the stored name, as an upload makes it.
fn record_of(board_id: &str, stored_name: &str) -> FileInfo {
    FileInfo {
        id: new_id(),
        board_id: board_id.to_owned(),
        stored_name: stored_name.to_owned(),
        create_at: now_millis(),
        path: format!("boards/{board_id}/{stored_name}"),
        name: "upload.png".to_owned(),
        extension: ".png".to_owned(),
        size: 7,
    }
}

/// Not in the original, whose file records name boards that need not exist. A file is
/// one board's: a record for a board that does not exist is refused over its foreign
/// key, and nothing is stored.
#[tokio::test]
async fn a_file_record_for_a_board_that_does_not_exist_is_refused() {
    let test = common::setup().await;
    let store = &test.store;
    let user_id = common::user(store, "kelly.kapoor").await;
    let team_id = common::team(store).await;
    let boards = common::create_test_boards(store, &team_id, &user_id, 1).await;

    let no_board = new_id();
    let record = record_of(&no_board, "7orphan.png");
    let error = store
        .save_file_info(&record)
        .await
        .expect_err("a record for a board that does not exist is refused");
    assert!(common::refused_over_a_foreign_key(&error), "got {error:?}");

    for board_id in [&no_board, &boards[0].id] {
        let error = store
            .get_file_info(board_id, "7orphan.png")
            .await
            .expect_err("the refused record is not stored");
        assert!(
            matches!(error, StoreError::NotFound(_)),
            "under board {board_id}: got {error:?}"
        );
    }

    // The same record for a board that exists is stored.
    let record = FileInfo {
        board_id: boards[0].id.clone(),
        ..record
    };
    store
        .save_file_info(&record)
        .await
        .expect("a record for a board that exists is stored");

    test.teardown().await;
}

/// Not in the original, where a stored name is one file of the whole server. A board
/// has one file under a stored name: a second record is refused as a duplicate and
/// changes nothing. Two boards may each have a file under the same name, and each finds
/// its own.
#[tokio::test]
async fn a_stored_name_is_taken_once_per_board() {
    let test = common::setup().await;
    let store = &test.store;
    let user_id = common::user(store, "oscar.martinez").await;
    let team_id = common::team(store).await;
    let boards = common::create_test_boards(store, &team_id, &user_id, 2).await;
    let (one, other) = (&boards[0].id, &boards[1].id);
    let stored_name = "7ledger.png";

    let first = record_of(one, stored_name);
    store
        .save_file_info(&first)
        .await
        .expect("the board's first file under the name is stored");

    let second = FileInfo {
        name: "another upload.png".to_owned(),
        size: 9,
        ..record_of(one, stored_name)
    };
    let error = store
        .save_file_info(&second)
        .await
        .expect_err("a second record of the board under the name is refused");
    assert!(
        matches!(
            &error,
            StoreError::Database(sqlx::Error::Database(refusal)) if refusal.is_unique_violation()
        ),
        "the refusal is the database's, of a duplicate: got {error:?}"
    );
    let found = store
        .get_file_info(one, stored_name)
        .await
        .expect("the board still finds its file");
    assert_eq!(found, first, "the refused record changed nothing");

    let of_other = FileInfo {
        name: "theirs.png".to_owned(),
        ..record_of(other, stored_name)
    };
    store
        .save_file_info(&of_other)
        .await
        .expect("another board has a file of its own under the same name");
    for (board_id, expected) in [(one, &first), (other, &of_other)] {
        let found = store
            .get_file_info(board_id, stored_name)
            .await
            .expect("each board finds a file under the name");
        assert_eq!(&found, expected, "board {board_id} finds its own file");
    }

    test.teardown().await;
}

/// A file record goes with its board: while the board is in the trash the record is not
/// found, and it comes back, unchanged, when the board is restored. The record of
/// another board stays throughout.
#[tokio::test]
async fn a_file_record_goes_with_its_board_and_comes_back_with_it() {
    let test = common::setup().await;
    let store = &test.store;
    let user_id = common::user(store, "angela.martin").await;
    let team_id = common::team(store).await;
    let boards = common::create_test_boards(store, &team_id, &user_id, 2).await;
    let (doomed, other) = (&boards[0].id, &boards[1].id);
    let stored_name = "7party.png";

    let records = [
        record_of(doomed, stored_name),
        record_of(doomed, "7second.png"),
    ];
    for record in &records {
        store
            .save_file_info(record)
            .await
            .expect("a file of the board is stored");
    }
    let of_other = record_of(other, stored_name);
    store
        .save_file_info(&of_other)
        .await
        .expect("a file of the other board is stored");

    store
        .delete_board(doomed, &user_id)
        .await
        .expect("the board is deleted");
    for record in &records {
        let error = store
            .get_file_info(doomed, &record.stored_name)
            .await
            .expect_err("the files of a deleted board are not found");
        assert!(
            matches!(error, StoreError::NotFound(_)),
            "{}: got {error:?}",
            record.stored_name
        );
    }
    let kept = store
        .get_file_info(other, stored_name)
        .await
        .expect("the other board still finds its file");
    assert_eq!(kept, of_other, "the file of the other board is untouched");

    store
        .undelete_board(doomed, &user_id)
        .await
        .expect("the board is restored");
    for record in &records {
        let back = store
            .get_file_info(doomed, &record.stored_name)
            .await
            .expect("the files come back with their board");
        assert_eq!(&back, record, "the record is as it was, its place included");
    }
    let kept = store
        .get_file_info(other, stored_name)
        .await
        .expect("the other board still finds its file");
    assert_eq!(kept, of_other, "the file of the other board is untouched");

    test.teardown().await;
}
