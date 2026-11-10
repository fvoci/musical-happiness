//! Port of `server/services/webhook/webhook_test.go`.

use std::sync::{Arc, Mutex};

use axum::Router;
use axum::routing::post;
use fvoci_model::{Block, BlockType};
use fvoci_server::webhook::Webhook;

/// `TestClientUpdateNotify`
#[tokio::test]
async fn client_update_notify() {
    let received: Arc<Mutex<Vec<serde_json::Value>>> = Arc::default();
    let sink = Arc::clone(&received);
    let hook = Router::new().route(
        "/hook",
        post(
            move |axum::Json(body): axum::Json<serde_json::Value>| async move {
                sink.lock()
                    .expect("the test holds no lock across a panic")
                    .push(body);
            },
        ),
    );
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("a local port is free");
    let address = listener.local_addr().expect("the listener has an address");
    let serving = tokio::spawn(async move { axum::serve(listener, hook).await });

    // The first address refuses connections (its listener is gone); the second must be
    // notified all the same.
    let closed = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("a local port is free");
    let closed_address = closed.local_addr().expect("the listener has an address");
    drop(closed);
    let client = Webhook::new(vec![
        format!("http://{closed_address}/unreachable"),
        format!("http://{address}/hook"),
    ])
    .expect("the webhook client is set up");
    let block = Block {
        id: "block-id".to_owned(),
        block_type: BlockType::Card,
        title: "A card".to_owned(),
        ..Block::default()
    };
    client.notify_update(&block).await;

    let received = received
        .lock()
        .expect("the test holds no lock across a panic");
    assert_eq!(received.len(), 1, "webhook url not be notified");
    assert_eq!(received[0]["id"], "block-id", "the block is the body");
    assert_eq!(received[0]["title"], "A card", "with its members");
    serving.abort();
}

/// Without addresses nothing is sent.
#[tokio::test]
async fn no_addresses_no_requests() {
    Webhook::new(Vec::new())
        .expect("no addresses need no client")
        .notify_update(&Block::default())
        .await;
}
