//! The Prometheus metrics: kept only if `prometheusaddress` is set, and served on a
//! listener of their own.

use std::time::Duration;

use reqwest::{Method, StatusCode};

use crate::common::{Client, Setup, TestServer};

/// A server that keeps metrics and serves them at a free local port.
async fn server_with_metrics() -> TestServer {
    Setup::native().metrics_at("127.0.0.1:0").start().await
}

/// What a `GET` of `path` on the metrics listener answers: the status, the
/// `Content-Type` and the body.
async fn scrape_path(server: &TestServer, path: &str) -> (StatusCode, String, String) {
    let address = server.metrics_address.expect("the server keeps metrics");
    let response = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .expect("an HTTP client")
        .get(format!("http://{address}{path}"))
        .send()
        .await
        .expect("the metrics listener answers");
    let status = response.status();
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    let body = response.text().await.expect("a text body");
    (status, content_type, body)
}

/// The metrics as the listener serves them.
pub(crate) async fn scrape(server: &TestServer) -> String {
    let (status, content_type, body) = scrape_path(server, "/metrics").await;
    assert_eq!(status, StatusCode::OK, "a scrape is answered: {body}");
    assert_eq!(
        content_type, "text/plain; version=0.0.4",
        "the Prometheus text format"
    );
    body
}

#[tokio::test]
async fn no_metrics_are_kept_and_nothing_listens_unless_an_address_is_set() {
    let server = Setup::native().start().await;
    assert_eq!(server.metrics_address, None, "no metrics listener");
    assert!(!server.app.metrics().is_kept(), "no metrics are kept");
    server.teardown().await;
}

#[tokio::test]
async fn the_metrics_are_served_on_a_listener_of_their_own() {
    let server = server_with_metrics().await;
    let body = scrape(&server).await;
    let version = env!("CARGO_PKG_VERSION");
    let commit = option_env!("FVOCI_BUILD_HASH").unwrap_or_default();
    assert!(
        body.contains(&format!(
            "fvoci_build_info{{commit=\"{commit}\",version=\"{version}\"}} 1"
        )),
        "the build information: {body}"
    );
    #[cfg(target_os = "linux")]
    for name in ["process_resident_memory_bytes", "process_open_fds"] {
        assert!(
            body.contains(&format!("\n{name} ")),
            "the process metric {name}: {body}"
        );
    }
    // Nothing but the metrics: no route of the API or the web app.
    for path in ["/", "/api/v1/hello", "/ws", "/static/main.js", "/metrics/"] {
        let (status, _, _) = scrape_path(&server, path).await;
        assert_eq!(
            status,
            StatusCode::NOT_FOUND,
            "{path} on the metrics listener"
        );
    }
    server.teardown().await;
}

#[tokio::test]
async fn the_api_listener_does_not_serve_the_metrics() {
    let server = server_with_metrics().await;
    // A path of the web app's: its page, not the metrics.
    let response = Client::send(server.client.raw_request(Method::GET, "/metrics")).await;
    let body = String::from_utf8_lossy(&response.body);
    assert!(!body.contains("fvoci_build_info"), "no metrics: {body}");
    assert!(body.contains("test web app"), "the web app's page: {body}");
    let response = server.client.get("/metrics").await;
    assert_eq!(
        response.status,
        StatusCode::NOT_FOUND,
        "no metrics under the API"
    );
    server.teardown().await;
}

#[tokio::test]
async fn a_metrics_address_that_is_taken_keeps_the_server_from_starting() {
    let taken = std::net::TcpListener::bind("127.0.0.1:0").expect("a free port");
    let address = taken.local_addr().expect("an address").to_string();
    let error = Setup::native()
        .metrics_at(&address)
        .try_start()
        .await
        .err()
        .expect("the server does not start");
    let error = format!("{error:#}");
    assert!(
        error.contains("prometheusaddress"),
        "the setting is named: {error}"
    );
    drop(taken);
}
