//! The metrics of the accounts: how logins ended, how many users registered, how many
//! sessions are live; and nothing in them names a user.

use reqwest::StatusCode;

use crate::common::{PASSWORD, Setup, USER1_USERNAME, USER2_USERNAME};
use crate::metrics::{sample, scrape};

/// How many logins ended so.
fn logins(body: &str, outcome: &str) -> Option<f64> {
    sample(body, "fvoci_logins_total", &[("outcome", outcome)])
}

#[tokio::test]
async fn logins_registrations_and_live_sessions_are_counted() {
    let server = Setup::native()
        .metrics_at("127.0.0.1:0")
        .start()
        .await
        .init_basic()
        .await;
    let user_ids = [server.client.me().await.id, server.client2.me().await.id];
    let attempt = async |username: &str, password: &str| {
        server
            .anonymous()
            .post(
                "/login",
                &serde_json::json!({"type": "normal", "username": username, "password": password}),
            )
            .await
            .status
    };
    // A wrong password and an account that is not there are one outcome.
    assert_eq!(
        attempt(USER1_USERNAME, "not the password").await,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        attempt("nobody-at-all", PASSWORD).await,
        StatusCode::UNAUTHORIZED
    );
    let body = scrape(&server).await;
    assert_eq!(
        logins(&body, "succeeded"),
        Some(2.0),
        "the two of init_basic: {body}"
    );
    assert_eq!(logins(&body, "invalid_credentials"), Some(2.0), "{body}");
    assert_eq!(logins(&body, "limited"), Some(0.0), "{body}");
    assert_eq!(logins(&body, "failed"), Some(0.0), "{body}");
    assert_eq!(
        body.lines()
            .filter(|line| line.starts_with("fvoci_logins_total{"))
            .count(),
        4,
        "four outcomes, whatever was asked: {body}"
    );
    assert_eq!(
        sample(&body, "fvoci_registrations_total", &[]),
        Some(2.0),
        "{body}"
    );
    assert_eq!(
        sample(&body, "fvoci_sessions_live", &[]),
        Some(2.0),
        "{body}"
    );
    for named in [
        USER1_USERNAME,
        USER2_USERNAME,
        "nobody-at-all",
        "sample.com",
    ]
    .into_iter()
    .chain(user_ids.iter().map(String::as_str))
    {
        assert!(!body.contains(named), "{named} is in no metric: {body}");
    }

    // A session that ends is live no more.
    let response = server.client2.post_empty("/logout").await;
    assert_eq!(response.status, StatusCode::NO_CONTENT);
    let body = scrape(&server).await;
    assert_eq!(
        sample(&body, "fvoci_sessions_live", &[]),
        Some(1.0),
        "{body}"
    );
    assert_eq!(
        sample(&body, "fvoci_sessions_read_failures_total", &[]),
        Some(0.0),
        "{body}"
    );
    server.teardown().await;
}
