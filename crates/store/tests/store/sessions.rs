//! Port of `server/services/store/storetests/session.go`, followed by tests of the session
//! operations that the original suite does not cover.
//!
//! The original's `testGetActiveUserCount` and `testUpdateSession` have no port: this
//! store counts no active users and never rewrites a session.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use fvoci_model::{Session, User, new_id};
use fvoci_store::{DbType, StoreError};
use serde_json::json;
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::{ConnectOptions as _, Connection as _};

use crate::common;

/// `testCreateAndGetAndDeleteSession`.
#[tokio::test]
async fn create_and_get_and_delete_session() {
    let test = common::setup().await;
    let store = &test.store;
    let user_id = common::user(store, "session.owner").await;

    let session = Session {
        id: new_id(),
        token: "token".to_owned(),
        user_id,
        ..Session::default()
    };

    // "CreateAndGetSession"
    store.create_session(&session).await.unwrap();

    let got = store.get_session(&session.token, 60 * 60).await.unwrap();
    assert_eq!(got, session);

    // "Get nonexistent session"
    let error = store
        .get_session("nonexistent-token", 60 * 60)
        .await
        .unwrap_err();
    assert!(error.is_not_found(), "got {error:?}");

    // "DeleteAndGetSession"
    store.delete_session(&session.id).await.unwrap();

    let result = store.get_session(&session.token, 60 * 60).await;
    assert!(result.is_err(), "got {result:?}");

    test.teardown().await;
}

// The tests below are not in the original suite. They pin behavior of the original's
// standalone store that its suite leaves untested, and what this store does differently.

/// A session is read without its times, and one that is exactly at the expiry limit is
/// already expired.
#[tokio::test]
async fn get_session_leaves_the_times_unset_and_honors_the_expiry() {
    let test = common::setup().await;
    let store = &test.store;
    let user_id = common::user(store, "session.owner").await;

    let session = Session {
        id: new_id(),
        token: "token".to_owned(),
        user_id,
        auth_service: "native".to_owned(),
        props: serde_json::from_value(json!({ "nested": { "n": 1 } })).unwrap(),
        create_at: 11,
        update_at: 22,
    };
    store.create_session(&session).await.unwrap();

    let got = store.get_session("token", 60).await.unwrap();
    assert_eq!((got.create_at, got.update_at), (0, 0), "times are not read");
    assert_eq!(
        got,
        Session {
            create_at: 0,
            update_at: 0,
            ..session.clone()
        }
    );

    let expired = store.get_session("token", 0).await.unwrap_err();
    assert!(expired.is_not_found(), "got {expired:?}");
    assert_eq!(expired.to_string(), "sql: no rows in result set");

    // Deleting a session that does not exist, and refreshing one whose token is unknown,
    // succeed without doing anything.
    store.delete_session(&new_id()).await.unwrap();
    let stranger = Session {
        token: "no-such-token".to_owned(),
        ..Session::default()
    };
    store.refresh_session(&stranger).await.unwrap();
    let got = store.get_session("token", 60).await.unwrap();
    assert_eq!(got.id, session.id, "the stored session is still there");

    // Text that is not an identifier names no session, and a statement that writes
    // says so instead of doing nothing.
    let error = store.delete_session("no-such-session").await.unwrap_err();
    assert!(
        matches!(&error, StoreError::InvalidId(refused) if refused == "no-such-session"),
        "got {error:?}"
    );
    // A token is one session's, and so is an identifier.
    let same_token = Session {
        id: new_id(),
        ..session.clone()
    };
    let error = store.create_session(&same_token).await.unwrap_err();
    assert!(matches!(error, StoreError::Database(_)), "got {error:?}");
    let same_id = Session {
        token: "another-token".to_owned(),
        ..session.clone()
    };
    let error = store.create_session(&same_id).await.unwrap_err();
    assert!(matches!(error, StoreError::Database(_)), "got {error:?}");
    let error = store.get_session("another-token", 60).await.unwrap_err();
    assert!(error.is_not_found(), "got {error:?}");
    // A session is some user's: one without a user is not stored.
    let nobodys = Session {
        id: new_id(),
        token: "nobodys-token".to_owned(),
        ..Session::default()
    };
    let error = store.create_session(&nobodys).await.unwrap_err();
    assert!(
        matches!(&error, StoreError::InvalidId(refused) if refused.is_empty()),
        "got {error:?}"
    );

    test.teardown().await;
}

/// Expiry and the cleanup go by the time of the last refresh, with limits given in
/// seconds.
#[tokio::test]
async fn refresh_and_clean_up_sessions_go_by_the_update_time() {
    let test = common::setup().await;
    let store = &test.store;

    let mut sessions = Vec::new();
    for name in ["stale", "fresh"] {
        let session = Session {
            id: new_id(),
            token: format!("token-{name}"),
            user_id: common::user(store, name).await,
            ..Session::default()
        };
        store.create_session(&session).await.unwrap();
        sessions.push(session);
    }
    let [stale, fresh] = sessions
        .try_into()
        .expect("two sessions were created, a stale and a fresh one");
    assert!(store.get_session(&stale.token, 1).await.is_ok());
    assert!(store.get_session(&fresh.token, 1).await.is_ok());

    tokio::time::sleep(Duration::from_millis(1100)).await;
    assert!(store.get_session(&stale.token, 1).await.is_err());
    assert!(store.get_session(&fresh.token, 1).await.is_err());

    store.refresh_session(&fresh).await.unwrap();
    assert!(store.get_session(&fresh.token, 1).await.is_ok());
    assert!(store.get_session(&stale.token, 1).await.is_err());

    // A second session of the user of the fresh one.
    let second = Session {
        id: new_id(),
        token: "token-second".to_owned(),
        ..fresh.clone()
    };
    store.create_session(&second).await.unwrap();

    assert!(store.get_session(&stale.token, 60).await.is_ok());
    store.clean_up_sessions(1).await.unwrap();
    assert!(
        store.get_session(&stale.token, 60).await.is_err(),
        "the session last refreshed more than a second ago is gone"
    );
    assert!(store.get_session(&fresh.token, 60).await.is_ok());
    assert!(store.get_session(&second.token, 60).await.is_ok());

    test.teardown().await;
}

/// The original binds JSON as bytes on SQLite, which the database keeps as a BLOB even
/// in a text column. This server writes JSON as text and refuses a database of the
/// original, yet its reader still accepts either storage class; this pins that.
#[tokio::test]
async fn json_stored_as_a_blob_by_the_original_on_sqlite_reads_back() {
    if common::db_type() != DbType::Sqlite {
        return;
    }
    let test = common::setup().await;
    let store = &test.store;
    let user_id = common::user(store, "blob.owner").await;
    let session_id = new_id();
    let team_id = new_id();

    let mut conn = SqliteConnectOptions::new()
        .filename(test.dbconfig())
        .connect()
        .await
        .unwrap();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis();
    let now = i64::try_from(now).unwrap();
    sqlx::query(
        "INSERT INTO sessions \
         (id, token, user_id, auth_service, props, create_at, update_at) \
         VALUES ($1, 'blob-token', $2, 'native', CAST('{\"k\":\"v\"}' AS BLOB), $3, $3)",
    )
    .bind(&session_id)
    .bind(&user_id)
    .bind(now)
    .execute(&mut conn)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO teams (id, signup_token, settings, modified_by, create_at, update_at) \
         VALUES ($1, 'token', CAST('{\"on\":true}' AS BLOB), $2, $3, $3)",
    )
    .bind(&team_id)
    .bind(&user_id)
    .bind(now)
    .execute(&mut conn)
    .await
    .unwrap();
    conn.close().await.unwrap();

    let session = store.get_session("blob-token", 60).await.unwrap();
    assert_eq!(session.id, session_id);
    assert_eq!(session.props.get("k"), Some(&json!("v")));
    let team = store.get_team(&team_id).await.unwrap();
    assert_eq!(team.settings.get("on"), Some(&json!(true)));
    assert_eq!(team.modified_by, user_id);

    test.teardown().await;
}

/// Not in the original: a password set by name ends the sessions of the user with that
/// name, and only theirs, together with the password change.
#[tokio::test]
async fn reset_user_password_deletes_the_sessions_of_those_users_only() {
    let test = common::setup().await;
    let store = &test.store;
    let hour = 60 * 60;
    let mut user_ids = Vec::new();
    for username in ["anna", "bert"] {
        let mut user = User {
            id: new_id(),
            username: username.to_owned(),
            email: format!("{username}@example.com"),
            password: "old-hash".to_owned(),
            ..User::default()
        };
        store.create_user(&mut user).await.unwrap();
        user_ids.push(user.id);
    }
    let (anna, bert) = (&user_ids[0], &user_ids[1]);
    for (token, user_id) in [
        ("anna-phone", anna),
        ("anna-laptop", anna),
        ("bert-phone", bert),
    ] {
        let session = Session {
            id: new_id(),
            token: token.to_owned(),
            user_id: user_id.clone(),
            ..Session::default()
        };
        store.create_session(&session).await.unwrap();
    }

    let reset = store.reset_user_password("anna", "new-hash").await.unwrap();

    assert_eq!(
        reset,
        std::slice::from_ref(anna),
        "the users whose password was set"
    );
    assert_eq!(
        store.get_user_by_id(anna).await.unwrap().password,
        "new-hash"
    );
    for token in ["anna-phone", "anna-laptop"] {
        let error = store.get_session(token, hour).await.unwrap_err();
        assert!(error.is_not_found(), "{token}: got {error:?}");
    }
    assert_eq!(
        store.get_session("bert-phone", hour).await.unwrap().user_id,
        *bert,
        "the session of a user with another name"
    );
    assert_eq!(
        store.get_user_by_id(bert).await.unwrap().password,
        "old-hash",
        "the password of a user with another name"
    );

    // An unknown name changes nothing.
    let error = store.reset_user_password("nobody", "x").await.unwrap_err();
    assert_eq!(error.to_string(), "user not found (nobody)");
    assert_eq!(
        store.get_session("bert-phone", hour).await.unwrap().user_id,
        *bert
    );

    test.teardown().await;
}
