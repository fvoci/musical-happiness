//! Recovering a password through the local socket and the public completion endpoint.

use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

use crate::common::{Client, PASSWORD, Setup};
use crate::rooms::{Incoming, PATIENCE, paragraphs, two_in_a_room};

#[tokio::test]
async fn reset_links_end_sessions_and_collaborative_rooms_and_cannot_be_replayed() {
    let th = Setup::native().start().await.init_basic().await;
    let mut other_session = th.anonymous();
    other_session
        .login("user1", PASSWORD)
        .await
        .check_no_content();
    let (_, mut one, mut two) = two_in_a_room(&th, &paragraphs(&["before reset"])).await;
    let url = th
        .app
        .password_reset_link("user1")
        .await
        .expect("link issued");
    let token = url.split_once('#').expect("a fragment").1;
    assert_eq!(token.len(), 43, "256 random bits in unpadded base64url");
    let body = json!({"token": token, "password": "New-password-1"});
    th.anonymous()
        .post("/password-reset/complete", &body)
        .await
        .check_no_content();
    assert_eq!(
        one.incoming(PATIENCE).await,
        Incoming::Closed,
        "the editor's session ended"
    );
    th.client
        .get("/users/me")
        .await
        .check_status(StatusCode::UNAUTHORIZED);
    other_session
        .get("/users/me")
        .await
        .check_status(StatusCode::UNAUTHORIZED);
    th.client2
        .get("/users/me")
        .await
        .check_status(StatusCode::OK);
    two.write_acked("the other account remains").await;
    for token in [token, "unknown"] {
        th.anonymous()
            .post(
                "/password-reset/complete",
                &json!({"token":token,"password":"Different-password"}),
            )
            .await
            .check_status(StatusCode::BAD_REQUEST);
    }
    let missing = th
        .anonymous()
        .post(
            "/password-reset/complete",
            &json!({"password": "New-password-1"}),
        )
        .await;
    missing.check_error(
        StatusCode::BAD_REQUEST,
        "the reset link is invalid or expired",
    );
    assert_eq!(missing.value()["errors"][0]["pointer"], json!("/token"));
    th.anonymous()
        .login("user1", PASSWORD)
        .await
        .check_status(StatusCode::UNAUTHORIZED);
    th.anonymous()
        .login("user1", "New-password-1")
        .await
        .check_no_content();
    th.teardown().await;
}

#[cfg(unix)]
#[tokio::test]
async fn only_the_unix_socket_can_issue_reset_links() {
    let socket = std::env::temp_dir().join(format!("fvoci-reset-{}.socket", fvoci_model::new_id()));
    let th = Setup::native()
        .admin_socket(&socket)
        .start()
        .await
        .init_basic()
        .await;
    let path = "/api/v1/admin/users/user1/password-reset";
    let http = reqwest::Client::builder()
        .unix_socket(socket.as_path())
        .build()
        .expect("socket client");
    let answer = http
        .post(format!("http://localhost{path}"))
        .send()
        .await
        .expect("socket responds");
    assert_eq!(answer.status(), StatusCode::OK);
    let body: Value = answer.json().await.expect("link response");
    assert!(
        body["url"]
            .as_str()
            .expect("url")
            .contains("/reset-password#")
    );
    for client in [&th.client, &th.client2, &th.anonymous()] {
        Client::send(client.raw_request(Method::POST, path))
            .await
            .check_status(StatusCode::NOT_FOUND);
    }
    th.teardown().await;
}

/// Who belongs only to an organization that takes no password may still reset their
/// password by mail: every session ends, the new password signs in, and the
/// organization opens for its provider's sessions only, as before.
#[tokio::test]
async fn a_reset_password_does_not_open_an_organization_that_takes_none() {
    use crate::common::{ROOT_WORKSPACE_ID, USER1_USERNAME};
    use crate::fake_oidc::FakeProvider;
    use crate::fake_smtp::FakeSmtp;
    use crate::oidc::{org_of, set_provider, with_key_and_private_addresses};
    use crate::sso::{
        ADMIN_SUBJECT, Passwords, admin_through_provider, require_sso, through_provider,
    };

    let fake = FakeProvider::start().await;
    let smtp = FakeSmtp::start_on(0).await;
    let ts = Setup::native()
        .trusting_oidc_provider(&fake.certificate_der)
        .configure(with_key_and_private_addresses)
        .smtp(smtp.settings())
        .start()
        .await
        .init_basic()
        .await;
    let org_id = org_of(&ts).await;
    set_provider(&ts, &org_id, &fake).await;
    let admin = admin_through_provider(&ts, &fake, &org_id).await;
    require_sso(&admin, &org_id, &fake, Passwords::Refused)
        .await
        .check_ok();
    ts.anonymous()
        .post(
            "/password-reset/request",
            &json!({"email": "user1@sample.com"}),
        )
        .await
        .check_no_content();
    let mail = smtp.received(1).await;
    let link = mail[0]
        .decoded
        .lines()
        .find(|line| line.starts_with("http") && line.contains("/reset-password#"))
        .expect("the mail has the link");
    let token = link.split_once('#').expect("a fragment").1.trim();
    ts.anonymous()
        .post(
            "/password-reset/complete",
            &json!({"token": token, "password": "New-password-1"}),
        )
        .await
        .check_no_content();
    for ended in [&ts.client, &admin] {
        ended
            .get("/users/me")
            .await
            .check_status(StatusCode::UNAUTHORIZED);
    }

    let boards = format!("/w/{ROOT_WORKSPACE_ID}/boards");
    let fresh = ts.login(USER1_USERNAME, "New-password-1").await;
    fresh.get("/users/me").await.check_ok();
    let walled = fresh.get(&boards).await;
    walled.check_status(StatusCode::UNAUTHORIZED);
    assert_eq!(walled.error_reason(), "sso_required", "{}", walled.text());
    through_provider(&ts, &fake, &org_id, ADMIN_SUBJECT)
        .await
        .get(&boards)
        .await
        .check_ok();
    ts.teardown().await;
    smtp.stop().await;
    fake.stop();
}
