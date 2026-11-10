//! Port of `server/services/store/storetests/sharing.go`, followed by a test of what the
//! original suite does not cover.

use fvoci_model::Sharing;

use crate::common;

/// `testUserID` of the original suite.
const TEST_USER_ID: &str = "user-id";

/// `testUpsertSharingAndGetSharing`.
#[tokio::test]
async fn upsert_sharing_and_get_sharing() {
    let test = common::setup().await;
    let store = &test.store;

    // "Insert first sharing and get it"
    let sharing = Sharing {
        id: "sharing-id".to_owned(),
        enabled: true,
        token: "token".to_owned(),
        modified_by: TEST_USER_ID.to_owned(),
        ..Sharing::default()
    };

    store.upsert_sharing(&sharing).await.unwrap();
    let mut new_sharing = store.get_sharing("sharing-id").await.unwrap();
    new_sharing.update_at = 0;
    assert_eq!(new_sharing, sharing);

    // "Upsert the inserted sharing and get it"
    let sharing = Sharing {
        id: "sharing-id".to_owned(),
        enabled: true,
        token: "token2".to_owned(),
        modified_by: "user-id2".to_owned(),
        ..Sharing::default()
    };

    let mut new_sharing = store.get_sharing("sharing-id").await.unwrap();
    new_sharing.update_at = 0;
    assert_ne!(new_sharing, sharing);

    store.upsert_sharing(&sharing).await.unwrap();
    let mut new_sharing = store.get_sharing("sharing-id").await.unwrap();
    new_sharing.update_at = 0;
    assert_eq!(new_sharing, sharing);

    // "Get not existing sharing"
    let error = store.get_sharing("not-existing").await.unwrap_err();
    assert!(error.is_not_found(), "got {error:?}");

    test.teardown().await;
}

/// Not in the original suite: the store stamps the modification time itself, sharing can
/// be switched off, and a missing sharing has the original's error text.
#[tokio::test]
async fn sharing_is_stamped_and_can_be_disabled() {
    let test = common::setup().await;
    let store = &test.store;

    let mut sharing = Sharing {
        id: "board-id".to_owned(),
        enabled: true,
        token: "token".to_owned(),
        modified_by: TEST_USER_ID.to_owned(),
        update_at: 5,
    };
    store.upsert_sharing(&sharing).await.unwrap();
    let stored = store.get_sharing("board-id").await.unwrap();
    assert!(stored.update_at > 5, "the given time is not used");
    assert!(stored.enabled, "got {stored:?}");

    sharing.enabled = false;
    store.upsert_sharing(&sharing).await.unwrap();
    let stored = store.get_sharing("board-id").await.unwrap();
    assert!(!stored.enabled, "got {stored:?}");
    assert_eq!(stored.token, "token");

    let missing = store.get_sharing("other-board").await.unwrap_err();
    assert_eq!(missing.to_string(), "sql: no rows in result set");

    test.teardown().await;
}
