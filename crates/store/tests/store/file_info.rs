//! Port of `server/services/store/storetests/files.go`, followed by a test of what the
//! original suite does not cover.

use std::time::{SystemTime, UNIX_EPOCH};

use fvoci_model::FileInfo;
use fvoci_store::StoreError;

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

    // "should save and retrieve fileinfo"
    let file_info = FileInfo {
        id: "file_info_1".to_owned(),
        create_at: now_millis(),
        name: "Dunder Mifflin Sales Report 2022".to_owned(),
        extension: ".sales".to_owned(),
        size: 112_233,
        delete_at: 0,
        ..FileInfo::default()
    };

    store.save_file_info(&file_info).await.unwrap();

    let retrieved_file_info = store.get_file_info("file_info_1").await.unwrap();
    assert_eq!(retrieved_file_info.id, "file_info_1");
    assert_eq!(retrieved_file_info.name, "Dunder Mifflin Sales Report 2022");
    assert_eq!(retrieved_file_info.extension, ".sales");
    assert_eq!(retrieved_file_info.size, 112_233);
    assert_eq!(retrieved_file_info.delete_at, 0);
    assert!(!retrieved_file_info.archived);

    // "should return an error on not found"
    let error = store.get_file_info("nonexistent").await.unwrap_err();
    assert!(matches!(error, StoreError::NotFound(_)), "got {error:?}");

    test.teardown().await;
}

/// Not in the original suite: which members are stored, and that a file is always saved
/// as not archived.
#[tokio::test]
async fn file_info_stores_a_subset_of_its_members() {
    let test = common::setup().await;
    let store = &test.store;

    let file_info = FileInfo {
        id: "file_info_2".to_owned(),
        creator_id: "boards".to_owned(),
        create_at: 1_700_000_000_000,
        update_at: 1_700_000_000_001,
        delete_at: 1_700_000_000_002,
        path: "boards/20240101/7abc.png".to_owned(),
        name: "picture.png".to_owned(),
        extension: ".png".to_owned(),
        size: 5,
        mime_type: "image/png".to_owned(),
        archived: true,
    };
    store.save_file_info(&file_info).await.unwrap();

    let stored = store.get_file_info("file_info_2").await.unwrap();
    assert_eq!(
        stored,
        FileInfo {
            creator_id: String::new(),
            update_at: 0,
            mime_type: String::new(),
            archived: false,
            ..file_info.clone()
        },
        "the creator, the update time and the media type are not stored"
    );

    // The table has no key: saving the same identifier again adds a second row.
    store.save_file_info(&file_info).await.unwrap();
    assert!(store.get_file_info("file_info_2").await.is_ok());

    let missing = store.get_file_info("FILE_INFO_2").await.unwrap_err();
    assert_eq!(missing.to_string(), "{file info ID=FILE_INFO_2} not found");

    test.teardown().await;
}
