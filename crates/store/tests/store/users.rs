//! Port of `server/services/store/storetests/users.go`, followed by tests of the user
//! operations that the original suite does not cover.

use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

use fvoci_model::{IdKind, User, UserPreferencesPatch, new_id};
use fvoci_store::{DbType, Preference, Store, StoreError};

use crate::common;

/// The count the original derives from the clock: `time.Now().Unix() % 10`.
pub(crate) fn clock_count() -> usize {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("the clock is past the epoch")
        .as_secs();
    usize::try_from(seconds % 10).expect("a value below ten fits")
}

fn now_millis() -> i64 {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("the clock is past the epoch")
        .as_millis();
    i64::try_from(millis).expect("the current time fits in 64 bits")
}

async fn create_user(store: &Store, mut user: User) -> User {
    store
        .create_user(&mut user)
        .await
        .expect("the user is created");
    user
}

fn value_of<'p>(preferences: &'p [Preference], name: &str) -> Option<&'p str> {
    preferences
        .iter()
        .find(|preference| preference.name == name)
        .map(|preference| preference.value.as_str())
}

/// `testGetUsersByTeam`.
#[tokio::test]
async fn get_users_by_team() {
    let test = common::setup().await;
    let store = &test.store;

    // "GetTeamUsers"
    // The original passes a team, a guest and two visibility flags, all of which its
    // standalone store ignores; this store does not take them.
    let users = store.get_users_by_team().await.unwrap();
    assert_eq!(users.len(), 0);

    let user_id = new_id(IdKind::User);

    let mut user = User {
        id: user_id.clone(),
        username: "darth.vader".to_owned(),
        ..User::default()
    };
    store.create_user(&mut user).await.unwrap();
    assert_eq!(user.id, user_id);
    assert_eq!(user.username, "darth.vader");

    let users = store.get_users_by_team().await.unwrap();
    assert_eq!(users.len(), 1);
    assert_eq!(users[0].username, "darth.vader");

    // The original defers this call and ignores its result. It does not delete the user:
    // an update writes only the user name and the email address.
    let _ = store
        .update_user(&mut User {
            id: user_id,
            delete_at: now_millis(),
            ..User::default()
        })
        .await;

    test.teardown().await;
}

/// `testCreateAndGetUser`.
#[tokio::test]
async fn create_and_get_user() {
    let test = common::setup().await;
    let store = &test.store;

    let mut user = User {
        id: new_id(IdKind::User),
        username: "damao".to_owned(),
        email: "mock@email.com".to_owned(),
        ..User::default()
    };

    // "CreateUser"
    store.create_user(&mut user).await.unwrap();

    // "GetUserByID"
    let got = store.get_user_by_id(&user.id).await.unwrap();
    assert_eq!(got.id, user.id);
    assert_eq!(got.username, user.username);
    assert_eq!(got.email, user.email);

    // "GetUserByID nonexistent"
    let error = store.get_user_by_id("nonexistent-id").await.unwrap_err();
    assert!(matches!(error, StoreError::NotFound(_)), "got {error:?}");

    // "GetUserByUsername"
    let got = store.get_user_by_username(&user.username).await.unwrap();
    assert_eq!(got.id, user.id);
    assert_eq!(got.username, user.username);
    assert_eq!(got.email, user.email);

    // "GetUserByUsername nonexistent"
    // The original looks the name up as an identifier here; kept as written.
    let error = store
        .get_user_by_id("nonexistent-username")
        .await
        .unwrap_err();
    assert!(matches!(error, StoreError::NotFound(_)), "got {error:?}");

    // "GetUserByEmail"
    let got = store.get_user_by_email(&user.email).await.unwrap();
    assert_eq!(got.id, user.id);
    assert_eq!(got.username, user.username);
    assert_eq!(got.email, user.email);

    // "GetUserByEmail nonexistent"
    // The original looks the address up as an identifier here; kept as written.
    let error = store.get_user_by_id("nonexistent-email").await.unwrap_err();
    assert!(matches!(error, StoreError::NotFound(_)), "got {error:?}");

    test.teardown().await;
}

/// One case of `testGetUsersList`.
struct UsersListCase {
    name: &'static str,
    user_ids: [&'static str; 2],
    expected_error: bool,
    expected_ids: &'static [&'static str],
}

/// `testGetUsersList`.
#[tokio::test]
async fn get_users_list() {
    let test = common::setup().await;
    let store = &test.store;

    for id in ["user1", "user2"] {
        let mut user = User {
            id: id.to_owned(),
            username: format!("{id}-username"),
            email: format!("{id}@sample.com"),
            ..User::default()
        };
        store.create_user(&mut user).await.unwrap();
    }

    let test_cases = [
        UsersListCase {
            name: "all of the IDs are found",
            user_ids: ["user1", "user2"],
            expected_error: false,
            expected_ids: &["user1", "user2"],
        },
        UsersListCase {
            name: "some of the IDs are found",
            user_ids: ["user2", "non-existent"],
            expected_error: true,
            expected_ids: &["user2"],
        },
        UsersListCase {
            name: "none of the IDs are found",
            user_ids: ["non-existent-1", "non-existent-2"],
            expected_error: true,
            expected_ids: &[],
        },
    ];

    for tc in test_cases {
        // The subtest named `tc.name`.
        let name = tc.name;
        let user_ids = tc.user_ids.map(str::to_owned);
        let result = store.get_users_list(&user_ids).await;
        if tc.expected_error {
            let error = result.as_ref().expect_err(name);
            assert!(error.is_not_found(), "{name}: got {error:?}");
        } else {
            assert!(result.is_ok(), "{name}: got {result:?}");
        }

        let mut found: Vec<String> = if let Ok(users) = result {
            users.into_iter().map(|user| user.id).collect()
        } else {
            // The original hands back the users it did find together with the error, and
            // the suite compares them with the expected identifiers. A `Result` carries
            // one or the other, so the same expectation is checked by asking for each
            // identifier on its own: exactly the expected ones are found.
            let mut found = Vec::new();
            for id in &user_ids {
                if let Ok(users) = store.get_users_list(std::slice::from_ref(id)).await {
                    found.extend(users.into_iter().map(|user| user.id));
                }
            }
            found
        };
        found.sort_unstable();
        assert_eq!(found, tc.expected_ids, "{name}");
    }

    test.teardown().await;
}

/// `testCreateAndUpdateUser`.
#[tokio::test]
async fn create_and_update_user() {
    let test = common::setup().await;
    let store = &test.store;

    let mut user = User {
        id: new_id(IdKind::User),
        ..User::default()
    };
    store.create_user(&mut user).await.unwrap();

    // "UpdateUser"
    user.username = "damao".to_owned();
    user.email = "mock@email.com".to_owned();
    // The original returns the user it was given; here that user is updated in place.
    store.update_user(&mut user).await.unwrap();
    assert_eq!(user.username, "damao");
    assert_eq!(user.email, "mock@email.com");

    let got = store.get_user_by_id(&user.id).await.unwrap();
    assert_eq!(got.id, user.id);
    assert_eq!(got.username, user.username);
    assert_eq!(got.email, user.email);

    // "UpdateUserPassword"
    let new_password = new_id(IdKind::None);
    store
        .update_user_password(&user.username, &new_password)
        .await
        .unwrap();

    let got = store.get_user_by_username(&user.username).await.unwrap();
    assert_eq!(got.username, user.username);
    assert_eq!(got.password, new_password);

    // "UpdateUserPasswordByID"
    let new_password = new_id(IdKind::None);
    store
        .update_user_password_by_id(&user.id, &new_password)
        .await
        .unwrap();

    let got = store.get_user_by_id(&user.id).await.unwrap();
    assert_eq!(got.id, user.id);
    assert_eq!(got.password, new_password);

    test.teardown().await;
}

/// `testCreateAndGetRegisteredUserCount`.
#[tokio::test]
async fn create_and_get_registered_user_count() {
    let test = common::setup().await;
    let store = &test.store;

    let random_n = clock_count();
    for _ in 0..random_n {
        let mut user = User {
            id: new_id(IdKind::User),
            ..User::default()
        };
        store.create_user(&mut user).await.unwrap();
    }

    let got = store.get_registered_user_count().await.unwrap();
    assert_eq!(usize::try_from(got).ok(), Some(random_n));

    test.teardown().await;
}

/// `testPatchUserProps`.
#[tokio::test]
async fn patch_user_props() {
    let test = common::setup().await;
    let store = &test.store;

    let mut user = User {
        id: new_id(IdKind::User),
        ..User::default()
    };
    store.create_user(&mut user).await.unwrap();

    let key1 = "new_key_1";
    let key2 = "new_key_2";
    let key3 = "new_key_3";
    let fields = |pairs: &[(&str, &str)]| -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect()
    };

    // Only update props
    let patch = UserPreferencesPatch {
        updated_fields: fields(&[
            (key1, "new_value_1"),
            (key2, "new_value_2"),
            (key3, "new_value_3"),
        ]),
        ..UserPreferencesPatch::default()
    };

    let user_preferences = store
        .patch_user_preferences(&user.id, &patch)
        .await
        .unwrap();
    assert_eq!(user_preferences.len(), 3);

    for preference in &user_preferences {
        match preference.name.as_str() {
            name if name == key1 => assert_eq!(preference.value, "new_value_1"),
            name if name == key2 => assert_eq!(preference.value, "new_value_2"),
            name if name == key3 => assert_eq!(preference.value, "new_value_3"),
            _ => {}
        }
    }

    // Delete a prop
    let patch = UserPreferencesPatch {
        deleted_fields: vec![key1.to_owned()],
        ..UserPreferencesPatch::default()
    };

    let user_preferences = store
        .patch_user_preferences(&user.id, &patch)
        .await
        .unwrap();

    for preference in &user_preferences {
        match preference.name.as_str() {
            name if name == key1 => {
                panic!("new_key_1 shouldn't exist in user preference as we just deleted it")
            }
            name if name == key2 => assert_eq!(preference.value, "new_value_2"),
            name if name == key3 => assert_eq!(preference.value, "new_value_3"),
            _ => {}
        }
    }

    // update and delete together
    let patch = UserPreferencesPatch {
        updated_fields: fields(&[(key3, "new_value_3_new_again")]),
        deleted_fields: vec![key2.to_owned()],
    };
    let user_preferences = store
        .patch_user_preferences(&user.id, &patch)
        .await
        .unwrap();

    for preference in &user_preferences {
        match preference.name.as_str() {
            name if name == key1 => {
                panic!("new_key_1 shouldn't exist in user preference as we just deleted it")
            }
            name if name == key2 => {
                panic!("new_key_2 shouldn't exist in user preference as we just deleted it")
            }
            name if name == key3 => assert_eq!(preference.value, "new_value_3_new_again"),
            _ => {}
        }
    }

    test.teardown().await;
}

// The tests below are not in the original suite. They pin behavior of the original's
// standalone store that its suite leaves untested.

/// What a created user gets stamped with, and which members a read fills in.
#[tokio::test]
async fn create_user_stamps_times_and_reads_back_the_stored_members() {
    let test = common::setup().await;
    let store = &test.store;

    let before = now_millis();
    let mut user = User {
        id: new_id(IdKind::User),
        username: "leia".to_owned(),
        email: "leia@example.com".to_owned(),
        nickname: "not stored".to_owned(),
        password: "hash".to_owned(),
        mfa_secret: "secret".to_owned(),
        auth_service: "native".to_owned(),
        auth_data: "data".to_owned(),
        create_at: 1,
        update_at: 2,
        delete_at: 3,
        ..User::default()
    };
    store.create_user(&mut user).await.unwrap();
    assert!(user.create_at >= before, "create_at is the current time");
    assert_eq!(user.update_at, user.create_at);
    assert_eq!(user.delete_at, 0, "a new user is never deleted");

    let got = store.get_user_by_id(&user.id).await.unwrap();
    let stored = User {
        nickname: String::new(),
        ..user.clone()
    };
    assert_eq!(
        got, stored,
        "everything but the unstored members reads back"
    );

    // The identifier is the only unique member.
    let duplicate = store.create_user(&mut user.clone()).await.unwrap_err();
    assert!(
        matches!(duplicate, StoreError::Database(_)),
        "got {duplicate:?}"
    );
    let mut namesake = User {
        id: new_id(IdKind::User),
        ..user.clone()
    };
    store.create_user(&mut namesake).await.unwrap();
    assert_eq!(store.get_registered_user_count().await.unwrap(), 2);

    test.teardown().await;
}

/// Lookups compare user names and email addresses exactly on both databases.
#[tokio::test]
async fn user_lookups_are_case_sensitive() {
    let test = common::setup().await;
    let store = &test.store;

    create_user(
        store,
        User {
            id: new_id(IdKind::User),
            username: "Han.Solo".to_owned(),
            email: "Han@Example.com".to_owned(),
            ..User::default()
        },
    )
    .await;

    assert!(store.get_user_by_username("Han.Solo").await.is_ok());
    assert!(store.get_user_by_email("Han@Example.com").await.is_ok());
    let by_name = store.get_user_by_username("han.solo").await.unwrap_err();
    assert!(by_name.is_not_found(), "got {by_name:?}");
    let by_email = store
        .get_user_by_email("han@example.com")
        .await
        .unwrap_err();
    assert!(by_email.is_not_found(), "got {by_email:?}");
    assert_eq!(by_name.to_string(), "{user} not found");

    test.teardown().await;
}

/// The search is a `LIKE` on the user name: a substring match of at most ten users, in
/// which `%` and `_` are wildcards. SQLite ignores ASCII case, PostgreSQL does not.
#[tokio::test]
async fn search_users_by_team_matches_substrings_of_the_user_name() {
    let test = common::setup().await;
    let store = &test.store;

    let found = store.search_users_by_team("anything").await.unwrap();
    assert!(found.is_empty(), "no users, no error: {found:?}");

    for index in 0..12 {
        create_user(
            store,
            User {
                id: new_id(IdKind::User),
                username: format!("Trooper.{index}"),
                email: format!("search.{index}@example.com"),
                ..User::default()
            },
        )
        .await;
    }
    create_user(
        store,
        User {
            id: new_id(IdKind::User),
            username: "yoda".to_owned(),
            ..User::default()
        },
    )
    .await;

    let found = store.search_users_by_team("oop").await.unwrap();
    assert_eq!(found.len(), 10, "at most ten of the twelve matches");
    assert!(
        found
            .iter()
            .all(|user| user.username.starts_with("Trooper.")),
        "got {found:?}"
    );

    let found = store.search_users_by_team("Trooper.11").await.unwrap();
    assert_eq!(found.len(), 1);

    let found = store.search_users_by_team("").await.unwrap();
    assert_eq!(found.len(), 10, "an empty query matches every user");

    let found = store.search_users_by_team("y_d").await.unwrap();
    assert_eq!(found.len(), 1, "an underscore matches any one character");

    let found = store.search_users_by_team("example.com").await.unwrap();
    assert!(found.is_empty(), "email addresses are not searched");

    let found = store.search_users_by_team("trooper.11").await.unwrap();
    let expected = match common::db_type() {
        DbType::Sqlite => 1,
        DbType::Postgres => 0,
    };
    assert_eq!(found.len(), expected, "case sensitivity is the database's");

    // A query is data, whatever it contains.
    let found = store
        .search_users_by_team("' OR '1'='1")
        .await
        .expect("a quote in the query is not SQL");
    assert!(found.is_empty(), "got {found:?}");

    test.teardown().await;
}

/// Updates of users that do not exist fail with an error that is not a "not found".
#[tokio::test]
async fn updates_of_missing_users_fail() {
    let test = common::setup().await;
    let store = &test.store;

    let mut ghost = User {
        id: "ghost".to_owned(),
        ..User::default()
    };
    let errors = [
        store.update_user(&mut ghost).await.unwrap_err(),
        store
            .update_user_password("ghost", "hash")
            .await
            .unwrap_err(),
        store
            .update_user_password_by_id("ghost", "hash")
            .await
            .unwrap_err(),
    ];
    for error in errors {
        assert_eq!(error.to_string(), "user not found (ghost)");
        assert!(!error.is_not_found(), "reported as a server error");
    }

    test.teardown().await;
}

/// Preferences are kept per user under one category, and a patch returns the untouched
/// ones first, then the updated ones.
#[tokio::test]
async fn user_preferences_are_per_user_and_ordered() {
    let test = common::setup().await;
    let store = &test.store;

    let none = store.get_user_preferences("user-a").await.unwrap();
    assert!(none.is_empty(), "got {none:?}");

    let patch = |updated: &[(&str, &str)], deleted: &[&str]| UserPreferencesPatch {
        updated_fields: updated
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect(),
        deleted_fields: deleted.iter().map(|name| (*name).to_owned()).collect(),
    };

    store
        .patch_user_preferences("user-a", &patch(&[("b", "1"), ("a", "2"), ("c", "3")], &[]))
        .await
        .unwrap();
    store
        .patch_user_preferences("user-b", &patch(&[("a", "other")], &[]))
        .await
        .unwrap();

    let patched = store
        .patch_user_preferences(
            "user-a",
            &patch(&[("a", "4"), ("d", "5")], &["b", "missing"]),
        )
        .await
        .unwrap();
    let names: Vec<&str> = patched
        .iter()
        .map(|preference| preference.name.as_str())
        .collect();
    assert_eq!(names, ["c", "a", "d"], "untouched first, then updated");
    assert_eq!(value_of(&patched, "a"), Some("4"));
    assert!(
        patched
            .iter()
            .all(|preference| preference.user_id == "user-a" && preference.category == "fvoci"),
        "got {patched:?}"
    );

    let mut stored = store.get_user_preferences("user-a").await.unwrap();
    stored.sort_by(|left, right| left.name.cmp(&right.name));
    let mut returned = patched;
    returned.sort_by(|left, right| left.name.cmp(&right.name));
    assert_eq!(stored, returned, "the patch returns what is stored");

    let other = store.get_user_preferences("user-b").await.unwrap();
    assert_eq!(value_of(&other, "a"), Some("other"));
    assert_eq!(other.len(), 1);

    // A name that is both updated and deleted ends up deleted.
    let both = store
        .patch_user_preferences("user-b", &patch(&[("a", "again")], &["a"]))
        .await
        .unwrap();
    assert!(both.is_empty(), "got {both:?}");
    let other = store.get_user_preferences("user-b").await.unwrap();
    assert!(other.is_empty(), "got {other:?}");

    test.teardown().await;
}
