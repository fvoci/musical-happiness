//! Port of `server/services/store/storetests/users.go`, followed by tests of the user
//! operations that the original suite does not cover.

use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

use fvoci_model::{User, UserPreferencesPatch, new_id};
use fvoci_store::{DbType, Preference, Store, StoreError};

use crate::common;

/// The count the original derives from the clock: `time.Now().Unix() % 10`.
fn clock_count() -> usize {
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

    let user_id = new_id();

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

    // The original ends with a deferred update of the user, whose result it ignores.
    // This store has no such update.

    test.teardown().await;
}

/// `testCreateAndGetUser`.
#[tokio::test]
async fn create_and_get_user() {
    let test = common::setup().await;
    let store = &test.store;

    let mut user = User {
        id: new_id(),
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
    let error = store.get_user_by_id(&new_id()).await.unwrap_err();
    assert!(matches!(error, StoreError::NotFound(_)), "got {error:?}");

    // "GetUserByUsername"
    let got = store.get_user_by_username(&user.username).await.unwrap();
    assert_eq!(got.id, user.id);
    assert_eq!(got.username, user.username);
    assert_eq!(got.email, user.email);

    // "GetUserByUsername nonexistent"
    // The original looks the name up as an identifier here; kept as written. It is text
    // that is not an identifier, under which nothing is found. The lookup by name, which
    // the original meant, follows.
    let error = store
        .get_user_by_id("nonexistent-username")
        .await
        .unwrap_err();
    assert!(matches!(error, StoreError::NotFound(_)), "got {error:?}");
    let error = store
        .get_user_by_username("nonexistent-username")
        .await
        .unwrap_err();
    assert!(matches!(error, StoreError::NotFound(_)), "got {error:?}");

    // "GetUserByEmail"
    let got = store.get_user_by_email(&user.email).await.unwrap();
    assert_eq!(got.id, user.id);
    assert_eq!(got.username, user.username);
    assert_eq!(got.email, user.email);

    // "GetUserByEmail nonexistent"
    // The original looks the address up as an identifier here; kept as written, and
    // followed by the lookup by address.
    let error = store.get_user_by_id("nonexistent-email").await.unwrap_err();
    assert!(matches!(error, StoreError::NotFound(_)), "got {error:?}");
    let error = store
        .get_user_by_email("nonexistent-email")
        .await
        .unwrap_err();
    assert!(matches!(error, StoreError::NotFound(_)), "got {error:?}");

    test.teardown().await;
}

/// One case of `testGetUsersList`.
struct UsersListCase<'a> {
    name: &'static str,
    user_ids: [&'a str; 2],
    expected_error: bool,
    expected_ids: &'a [&'a str],
}

/// `testGetUsersList`.
#[tokio::test]
async fn get_users_list() {
    let test = common::setup().await;
    let store = &test.store;

    // The original's "user1" and "user2", and its identifiers of nobody.
    let user1 = common::user(store, "user1-username").await;
    let user2 = common::user(store, "user2-username").await;
    let (nobody1, nobody2) = (new_id(), new_id());

    let test_cases = [
        UsersListCase {
            name: "all of the IDs are found",
            user_ids: [&user1, &user2],
            expected_error: false,
            expected_ids: &[&user1, &user2],
        },
        UsersListCase {
            name: "some of the IDs are found",
            user_ids: [&user2, &nobody1],
            expected_error: true,
            expected_ids: &[&user2],
        },
        UsersListCase {
            name: "none of the IDs are found",
            user_ids: [&nobody1, &nobody2],
            expected_error: true,
            expected_ids: &[],
        },
        // Not in the original, whose "non-existent" is this: text that is not an
        // identifier names nobody.
        UsersListCase {
            name: "text that is not an identifier is not found",
            user_ids: [&user1, "non-existent"],
            expected_error: true,
            expected_ids: &[&user1],
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
        let mut expected = tc.expected_ids.to_vec();
        expected.sort_unstable();
        assert_eq!(found, expected, "{name}");
    }

    test.teardown().await;
}

/// `testCreateAndUpdateUser`.
#[tokio::test]
async fn create_and_update_user() {
    let test = common::setup().await;
    let store = &test.store;

    // The original creates a user without a name and gives it one in an "UpdateUser"
    // step. This store never rewrites a name or an address, so the user is created with
    // both, and the password updates follow.
    let mut user = User {
        id: new_id(),
        username: "damao".to_owned(),
        email: "mock@email.com".to_owned(),
        ..User::default()
    };
    store.create_user(&mut user).await.unwrap();

    let got = store.get_user_by_id(&user.id).await.unwrap();
    assert_eq!(got.id, user.id);
    assert_eq!(got.username, user.username);
    assert_eq!(got.email, user.email);

    // "UpdateUserPassword"
    let new_password = new_id();
    store
        .reset_user_password(&user.username, &new_password)
        .await
        .unwrap();

    let got = store.get_user_by_username(&user.username).await.unwrap();
    assert_eq!(got.username, user.username);
    assert_eq!(got.password_hash, new_password);

    // "UpdateUserPasswordByID": the password a user changes themselves.
    let new_password = new_id();
    store
        .change_user_password(&user.id, &new_password, &new_id())
        .await
        .unwrap();

    let got = store.get_user_by_id(&user.id).await.unwrap();
    assert_eq!(got.id, user.id);
    assert_eq!(got.password_hash, new_password);

    test.teardown().await;
}

/// `testCreateAndGetRegisteredUserCount`.
#[tokio::test]
async fn create_and_get_registered_user_count() {
    let test = common::setup().await;
    let store = &test.store;

    // The original's users have no names; here a name and an address are one user's.
    let random_n = clock_count();
    for index in 0..random_n {
        let mut user = User {
            id: new_id(),
            username: format!("user-{index}"),
            email: format!("user-{index}@example.com"),
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
        id: new_id(),
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
// standalone store that its suite leaves untested, and what this store does differently.

/// What a created user gets stamped with, which members a read fills in, and what is
/// one user's alone.
#[tokio::test]
async fn create_user_stamps_times_and_reads_back_the_stored_members() {
    let test = common::setup().await;
    let store = &test.store;

    let before = now_millis();
    let mut user = User {
        id: new_id(),
        username: "leia".to_owned(),
        email: "leia@example.com".to_owned(),
        nickname: "not stored".to_owned(),
        password_hash: "hash".to_owned(),
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

    // The identifier, the name and the address are each one user's.
    let mut same_id = User {
        username: "luke".to_owned(),
        email: "luke@example.com".to_owned(),
        ..user.clone()
    };
    let duplicate = store.create_user(&mut same_id).await.unwrap_err();
    assert!(
        matches!(duplicate, StoreError::Database(_)),
        "got {duplicate:?}"
    );
    let mut namesake = User {
        id: new_id(),
        email: "namesake@example.com".to_owned(),
        ..user.clone()
    };
    let taken = store.create_user(&mut namesake).await.unwrap_err();
    assert!(matches!(taken, StoreError::Conflict(_)), "got {taken:?}");
    assert_eq!(taken.to_string(), "the user name is taken");
    let mut same_address = User {
        id: new_id(),
        username: "organa".to_owned(),
        ..user.clone()
    };
    let taken = store.create_user(&mut same_address).await.unwrap_err();
    assert!(matches!(taken, StoreError::Conflict(_)), "got {taken:?}");
    assert_eq!(taken.to_string(), "the email address is taken");
    assert_eq!(
        store.get_registered_user_count().await.unwrap(),
        1,
        "none of the refused users was stored"
    );
    let got = store.get_user_by_id(&user.id).await.unwrap();
    assert_eq!(got, stored, "the refused users changed nothing");

    // Text that is not an identifier is no user's identifier.
    let mut unnamed = User {
        id: "not-an-id".to_owned(),
        username: "han".to_owned(),
        email: "han@example.com".to_owned(),
        ..User::default()
    };
    let error = store.create_user(&mut unnamed).await.unwrap_err();
    assert!(
        matches!(&error, StoreError::InvalidId(refused) if refused == "not-an-id"),
        "got {error:?}"
    );

    test.teardown().await;
}

/// A name and an address are one user's whatever their letter case: a second user with
/// either, spelled in another case, is refused as a conflict that says which is taken.
#[tokio::test]
async fn a_name_or_an_address_in_another_letter_case_is_taken() {
    let test = common::setup().await;
    let store = &test.store;

    let han = create_user(
        store,
        User {
            id: new_id(),
            username: "Han.Solo".to_owned(),
            email: "Han@Example.com".to_owned(),
            ..User::default()
        },
    )
    .await;

    let cases = [
        ("han.solo", "smuggler@example.com", "the user name"),
        ("HAN.SOLO", "smuggler@example.com", "the user name"),
        ("hAN.sOLO", "smuggler@example.com", "the user name"),
        ("smuggler", "han@example.com", "the email address"),
        ("smuggler", "HAN@EXAMPLE.COM", "the email address"),
        ("smuggler", "hAN@eXAMPLE.COM", "the email address"),
    ];
    for (username, email, taken) in cases {
        let mut second = User {
            id: new_id(),
            username: username.to_owned(),
            email: email.to_owned(),
            ..User::default()
        };
        let error = store.create_user(&mut second).await.unwrap_err();
        assert!(
            matches!(error, StoreError::Conflict(_)),
            "{username} <{email}>: got {error:?}"
        );
        assert_eq!(
            error.to_string(),
            format!("{taken} is taken"),
            "{username} <{email}>"
        );
        assert!(
            !error.is_not_found(),
            "{username} <{email}>: a conflict is not a missing record"
        );
    }

    // Both taken at once: a conflict, of whichever the database looks at first.
    let mut both = User {
        id: new_id(),
        username: "han.solo".to_owned(),
        email: "han@example.com".to_owned(),
        ..User::default()
    };
    let error = store.create_user(&mut both).await.unwrap_err();
    assert!(matches!(error, StoreError::Conflict(_)), "got {error:?}");

    assert_eq!(
        store.get_registered_user_count().await.unwrap(),
        1,
        "none of the refused users was stored"
    );
    let stored = store.get_user_by_id(&han.id).await.unwrap();
    assert_eq!(
        (stored.username.as_str(), stored.email.as_str()),
        ("Han.Solo", "Han@Example.com"),
        "the first user keeps the name and the address as given"
    );

    // A name that only resembles the taken one is free: the comparison is of whole
    // names, and `_` is no wildcard.
    for (username, email) in [
        ("han_solo", "han_solo@example.com"),
        ("han.sol", "han@example.co"),
    ] {
        let mut other = User {
            id: new_id(),
            username: username.to_owned(),
            email: email.to_owned(),
            ..User::default()
        };
        store
            .create_user(&mut other)
            .await
            .unwrap_or_else(|error| panic!("{username} <{email}> is free: {error:?}"));
    }
    assert_eq!(store.get_registered_user_count().await.unwrap(), 3);

    test.teardown().await;
}

/// Lookups by name and by email address ignore the case of ASCII letters on both
/// databases, and find the user as stored.
#[tokio::test]
#[expect(
    clippy::too_many_lines,
    reason = "one store, looked up by every name and address in every letter case"
)]
async fn user_lookups_ignore_ascii_case() {
    let test = common::setup().await;
    let store = &test.store;

    let han = create_user(
        store,
        User {
            id: new_id(),
            username: "Han.Solo".to_owned(),
            email: "Han@Example.com".to_owned(),
            ..User::default()
        },
    )
    .await;
    let leia = create_user(
        store,
        User {
            id: new_id(),
            username: "leia".to_owned(),
            email: "leia@example.com".to_owned(),
            password_hash: "leia-hash".to_owned(),
            ..User::default()
        },
    )
    .await;

    for username in ["Han.Solo", "han.solo", "HAN.SOLO", "hAN.sOLO"] {
        let found = store
            .get_user_by_username(username)
            .await
            .unwrap_or_else(|error| panic!("{username} names the user: {error:?}"));
        assert_eq!(found.id, han.id, "{username}");
        assert_eq!(found.username, "Han.Solo", "{username}: read as stored");
    }
    for email in [
        "Han@Example.com",
        "han@example.com",
        "HAN@EXAMPLE.COM",
        "hAN@eXAMPLE.COM",
    ] {
        let found = store
            .get_user_by_email(email)
            .await
            .unwrap_or_else(|error| panic!("{email} is the user's address: {error:?}"));
        assert_eq!(found.id, han.id, "{email}");
        assert_eq!(found.email, "Han@Example.com", "{email}: read as stored");
    }
    let found = store.get_user_by_username("LEIA").await.unwrap();
    assert_eq!(found.id, leia.id, "each name finds its own user");

    // Only the case is ignored: the whole name is compared, a name is not an address,
    // and `%` and `_` are not wildcards.
    for username in ["han", "han.solo ", "han_solo", "%", "Han@Example.com", ""] {
        let error = store.get_user_by_username(username).await.unwrap_err();
        assert!(error.is_not_found(), "{username:?}: got {error:?}");
        assert_eq!(error.to_string(), "{user} not found", "{username:?}");
    }
    for email in ["han@example", "han_example.com", "%", "Han.Solo", ""] {
        let error = store.get_user_by_email(email).await.unwrap_err();
        assert!(error.is_not_found(), "{email:?}: got {error:?}");
    }

    // A password is set by name in any letter case too, for that user alone.
    let reset = store
        .reset_user_password("hAN.sOLO", "new-hash")
        .await
        .unwrap();
    assert_eq!(reset, han.id, "the user whose password was set");
    let found = store.get_user_by_id(&han.id).await.unwrap();
    assert_eq!(found.password_hash, "new-hash");
    let found = store.get_user_by_id(&leia.id).await.unwrap();
    assert_eq!(
        found.password_hash, "leia-hash",
        "another user's password stays"
    );

    // SQLite folds the ASCII letters only, so there a name that differs in the case of
    // another letter is another name. What PostgreSQL makes of it follows the locale of
    // the database, and is not pinned here.
    if common::db_type() == DbType::Sqlite {
        let zoe = create_user(
            store,
            User {
                id: new_id(),
                username: "ZOË".to_owned(),
                email: "ZOË@example.com".to_owned(),
                ..User::default()
            },
        )
        .await;
        let found = store.get_user_by_username("zoË").await.unwrap();
        assert_eq!(found.id, zoe.id, "the ASCII letters are folded");
        let found = store.get_user_by_email("zoË@EXAMPLE.COM").await.unwrap();
        assert_eq!(found.id, zoe.id, "the ASCII letters are folded");
        let error = store.get_user_by_username("zoë").await.unwrap_err();
        assert!(error.is_not_found(), "Ë is not folded: got {error:?}");
        let error = store
            .get_user_by_email("zoë@example.com")
            .await
            .unwrap_err();
        assert!(error.is_not_found(), "Ë is not folded: got {error:?}");
        create_user(
            store,
            User {
                id: new_id(),
                username: "zoë".to_owned(),
                email: "zoë@example.com".to_owned(),
                ..User::default()
            },
        )
        .await;
    }

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
                id: new_id(),
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
            id: new_id(),
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

    let by_name = store
        .reset_user_password("ghost", "hash")
        .await
        .unwrap_err();
    assert_eq!(by_name.to_string(), "user not found (ghost)");
    assert!(!by_name.is_not_found(), "reported as a server error");

    let ghost = new_id();
    let by_id = store
        .change_user_password(&ghost, "hash", &new_id())
        .await
        .unwrap_err();
    assert_eq!(by_id.to_string(), format!("user not found ({ghost})"));
    assert!(!by_id.is_not_found(), "reported as a server error");

    // Text that is not an identifier is refused as such before any user is looked for.
    let no_id = store
        .change_user_password("ghost", "hash", &new_id())
        .await
        .unwrap_err();
    assert!(
        matches!(&no_id, StoreError::InvalidId(refused) if refused == "ghost"),
        "got {no_id:?}"
    );

    test.teardown().await;
}

/// Preferences are kept per user under one category, and a patch returns the untouched
/// ones first, then the updated ones.
#[tokio::test]
async fn user_preferences_are_per_user_and_ordered() {
    let test = common::setup().await;
    let store = &test.store;
    let user_a = common::user(store, "user-a").await;
    let user_b = common::user(store, "user-b").await;

    let none = store.get_user_preferences(&user_a).await.unwrap();
    assert!(none.is_empty(), "got {none:?}");

    let patch = |updated: &[(&str, &str)], deleted: &[&str]| UserPreferencesPatch {
        updated_fields: updated
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect(),
        deleted_fields: deleted.iter().map(|name| (*name).to_owned()).collect(),
    };

    store
        .patch_user_preferences(&user_a, &patch(&[("b", "1"), ("a", "2"), ("c", "3")], &[]))
        .await
        .unwrap();
    store
        .patch_user_preferences(&user_b, &patch(&[("a", "other")], &[]))
        .await
        .unwrap();

    let patched = store
        .patch_user_preferences(
            &user_a,
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
            .all(|preference| preference.user_id == user_a && preference.category == "fvoci"),
        "got {patched:?}"
    );

    let mut stored = store.get_user_preferences(&user_a).await.unwrap();
    stored.sort_by(|left, right| left.name.cmp(&right.name));
    let mut returned = patched;
    returned.sort_by(|left, right| left.name.cmp(&right.name));
    assert_eq!(stored, returned, "the patch returns what is stored");

    let other = store.get_user_preferences(&user_b).await.unwrap();
    assert_eq!(value_of(&other, "a"), Some("other"));
    assert_eq!(other.len(), 1);

    // A name that is both updated and deleted ends up deleted.
    let both = store
        .patch_user_preferences(&user_b, &patch(&[("a", "again")], &["a"]))
        .await
        .unwrap();
    assert!(both.is_empty(), "got {both:?}");
    let other = store.get_user_preferences(&user_b).await.unwrap();
    assert!(other.is_empty(), "got {other:?}");

    // Text that is not an identifier names no user: it has no preferences, and none
    // are stored for it.
    let none = store.get_user_preferences("not-a-user").await.unwrap();
    assert!(none.is_empty(), "got {none:?}");
    let error = store
        .patch_user_preferences("not-a-user", &patch(&[("a", "1")], &[]))
        .await
        .unwrap_err();
    assert!(
        matches!(&error, StoreError::InvalidId(refused) if refused == "not-a-user"),
        "got {error:?}"
    );

    test.teardown().await;
}
