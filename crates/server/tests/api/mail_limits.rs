//! What invitation mail may hold of the SMTP work: its own queue, three of the four
//! connections, recipients per request and messages per workspace and hour; and
//! recovery answering as always when its queue is full.

use std::time::Duration;

use fvoci_model::Invitation;
use reqwest::StatusCode;
use serde_json::{Value, json};

use crate::common::{ROOT_WORKSPACE_ID, Setup, TestServer};
use crate::fake_smtp::FakeSmtp;
use crate::metrics::{sample, scrape_until};

/// The token of a new member link of the root workspace, made by user1.
async fn member_link(th: &TestServer) -> String {
    let link: Invitation = th
        .client
        .post(
            &format!("/w/{ROOT_WORKSPACE_ID}/invitations"),
            &json!({"role":"member"}),
        )
        .await
        .created_json();
    link.token.expect("a new link has its token")
}

/// `count` addresses `<prefix><n><suffix>`.
fn addresses(prefix: &str, count: usize, suffix: &str) -> Vec<String> {
    (0..count).map(|n| format!("{prefix}{n}{suffix}")).collect()
}

fn mail_path() -> String {
    format!("/w/{ROOT_WORKSPACE_ID}/invitations/mail")
}

#[tokio::test]
async fn invitations_never_take_the_last_connection_from_recovery() {
    let smtp = FakeSmtp::start_on(0).await;
    smtp.hold("@held.test");
    let th = Setup::native()
        .smtp(smtp.settings())
        .configure(|config| config.smtp.mail_invitation_queue = 20)
        .start()
        .await
        .init_basic()
        .await;
    let token = member_link(&th).await;
    // Four requests deliver one message at a time each: more than the connections
    // invitations may use, and as many messages as their queue holds.
    for batch in 0..4 {
        th.client
            .post(
                &mail_path(),
                &json!({"token":token,"recipients":addresses(&format!("b{batch}-"), 5, "@held.test")}),
            )
            .await
            .check_no_content();
    }
    smtp.attempted(3).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        smtp.attempts(),
        3,
        "invitations hold three connections at most"
    );
    th.client
        .post(
            &mail_path(),
            &json!({"token":token,"recipients":["more@held.test"]}),
        )
        .await
        .check_status(StatusCode::SERVICE_UNAVAILABLE);

    th.anonymous()
        .post(
            "/password-reset/request",
            &json!({"email":"user1@sample.com"}),
        )
        .await
        .check_no_content();
    let messages = smtp.received(1).await;
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert_eq!(messages[0].recipient, "user1@sample.com");

    smtp.release();
    smtp.received(21).await;
    assert!(th.app.finish_background(Duration::from_secs(10)).await);
    assert_eq!(smtp.messages().await.len(), 21);
    th.teardown().await;
    smtp.stop().await;
}

#[tokio::test]
async fn an_invitation_mail_request_names_at_most_its_recipients_limit() {
    let smtp = FakeSmtp::start_on(0).await;
    let th = Setup::native()
        .smtp(smtp.settings())
        .start()
        .await
        .init_basic()
        .await;
    let token = member_link(&th).await;
    // Repeated addresses count: the limit is on what the request names.
    let mut recipients = addresses("r", 20, "@example.test");
    recipients.push("r0@example.test".to_owned());
    let refused = th
        .client
        .post(
            &mail_path(),
            &json!({"token":token,"recipients":recipients}),
        )
        .await;
    refused.check_status(StatusCode::BAD_REQUEST);
    let body: Value = serde_json::from_slice(&refused.body).expect("problem");
    assert_eq!(body["type"], "/problems/too-many-recipients", "{body}");
    assert_eq!(body["reason"], "too_many_recipients", "{body}");
    assert_eq!(body["limit"], 20, "{body}");
    assert!(body.to_string().contains("recipients"), "{body}");
    assert!(th.app.finish_background(Duration::from_secs(5)).await);
    assert!(smtp.messages().await.is_empty(), "nothing of it is sent");

    recipients.pop();
    th.client
        .post(
            &mail_path(),
            &json!({"token":token,"recipients":recipients}),
        )
        .await
        .check_no_content();
    smtp.received(20).await;
    th.teardown().await;
    smtp.stop().await;
}

#[tokio::test]
async fn a_workspace_waits_once_it_queued_its_hourly_invitation_mail() {
    let smtp = FakeSmtp::start_on(0).await;
    let th = Setup::native()
        .smtp(smtp.settings())
        .configure(|config| {
            config.smtp.invitation_mail_recipients = 3;
            config.smtp.invitation_mail_per_hour = 5;
        })
        .start()
        .await
        .init_basic()
        .await;
    let token = member_link(&th).await;
    th.client
        .post(
            &mail_path(),
            &json!({"token":token,"recipients":addresses("a", 3, "@example.test")}),
        )
        .await
        .check_no_content();
    let refused = th
        .client
        .post(
            &mail_path(),
            &json!({"token":token,"recipients":addresses("b", 3, "@example.test")}),
        )
        .await;
    refused.check_status(StatusCode::TOO_MANY_REQUESTS);
    let wait: u64 = refused
        .headers
        .get("retry-after")
        .expect("Retry-After")
        .to_str()
        .expect("text")
        .parse()
        .expect("seconds");
    assert!((3590..=3600).contains(&wait), "{wait}");
    let body: Value = serde_json::from_slice(&refused.body).expect("problem");
    assert_eq!(body["type"], "/problems/invitation-mail-limit", "{body}");
    assert_eq!(body["retryAfter"], wait, "{body}");
    // What is left of the hour is still taken.
    th.client
        .post(
            &mail_path(),
            &json!({"token":token,"recipients":addresses("c", 2, "@example.test")}),
        )
        .await
        .check_no_content();
    smtp.received(5).await;
    assert!(th.app.finish_background(Duration::from_secs(5)).await);
    let messages = smtp.messages().await;
    assert_eq!(messages.len(), 5);
    assert!(
        messages
            .iter()
            .all(|message| !message.recipient.starts_with('b')),
        "the refused request sent nothing"
    );
    th.teardown().await;
    smtp.stop().await;
}

#[tokio::test]
async fn a_full_invitation_queue_refuses_a_whole_request_before_answering() {
    let smtp = FakeSmtp::start_on(0).await;
    smtp.hold("@held.test");
    let th = Setup::native()
        .smtp(smtp.settings())
        .configure(|config| {
            config.smtp.mail_invitation_queue = 3;
            config.smtp.invitation_mail_recipients = 3;
        })
        .start()
        .await
        .init_basic()
        .await;
    let token = member_link(&th).await;
    th.client
        .post(
            &mail_path(),
            &json!({"token":token,"recipients":addresses("q", 3, "@held.test")}),
        )
        .await
        .check_no_content();
    smtp.attempted(1).await;
    let refused = th
        .client
        .post(
            &mail_path(),
            &json!({"token":token,"recipients":["late@example.test"]}),
        )
        .await;
    refused.check_status(StatusCode::SERVICE_UNAVAILABLE);
    let body: Value = serde_json::from_slice(&refused.body).expect("problem");
    assert_eq!(body["type"], "/problems/mail-queue-full", "{body}");

    smtp.release();
    smtp.received(3).await;
    assert!(th.app.finish_background(Duration::from_secs(10)).await);
    let messages = smtp.messages().await;
    assert_eq!(messages.len(), 3, "{messages:?}");
    assert!(
        messages
            .iter()
            .all(|message| message.recipient != "late@example.test")
    );
    // The queue empties as messages leave, and the refusal used none of the hour.
    th.client
        .post(
            &mail_path(),
            &json!({"token":token,"recipients":["late@example.test"]}),
        )
        .await
        .check_no_content();
    smtp.received(4).await;
    th.teardown().await;
    smtp.stop().await;
}

#[tokio::test]
async fn recovery_answers_the_same_when_its_queue_is_full_and_counts_the_drop() {
    let smtp = FakeSmtp::start_on(0).await;
    smtp.hold("@sample.com");
    let th = Setup::native()
        .smtp(smtp.settings())
        .metrics_at("127.0.0.1:0")
        .configure(|config| config.smtp.mail_reset_queue = 1)
        .start()
        .await
        .init_basic()
        .await;
    let first = th
        .anonymous()
        .post(
            "/password-reset/request",
            &json!({"email":"user1@sample.com"}),
        )
        .await;
    smtp.attempted(1).await;
    let dropped = th
        .anonymous()
        .post(
            "/password-reset/request",
            &json!({"email":"user2@sample.com"}),
        )
        .await;
    for answer in [&first, &dropped] {
        answer.check_status(StatusCode::NO_CONTENT);
        assert_eq!(answer.body, b"");
    }
    let headers = |answer: &crate::common::ApiResponse| {
        let mut names: Vec<String> = answer
            .headers
            .keys()
            .map(|name| name.as_str().to_owned())
            .filter(|name| name != "date" && name != "x-request-id")
            .collect();
        names.sort();
        names
    };
    assert_eq!(headers(&first), headers(&dropped));
    let body = scrape_until(&th, |body| {
        sample(
            body,
            "fvoci_password_reset_mail_dropped_total",
            &[("reason", "queue_full")],
        ) == Some(1.0)
    })
    .await;
    assert!(!body.contains("user2"), "{body}");

    smtp.release();
    smtp.received(1).await;
    assert!(th.app.finish_background(Duration::from_secs(10)).await);
    let messages = smtp.messages().await;
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert_eq!(messages[0].recipient, "user1@sample.com");
    assert_eq!(
        th.app
            .store()
            .count_rows("password_resets")
            .await
            .expect("tokens"),
        1,
        "the dropped request made no token"
    );
    th.teardown().await;
    smtp.stop().await;
}
