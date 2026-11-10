//! Port of `server/services/store/storetests/system.go`, followed by a test of what the
//! original suite does not cover.

use std::collections::BTreeMap;

use crate::common;

/// `dataMigrationSystemSettings`: the settings a new database starts with.
const DATA_MIGRATION_SYSTEM_SETTINGS: [(&str, &str); 3] = [
    ("UniqueIDsMigrationComplete", "true"),
    ("CategoryUuidIdMigrationComplete", "true"),
    ("DeDuplicateCategoryBoardTableComplete", "true"),
];

/// `addBaseSettings`.
fn add_base_settings(settings: &[(&str, &str)]) -> BTreeMap<String, String> {
    DATA_MIGRATION_SYSTEM_SETTINGS
        .iter()
        .chain(settings)
        .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
        .collect()
}

/// `testSetGetSystemSettings`.
#[tokio::test]
async fn set_get_system_settings() {
    let test = common::setup().await;
    let store = &test.store;

    // "Get empty settings"
    let settings = store.get_system_settings().await.unwrap();
    assert_eq!(settings, add_base_settings(&[]));

    // "Set, update and get multiple settings"
    store
        .set_system_setting("test-1", "test-value-1")
        .await
        .unwrap();
    store
        .set_system_setting("test-2", "test-value-2")
        .await
        .unwrap();
    let settings = store.get_system_settings().await.unwrap();
    assert_eq!(
        settings,
        add_base_settings(&[("test-1", "test-value-1"), ("test-2", "test-value-2")])
    );

    store
        .set_system_setting("test-2", "test-value-updated-2")
        .await
        .unwrap();
    let settings = store.get_system_settings().await.unwrap();
    assert_eq!(
        settings,
        add_base_settings(&[
            ("test-1", "test-value-1"),
            ("test-2", "test-value-updated-2")
        ])
    );

    // "Get a single setting"
    let value = store.get_system_setting("test-1").await.unwrap();
    assert_eq!(value.as_deref(), Some("test-value-1"));

    test.teardown().await;
}

/// Not in the original suite: a setting that was never set is absent, not an error, and
/// differs from one set to the empty string.
#[tokio::test]
async fn a_missing_system_setting_is_absent() {
    let test = common::setup().await;
    let store = &test.store;

    let missing = store.get_system_setting("never-set").await.unwrap();
    assert_eq!(missing, None);

    store.set_system_setting("empty", "").await.unwrap();
    let empty = store.get_system_setting("empty").await.unwrap();
    assert_eq!(empty.as_deref(), Some(""));

    test.teardown().await;
}
