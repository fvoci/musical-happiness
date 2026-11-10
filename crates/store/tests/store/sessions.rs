//! Port of `server/services/store/storetests/session.go`, followed by tests of the session
//! operations that the original suite does not cover.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use fvoci_model::Session;
use fvoci_store::DbType;
use serde_json::json;
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::{ConnectOptions as _, Connection as _};

use crate::common;
use crate::users::clock_count;

/// `testCreateAndGetAndDeleteSession`.
#[tokio::test]
async fn create_and_get_and_delete_session() {
    let test = common::setup().await;
    let store = &test.store;

    let session = Session {
        id: "session-id".to_owned(),
        token: "token".to_owned(),
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

/// `testGetActiveUserCount`.
#[tokio::test]
async fn get_active_user_count() {
    let test = common::setup().await;
    let store = &test.store;

    // "no active user"
    let count = store.get_active_user_count(60).await.unwrap();
    assert_eq!(count, 0);

    // "active user"
    // gen random count active user session
    let count = clock_count();
    for index in 0..count {
        let session = Session {
            id: format!("id-{index}"),
            user_id: format!("user-id-{index}"),
            token: format!("token-{index}"),
            ..Session::default()
        };
        store.create_session(&session).await.unwrap();
    }

    let got = store.get_active_user_count(60).await.unwrap();
    assert_eq!(usize::try_from(got).ok(), Some(count));

    test.teardown().await;
}

/// `testUpdateSession`.
#[tokio::test]
async fn update_session() {
    let test = common::setup().await;
    let store = &test.store;

    let mut session = Session {
        id: "session-id".to_owned(),
        token: "token".to_owned(),
        props: serde_json::from_value(json!({ "field1": "A" })).unwrap(),
        ..Session::default()
    };

    store.create_session(&session).await.unwrap();

    // update session
    session.props.insert("field1".to_owned(), json!("B"));
    store.update_session(&session).await.unwrap();

    let got = store.get_session(&session.token, 60).await.unwrap();
    assert_eq!(got, session);

    test.teardown().await;
}

// The tests below are not in the original suite. They pin behavior of the original's
// standalone store that its suite leaves untested.

/// A session is read without its times, and one that is exactly at the expiry limit is
/// already expired.
#[tokio::test]
async fn get_session_leaves_the_times_unset_and_honors_the_expiry() {
    let test = common::setup().await;
    let store = &test.store;

    let session = Session {
        id: "session-id".to_owned(),
        token: "token".to_owned(),
        user_id: "user-id".to_owned(),
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

    // Deleting a session that does not exist, and refreshing or updating one whose token
    // is unknown, succeed without doing anything.
    store.delete_session("no-such-session").await.unwrap();
    let stranger = Session {
        token: "no-such-token".to_owned(),
        ..Session::default()
    };
    store.refresh_session(&stranger).await.unwrap();
    store.update_session(&stranger).await.unwrap();

    test.teardown().await;
}

/// Expiry, the active-user count and the cleanup all go by the time of the last refresh,
/// with limits given in seconds.
#[tokio::test]
async fn refresh_and_clean_up_sessions_go_by_the_update_time() {
    let test = common::setup().await;
    let store = &test.store;

    let session = |name: &str| Session {
        id: format!("id-{name}"),
        token: format!("token-{name}"),
        user_id: format!("user-{name}"),
        ..Session::default()
    };
    let stale = session("stale");
    let fresh = session("fresh");
    store.create_session(&stale).await.unwrap();
    store.create_session(&fresh).await.unwrap();
    assert_eq!(store.get_active_user_count(1).await.unwrap(), 2);

    tokio::time::sleep(Duration::from_millis(1100)).await;
    assert_eq!(store.get_active_user_count(1).await.unwrap(), 0);
    assert!(store.get_session(&fresh.token, 1).await.is_err());

    store.refresh_session(&fresh).await.unwrap();
    assert!(store.get_session(&fresh.token, 1).await.is_ok());
    assert!(store.get_session(&stale.token, 1).await.is_err());
    assert_eq!(store.get_active_user_count(1).await.unwrap(), 1);

    // Two sessions of one user count once.
    let second = Session {
        id: "id-second".to_owned(),
        token: "token-second".to_owned(),
        ..fresh.clone()
    };
    store.create_session(&second).await.unwrap();
    assert_eq!(store.get_active_user_count(1).await.unwrap(), 1);

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

/// On SQLite the original binds JSON as bytes, which the database keeps as a BLOB even in
/// a text column. A database written by the original must still read back.
#[tokio::test]
async fn json_stored_as_a_blob_by_the_original_on_sqlite_reads_back() {
    if common::db_type() != DbType::Sqlite {
        return;
    }
    let test = common::setup().await;
    let store = &test.store;

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
    let prefix = common::TABLE_PREFIX;
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "INSERT INTO {prefix}sessions \
         (id, token, user_id, auth_service, props, create_at, update_at) \
         VALUES ('s', 'blob-token', 'u', 'native', CAST('{{\"k\":\"v\"}}' AS BLOB), $1, $1)"
    )))
    .bind(now)
    .execute(&mut conn)
    .await
    .unwrap();
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "INSERT INTO {prefix}teams (id, signup_token, settings, modified_by, update_at) \
         VALUES ('0', 'token', CAST('{{\"on\":true}}' AS BLOB), 'u', $1)"
    )))
    .bind(now)
    .execute(&mut conn)
    .await
    .unwrap();
    conn.close().await.unwrap();

    let session = store.get_session("blob-token", 60).await.unwrap();
    assert_eq!(session.props.get("k"), Some(&json!("v")));
    let team = store.get_team("0").await.unwrap();
    assert_eq!(team.settings.get("on"), Some(&json!(true)));

    test.teardown().await;
}
