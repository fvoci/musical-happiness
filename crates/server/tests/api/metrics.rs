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

/// The value of the sample of `name` whose labels are exactly `labels`, given in the
/// order of their names, as the text format writes them.
pub(crate) fn sample(body: &str, name: &str, labels: &[(&str, &str)]) -> Option<f64> {
    let labels: Vec<String> = labels
        .iter()
        .map(|(label, value)| format!("{label}=\"{value}\""))
        .collect();
    let series = if labels.is_empty() {
        name.to_owned()
    } else {
        format!("{name}{{{}}}", labels.join(","))
    };
    body.lines().find_map(|line| {
        let value = line.strip_prefix(&series)?.strip_prefix(' ')?;
        value.parse().ok()
    })
}

/// The lines of the samples of `name`.
fn series<'a>(body: &'a str, name: &str) -> Vec<&'a str> {
    body.lines()
        .filter(|line| line.starts_with(name) && line[name.len()..].starts_with(['{', ' ']))
        .collect()
}

/// How many requests the histogram counted with these labels.
fn requests(body: &str, method: &str, route: &str, status: &str) -> Option<f64> {
    sample(
        body,
        "fvoci_http_request_duration_seconds_count",
        &[("method", method), ("route", route), ("status", status)],
    )
}

#[tokio::test]
async fn requests_are_counted_by_method_route_template_and_status() {
    let server = server_with_metrics().await;
    for _ in 0..3 {
        let response = server.client.get("/hello").await;
        assert_eq!(response.status, StatusCode::OK);
    }
    let response = Client::send(server.client.raw_request(Method::GET, "/static/main.js")).await;
    assert_eq!(response.status, StatusCode::OK);
    let body = scrape(&server).await;
    assert_eq!(
        requests(&body, "GET", "/api/v1/hello", "200"),
        Some(3.0),
        "{body}"
    );
    assert_eq!(
        requests(&body, "GET", "/static/{*path}", "200"),
        Some(1.0),
        "{body}"
    );
    assert_eq!(
        sample(&body, "fvoci_http_requests_in_flight", &[]),
        Some(0.0),
        "nothing is being answered: {body}"
    );
    server.teardown().await;
}

#[tokio::test]
async fn paths_with_identifiers_are_counted_under_their_template() {
    let server = server_with_metrics().await;
    let ids: Vec<String> = (0..3).map(|_| fvoci_model::new_id()).collect();
    for id in &ids {
        let response = server.client.get(&format!("/boards/{id}")).await;
        assert_eq!(response.status, StatusCode::UNAUTHORIZED, "no session");
    }
    let body = scrape(&server).await;
    assert_eq!(
        requests(&body, "GET", "/api/v1/boards/{boardId}", "401"),
        Some(3.0),
        "one series for the three boards: {body}"
    );
    for id in &ids {
        assert!(
            !body.contains(id.as_str()),
            "no identifier in a label: {body}"
        );
    }
    server.teardown().await;
}

#[tokio::test]
async fn paths_and_methods_that_are_no_route_do_not_make_labels() {
    let server = server_with_metrics().await;
    let ask_nonsense = async |count: usize| {
        let mut asked = Vec::new();
        for n in 0..count {
            let word = fvoci_model::new_id();
            let path = match n % 3 {
                0 => format!("/api/v1/{word}"),
                1 => format!("/{word}/{n}"),
                _ => format!("/api/v2/boards/{word}"),
            };
            let method = if n % 2 == 0 {
                Method::GET
            } else {
                Method::from_bytes(format!("X{n}").as_bytes()).expect("a method")
            };
            Client::send(server.client.raw_request(method, &path)).await;
            asked.push(word);
        }
        asked
    };
    let name = "fvoci_http_request_duration_seconds_count";
    let mut asked = ask_nonsense(6).await;
    let before = scrape(&server).await;
    asked.extend(ask_nonsense(60).await);
    let after = scrape(&server).await;
    assert_eq!(
        series(&before, name).len(),
        series(&after, name).len(),
        "ten times the nonsense makes no new series: {after}"
    );
    for word in &asked {
        assert!(
            !after.contains(word.as_str()),
            "nothing asked is a label: {after}"
        );
    }
    assert!(
        series(&after, name)
            .iter()
            .any(|line| line.contains("method=\"other\"") && line.contains("route=\"unmatched\"")),
        "an unknown method on an unknown path: {after}"
    );
    server.teardown().await;
}

#[tokio::test]
async fn the_base_path_is_not_part_of_the_route() {
    let server = Setup::native()
        .base_path("/boards")
        .metrics_at("127.0.0.1:0")
        .start()
        .await;
    let response = server.client.get("/hello").await;
    assert_eq!(response.status, StatusCode::OK);
    let body = scrape(&server).await;
    assert_eq!(
        requests(&body, "GET", "/api/v1/hello", "200"),
        Some(1.0),
        "{body}"
    );
    server.teardown().await;
}
