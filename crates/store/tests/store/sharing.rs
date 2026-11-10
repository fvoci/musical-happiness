//! Port of `server/services/store/storetests/sharing.go`, followed by a test of what the
//! original suite does not cover.

use fvoci_model::{Sharing, new_id};
use fvoci_store::StoreError;

use crate::common;

/// `testUpsertSharingAndGetSharing`.
#[tokio::test]
async fn upsert_sharing_and_get_sharing() {
    let test = common::setup().await;
    let store = &test.store;
    // `testUserID` of the original suite, and the second user it names.
    let user_id = common::user(store, "user-id").await;
    let user_id2 = common::user(store, "user-id2").await;
    let team_id = common::team(store).await;
    let boards = common::create_test_boards(store, &team_id, &user_id, 1).await;
    // A sharing record carries the identifier of the board it shares.
    let sharing_id = &boards[0].id;

    // "Insert first sharing and get it"
    let sharing = Sharing {
        id: sharing_id.clone(),
        enabled: true,
        token: "token".to_owned(),
        modified_by: user_id,
        ..Sharing::default()
    };

    store.upsert_sharing(&sharing).await.unwrap();
    let mut new_sharing = store.get_sharing(sharing_id).await.unwrap();
    new_sharing.update_at = 0;
    assert_eq!(new_sharing, sharing);

    // "Upsert the inserted sharing and get it"
    let sharing = Sharing {
        id: sharing_id.clone(),
        enabled: true,
        token: "token2".to_owned(),
        modified_by: user_id2,
        ..Sharing::default()
    };

    let mut new_sharing = store.get_sharing(sharing_id).await.unwrap();
    new_sharing.update_at = 0;
    assert_ne!(new_sharing, sharing);

    store.upsert_sharing(&sharing).await.unwrap();
    let mut new_sharing = store.get_sharing(sharing_id).await.unwrap();
    new_sharing.update_at = 0;
    assert_eq!(new_sharing, sharing);

    // "Get not existing sharing"
    let error = store.get_sharing(&new_id()).await.unwrap_err();
    assert!(error.is_not_found(), "got {error:?}");

    test.teardown().await;
}

/// Not in the original suite: the store stamps the modification time itself, sharing can
/// be switched off, a board that was never shared has the original's error text, and
/// text that is not an identifier names no board.
#[tokio::test]
async fn sharing_is_stamped_and_can_be_disabled() {
    let test = common::setup().await;
    let store = &test.store;
    let user_id = common::user(store, "sharer").await;
    let team_id = common::team(store).await;
    let boards = common::create_test_boards(store, &team_id, &user_id, 2).await;
    let (board_id, other_board_id) = (&boards[0].id, &boards[1].id);

    let mut sharing = Sharing {
        id: board_id.clone(),
        enabled: true,
        token: "token".to_owned(),
        modified_by: user_id.clone(),
        update_at: 5,
    };
    store.upsert_sharing(&sharing).await.unwrap();
    let stored = store.get_sharing(board_id).await.unwrap();
    assert!(stored.update_at > 5, "the given time is not used");
    assert!(stored.enabled, "got {stored:?}");

    // Switched off by nobody in particular: the modifier reads as empty.
    sharing.enabled = false;
    sharing.modified_by = String::new();
    store.upsert_sharing(&sharing).await.unwrap();
    let stored = store.get_sharing(board_id).await.unwrap();
    assert!(!stored.enabled, "got {stored:?}");
    assert_eq!(stored.token, "token");
    assert_eq!(stored.modified_by, "", "no user is stored as none");

    let missing = store.get_sharing(other_board_id).await.unwrap_err();
    assert_eq!(missing.to_string(), "sql: no rows in result set");

    // Text that is not an identifier: nothing is found under it, and nothing is stored
    // under it or on behalf of it.
    let missing = store.get_sharing("not-a-board").await.unwrap_err();
    assert!(matches!(missing, StoreError::NoRows), "got {missing:?}");
    let no_board = Sharing {
        id: "not-a-board".to_owned(),
        ..sharing.clone()
    };
    let error = store.upsert_sharing(&no_board).await.unwrap_err();
    assert!(
        matches!(&error, StoreError::InvalidId(refused) if refused == "not-a-board"),
        "got {error:?}"
    );
    let no_user = Sharing {
        id: other_board_id.clone(),
        modified_by: "not-a-user".to_owned(),
        ..sharing.clone()
    };
    let error = store.upsert_sharing(&no_user).await.unwrap_err();
    assert!(
        matches!(&error, StoreError::InvalidId(refused) if refused == "not-a-user"),
        "got {error:?}"
    );
    let missing = store.get_sharing(other_board_id).await.unwrap_err();
    assert!(matches!(missing, StoreError::NoRows), "got {missing:?}");

    test.teardown().await;
}
