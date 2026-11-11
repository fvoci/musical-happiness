//! The metrics of what the server's work came to: the problems the API refused with.

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

#[tokio::test]
async fn refusals_are_counted_by_the_name_of_their_problem() {
    let server = server_with_metrics().await;
    for _ in 0..2 {
        let response = server.client.get("/users/me").await;
        assert_eq!(response.status, StatusCode::UNAUTHORIZED);
    }
    let response = server.client.get("/no-such-route").await;
    assert_eq!(response.status, StatusCode::NOT_FOUND);
    let response = server.client.get("/hello").await;
    assert_eq!(response.status, StatusCode::OK);
    let body = scrape(&server).await;
    assert_eq!(problems(&body, "unauthorized"), Some(2.0), "{body}");
    assert_eq!(problems(&body, "not-found"), Some(1.0), "{body}");
    assert_eq!(
        series(&body, "fvoci_http_problems_total").len(),
        2,
        "an answer that is no problem is not counted: {body}"
    );
    server.teardown().await;
}

#[tokio::test]
async fn what_a_client_sends_makes_no_problem_of_its_own() {
    let server = server_with_metrics().await;
    let name = "fvoci_http_problems_total";
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
    let body = scrape(&server).await;
    let lines = series(&body, name);
    assert!(
        (1..=3).contains(&lines.len()),
        "a few problems, whatever was asked: {lines:?}"
    );
    for word in &asked {
        assert!(
            !body.contains(word.as_str()),
            "nothing asked is a label: {body}"
        );
    }
    server.teardown().await;
}
