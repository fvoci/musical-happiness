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
