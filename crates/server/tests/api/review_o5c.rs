//! Found in the independent review of `org-o5c`: what a pending write of the
//! system's reach does to the requests of an organization.

use std::time::{Duration, Instant};

use futures_util::{SinkExt as _, StreamExt as _};
use fvoci_store::DbType;
use fvoci_store::testing::db_type_of_the_tests;
use serde_json::json;
use tokio_tungstenite::tungstenite::Message;

use crate::common::{ROOT_WORKSPACE_ID, Setup, TestServer};

/// How long the system's write below is held open.
const HELD: Duration = Duration::from_secs(4);

/// How much slower than an unhindered one a category may be made.
const SLACK: Duration = Duration::from_secs(1);

/// How long making a category takes while a slow statement of the system's reach runs
/// on a table of the trash, as a purge of a large trash does: it touches no row.
async fn category_during_a_system_write(ts: &TestServer, user: &str, name: &str) -> Duration {
    let system = ts.app.store().system();
    let slow = format!(
        "DELETE FROM deleted_boards WHERE CAST((SELECT pg_sleep({})) AS TEXT) = 'never'",
        HELD.as_secs()
    );
    let pending = tokio::spawn(async move { system.execute_unchecked(&slow).await });
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(!pending.is_finished(), "the system's write is under way");
    let started = Instant::now();
    ts.client
        .post(
            &format!("/w/{ROOT_WORKSPACE_ID}/categories"),
            &json!({"name": name, "userId": user, "workspaceId": ROOT_WORKSPACE_ID}),
        )
        .await
        .check_created();
    let took = started.elapsed();
    pending
        .await
        .expect("the system's write ran")
        .expect("the system's write succeeded");
    took
}

/// The housekeeping purges the trash of every organization through the system's
/// reach (`App::housekeeping`), whose writes move the revision every organization's
/// dispatch waits for. A category change is now announced inline in its request
/// (`broadcast_category_change(..).await`), and `Hub::send` waits for every writer of
/// that revision first: so while such a write is under way, an organization's user who
/// has the app open waits for it to make a category. Without an open connection the
/// same request is not held, which shows the database is not what holds it.
#[tokio::test]
async fn a_pending_system_write_does_not_hold_an_organizations_request() {
    if db_type_of_the_tests() != DbType::Postgres {
        return;
    }
    let ts = Setup::native().start().await.init_basic().await;
    let user = ts.client.me().await.id;

    let without_listener = category_during_a_system_write(&ts, &user, "closed app").await;
    assert!(
        without_listener < SLACK,
        "with nobody listening the system's write holds nothing: {without_listener:?}"
    );

    let mut socket = ts.client.connect_websocket().await;
    socket
        .send(Message::Text(
            json!({"action": "SUBSCRIBE_WORKSPACE", "workspaceId": ROOT_WORKSPACE_ID})
                .to_string()
                .into(),
        ))
        .await
        .expect("subscription sent");
    socket
        .send(Message::Ping(vec![7].into()))
        .await
        .expect("barrier sent");
    tokio::time::timeout(Duration::from_secs(5), async {
        while let Some(frame) = socket.next().await {
            if matches!(frame.expect("a frame"), Message::Pong(_)) {
                return;
            }
        }
    })
    .await
    .expect("the subscription is registered");

    let with_listener = category_during_a_system_write(&ts, &user, "open app").await;
    assert!(
        with_listener < SLACK,
        "a category took {with_listener:?} while the system wrote and its user had the app \
         open, {without_listener:?} with the app closed: the request waited for a write \
         of no organization of its own"
    );
    drop(socket);
    ts.teardown().await;
}
