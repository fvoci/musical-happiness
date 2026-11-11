//! The metrics of what the server's work came to: the problems the API refused with, and
//! the calls of the database.

use reqwest::{Method, StatusCode};

use crate::common::{Client, Setup, TestServer};
use crate::metrics::{sample, scrape};

/// A server that keeps metrics and serves them at a free local port.
async fn server_with_metrics() -> TestServer {
    Setup::native().metrics_at("127.0.0.1:0").start().await
}

/// How many answers were the problem `name`.
fn problems(body: &str, name: &str) -> Option<f64> {
    sample(body, "fvoci_http_problems_total", &[("problem", name)])
}

/// The lines of the samples of `name`.
fn series<'a>(body: &'a str, name: &str) -> Vec<&'a str> {
    body.lines()
        .filter(|line| line.starts_with(name) && line[name.len()..].starts_with(['{', ' ']))
        .collect()
}

/// The values of the label `problem` of the samples of the problems.
fn problem_labels(body: &str) -> Vec<String> {
    series(body, "fvoci_http_problems_total")
        .iter()
        .filter_map(|line| {
            let rest = line.split_once("problem=\"")?.1;
            Some(rest.split_once('"')?.0.to_owned())
        })
        .collect()
}

#[tokio::test]
async fn refusals_are_counted_by_the_name_of_their_problem() {
    let server = Setup::native()
        .metrics_at("127.0.0.1:0")
        .start()
        .await
        .init_basic()
        .await;
    let before = scrape(&server).await;
    assert_eq!(
        problems(&before, "username-taken"),
        Some(0.0),
        "every problem is there from the start: {before}"
    );
    let stranger = server.anonymous();
    for _ in 0..2 {
        let response = stranger.get("/users/me").await;
        assert_eq!(response.status, StatusCode::UNAUTHORIZED);
    }
    let response = server.client.get("/no-such-route").await;
    assert_eq!(response.status, StatusCode::NOT_FOUND);
    // Refused by the framework, and made a problem.
    let response = server.client.get("/boards/%FF").await;
    assert_eq!(response.status, StatusCode::BAD_REQUEST);
    let response = server.client.get("/hello").await;
    assert_eq!(response.status, StatusCode::OK);
    let after = scrape(&server).await;
    let counted = |name: &str| {
        problems(&after, name).unwrap_or_default() - problems(&before, name).unwrap_or_default()
    };
    assert_eq!(counted("unauthorized"), 2.0, "{after}");
    assert_eq!(counted("not-found"), 1.0, "{after}");
    assert_eq!(counted("bad-request"), 1.0, "{after}");
    assert_eq!(
        problem_labels(&before),
        problem_labels(&after),
        "the same problems, counted: {after}"
    );
    server.teardown().await;
}

#[tokio::test]
async fn what_a_client_sends_makes_no_problem_of_its_own() {
    let server = server_with_metrics().await;
    let before = scrape(&server).await;
    let mut asked = Vec::new();
    for n in 0..40 {
        let word = fvoci_model::new_id();
        let path = match n % 4 {
            0 => format!("/api/v1/{word}"),
            1 => format!("/api/v1/boards/{word}"),
            2 => format!("/api/v1/problems/{word}"),
            _ => format!("/problems/{word}"),
        };
        let method = if n % 2 == 0 {
            Method::GET
        } else {
            Method::from_bytes(format!("X{n}").as_bytes()).expect("a method")
        };
        Client::send(server.client.raw_request(method, &path)).await;
        asked.push(word);
    }
    let after = scrape(&server).await;
    let labels = problem_labels(&after);
    assert_eq!(labels, problem_labels(&before), "no new problem: {after}");
    for label in &labels {
        assert!(
            !label.contains('/') && label.len() < 40,
            "a name, and nothing asked: {label}"
        );
    }
    for word in &asked {
        assert!(
            !after.contains(word.as_str()),
            "nothing asked is a label: {after}"
        );
    }
    server.teardown().await;
}

#[tokio::test]
async fn the_calls_of_the_database_are_measured() {
    let server = server_with_metrics().await;
    let before = scrape(&server).await;
    let db = server.app.store().db_type().as_str();
    let reads = |body: &str| {
        sample(
            body,
            "fvoci_db_call_duration_seconds_count",
            &[("access", "read"), ("db", db)],
        )
        .unwrap_or_default()
    };
    let waits = |body: &str| {
        sample(body, "fvoci_db_pool_wait_seconds_count", &[("db", db)]).unwrap_or_default()
    };
    let response = server.client.get("/ping").await;
    assert_eq!(response.status, StatusCode::OK);
    let after = scrape(&server).await;
    // The scrape reads once itself, for the open uploads.
    assert!(reads(&after) >= reads(&before) + 2.0, "ping reads: {after}");
    assert!(
        waits(&after) >= waits(&before) + 2.0,
        "and waits for a connection: {after}"
    );
    for access in ["read", "write"] {
        assert_eq!(
            sample(
                &after,
                "fvoci_db_call_failures_total",
                &[("access", access), ("db", db)]
            ),
            Some(0.0),
            "nothing failed, and the count is there from the start: {after}"
        );
    }
    for line in after.lines().filter(|line| line.starts_with("fvoci_db_")) {
        let db_named = ["db=\"sqlite3\"", "db=\"postgres\""]
            .iter()
            .any(|value| line.contains(value));
        assert!(db_named || !line.contains("db="), "{line}");
        if line.contains("access=") {
            assert!(
                line.contains("access=\"read\"") || line.contains("access=\"write\""),
                "{line}"
            );
        }
    }
    server.teardown().await;
}

#[tokio::test]
async fn a_server_without_metrics_gives_its_store_no_observer() {
    let server = Setup::native().start().await;
    assert!(
        server.app.store().observe(Box::new(NoObserver)),
        "the store had none"
    );
    server.teardown().await;
}

/// An observer that is told nothing it keeps.
#[derive(Debug)]
struct NoObserver;

impl fvoci_store::Observer for NoObserver {
    fn called(&self, _: fvoci_store::Call) {}

    fn waited_for_connection(&self, _: fvoci_store::DbType, _: std::time::Duration) {}
}
