//! Whether the server is alive (`hello`) and ready (`ready`): the database answers and
//! the file storage can be used. A failing check says which, and never why.

use reqwest::StatusCode;
use serde_json::json;

use crate::common::{Setup, TestServer};
use crate::metrics::{sample, scrape};

/// What `ready` answers to someone without a session.
async fn ready(server: &TestServer) -> crate::common::ApiResponse {
    server.anonymous().get("/ready").await
}

#[tokio::test]
async fn a_server_that_can_serve_is_ready_and_says_so_without_a_session() {
    let server = Setup::native().metrics_at("127.0.0.1:0").start().await;
    let response = ready(&server).await;
    response.check_ok();
    assert_eq!(response.value(), json!({"database": "ok", "files": "ok"}));
    let body = scrape(&server).await;
    assert_eq!(sample(&body, "fvoci_ready", &[]), Some(1.0), "{body}");
    for check in ["database", "files"] {
        assert_eq!(
            sample(&body, "fvoci_ready_check", &[("check", check)]),
            Some(1.0),
            "{check}: {body}"
        );
    }
    assert!(
        !body.contains("fvoci_files_operation_duration_seconds_count"),
        "the check of the files is no operation on them: {body}"
    );
    server.teardown().await;
}

#[tokio::test]
async fn without_the_database_the_server_is_alive_and_not_ready_and_says_no_more() {
    let server = Setup::native().metrics_at("127.0.0.1:0").start().await;
    server.app.store().close().await;
    let alive = server.anonymous().get("/hello").await;
    alive.check_ok();
    assert_eq!(alive.text(), "Hello", "alive asks nothing of the database");

    let response = ready(&server).await;
    response.check_error(StatusCode::SERVICE_UNAVAILABLE, "not ready: database");
    let text = response.text().to_lowercase();
    for cause in [
        "pool",
        "closed",
        "sqlx",
        "sqlite",
        "postgres",
        "error",
        "127.0.0.1",
    ] {
        assert!(!text.contains(cause), "no cause in the answer: {text}");
    }
    let body = scrape(&server).await;
    assert_eq!(sample(&body, "fvoci_ready", &[]), Some(0.0), "{body}");
    assert_eq!(
        sample(&body, "fvoci_ready_check", &[("check", "database")]),
        Some(0.0),
        "{body}"
    );
    assert_eq!(
        sample(&body, "fvoci_ready_check", &[("check", "files")]),
        Some(1.0),
        "{body}"
    );
    server.teardown().await;
}

#[tokio::test]
async fn a_files_directory_that_is_no_directory_is_not_ready() {
    let server = Setup::native()
        .configure(|config| {
            // A file, which the server only reads here.
            concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml").clone_into(&mut config.files_path);
        })
        .start()
        .await;
    let response = ready(&server).await;
    response.check_error(StatusCode::SERVICE_UNAVAILABLE, "not ready: files");
    assert!(
        !response.text().contains("Cargo.toml"),
        "no path in the answer: {}",
        response.text()
    );
    server.teardown().await;
}

#[tokio::test]
async fn the_checks_are_kept_for_a_few_seconds() {
    let server = Setup::native().start().await;
    ready(&server).await.check_ok();
    server.app.store().close().await;
    ready(&server).await.check_ok();
    server.teardown().await;
}

/// With S3, a bucket that answers is ready, and one that is not there is not: a look at
/// a key would find both alike.
#[tokio::test]
#[ignore = "needs an S3-compatible server; see tests/storage.rs"]
async fn with_s3_a_bucket_that_is_not_there_is_not_ready() {
    assert!(
        std::env::var("FVOCI_S3_TEST_ENDPOINT").is_ok()
            && std::env::var("FVOCI_S3_TEST_BUCKET").is_ok(),
        "set FVOCI_S3_TEST_ENDPOINT and FVOCI_S3_TEST_BUCKET to run this test"
    );
    let server = Setup::native()
        .configure(|config| {
            let bucket = std::env::var("FVOCI_S3_TEST_BUCKET").unwrap_or_default();
            crate::uploads::configure_s3(config, &bucket);
        })
        .start()
        .await;
    ready(&server).await.check_ok();
    server.teardown().await;

    let server = Setup::native()
        .configure(|config| crate::uploads::configure_s3(config, "fvoci-no-such-bucket"))
        .start()
        .await;
    let response = ready(&server).await;
    response.check_error(StatusCode::SERVICE_UNAVAILABLE, "not ready: files");
    assert!(
        !response.text().contains("no-such-bucket"),
        "no bucket in the answer: {}",
        response.text()
    );
    server.teardown().await;
}
