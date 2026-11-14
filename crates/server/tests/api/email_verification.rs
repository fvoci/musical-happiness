//! Registrations that wait for their address (`email_verification`): nothing until the
//! mailed link, which makes the account and the organization and signs in.

use std::time::Duration;

use fvoci_store::{SIGNUP_LINK_LIFETIME_MILLIS, SignupLookup};
use reqwest::StatusCode;
use serde_json::{Value, json};

use crate::common::{Setup, TestServer};
use crate::fake_smtp::{FakeSmtp, Received};

/// A server that asks registrations to confirm their address, mailing through `smtp`.
pub(crate) async fn verifying(smtp: &FakeSmtp) -> TestServer {
    Setup::native()
        .smtp(smtp.settings())
        .configure(|config| config.mail.email_verification = true)
        .start()
        .await
}

/// The registration request of `username`, at `username@example.test`.
pub(crate) fn registration(username: &str, language: &str) -> Value {
    json!({
        "username": username,
        "email": format!("{username}@example.test"),
        "password": "Pa$$word-1",
        "organizationName": format!("{username} works"),
        "language": language,
    })
}

/// The token of the confirmation link in `message`.
pub(crate) fn link_token(message: &Received) -> String {
    let link = message
        .decoded
        .lines()
        .find(|line| line.starts_with("http") && line.contains("/verify-email#"))
        .expect("a confirmation link in the plain text");
    link.split_once('#')
        .expect("a fragment")
        .1
        .trim()
        .to_owned()
}

#[tokio::test]
async fn a_registration_opens_nothing_until_its_mailed_link_signs_it_in() {
    let smtp = FakeSmtp::start_on(0).await;
    let th = verifying(&smtp).await;
    let options = th.anonymous().get("/login-options").await.value();
    assert_eq!(options["emailVerification"], true, "{options}");

    let answer = th
        .anonymous()
        .post("/register", &registration("dana", "ko"))
        .await;
    answer.check_status(StatusCode::ACCEPTED);
    assert_eq!(answer.session_cookie(), None, "no session before the link");
    assert!(
        answer.text().is_empty(),
        "nothing of an account: {}",
        answer.text()
    );
    assert!(
        th.app
            .store()
            .get_user_by_username("dana")
            .await
            .is_err_and(|error| error.is_not_found()),
        "no account"
    );

    let messages = smtp.received(1).await;
    let message = &messages[0];
    assert_eq!(message.recipient, "dana@example.test");
    assert!(
        message.decoded.contains("lang=\"ko\""),
        "the screen's language"
    );
    let signup = th
        .app
        .store()
        .live_signup(
            SignupLookup::Username("dana"),
            fvoci_server::auth::now_millis(),
        )
        .await
        .expect("read")
        .expect("the registration");
    assert!(
        !message.decoded.contains(&signup.id),
        "no identifier in the mail"
    );
    let token = link_token(message);

    names_are_held_and_logins_refused(&th).await;

    // Another browser than the one that registered: the mailbox is the proof.
    let confirmed = th
        .anonymous()
        .post("/email-verification/complete", &json!({"token": token}))
        .await;
    confirmed.check_status(StatusCode::OK);
    let session = confirmed.session_cookie().expect("a session");
    let workspace_id = confirmed.value()["workspaceId"]
        .as_str()
        .expect("where it was let in")
        .to_owned();
    let signed_in = th.client_with_token(&session);
    let me = signed_in.me().await;
    assert_eq!(me.username, "dana");
    assert_eq!(me.id, signup.id, "the account is the registration's");
    let workspace = signed_in.get(&format!("/w/{workspace_id}")).await;
    workspace.check_status(StatusCode::OK);
    assert_eq!(
        workspace.value()["title"],
        "dana works",
        "{}",
        workspace.text()
    );

    // Used once.
    let again = th
        .anonymous()
        .post("/email-verification/complete", &json!({"token": token}))
        .await;
    again.check_status(StatusCode::BAD_REQUEST);
    assert_eq!(again.refused_at(), "/token");
    th.anonymous()
        .login("dana", "Pa$$word-1")
        .await
        .check_no_content();
    th.teardown().await;
    smtp.stop().await;
}

/// While `dana` waits for the link: the same name or address again is the answer for an
/// account, and a login is refused as unconfirmed for the right password only.
async fn names_are_held_and_logins_refused(th: &TestServer) {
    // The same name or address again is the answer for an account.
    for (taken, reason) in [
        (
            json!({"username": "DANA", "email": "other@example.test"}),
            "username_taken",
        ),
        (
            json!({"username": "other", "email": "Dana@Example.test"}),
            "email_taken",
        ),
    ] {
        let mut request = registration("other", "en");
        request["username"] = taken["username"].clone();
        request["email"] = taken["email"].clone();
        let answer = th.anonymous().post("/register", &request).await;
        answer.check_status(StatusCode::CONFLICT);
        assert_eq!(answer.error_reason(), reason, "{taken}");
    }

    let mut browser = th.anonymous();
    browser
        .login("dana", "Pa$$word-1")
        .await
        .check_status(StatusCode::FORBIDDEN);
    let refused = browser.login("dana", "Pa$$word-1").await;
    assert_eq!(
        refused.error_reason(),
        "email_unverified",
        "the right password"
    );
    assert_eq!(
        refused.problem_type(),
        "/problems/email-unverified",
        "a problem of its own"
    );
    let wrong = browser.login("dana", "wrong-password").await;
    wrong.check_status(StatusCode::UNAUTHORIZED);
    assert_eq!(
        wrong.error_reason(),
        "invalid_credentials",
        "as for no account"
    );
    let by_email = th
        .anonymous()
        .post(
            "/login",
            &json!({"email": "dana@example.test", "password": "Pa$$word-1"}),
        )
        .await;
    assert_eq!(
        by_email.error_reason(),
        "email_unverified",
        "by its address too"
    );
}

#[tokio::test]
async fn a_link_confirms_for_a_day_and_two_at_once_confirm_once() {
    let smtp = FakeSmtp::start_on(0).await;
    let th = verifying(&smtp).await;
    th.anonymous()
        .post("/register", &registration("erin", "en"))
        .await
        .check_status(StatusCode::ACCEPTED);
    let messages = smtp.received(1).await;
    assert!(messages[0].decoded.contains("lang=\"en\""));
    let token = link_token(&messages[0]);
    assert!(th.app.finish_background(Duration::from_secs(5)).await);

    // The link a day old: as if made a day and a moment ago.
    let store = th.app.store();
    let now = fvoci_server::auth::now_millis();
    let signup = store
        .live_signup(SignupLookup::Email("ERIN@example.test"), now)
        .await
        .expect("read")
        .expect("the registration");
    let digest = fvoci_server::auth::secret::token_digest(&token);
    assert!(
        store
            .renew_signup_link(&signup.id, &digest, now - SIGNUP_LINK_LIFETIME_MILLIS - 1)
            .await
            .expect("renewed")
    );
    let ended = th
        .anonymous()
        .post("/email-verification/complete", &json!({"token": token}))
        .await;
    ended.check_status(StatusCode::BAD_REQUEST);
    assert_eq!(ended.refused_at(), "/token");

    assert!(
        store
            .renew_signup_link(&signup.id, &digest, now)
            .await
            .expect("renewed")
    );
    let body = json!({"token": token});
    let (one, two) = (th.anonymous(), th.anonymous());
    let (first, second) = tokio::join!(
        one.post("/email-verification/complete", &body),
        two.post("/email-verification/complete", &body),
    );
    let mut statuses = [first.status, second.status];
    statuses.sort();
    assert_eq!(statuses, [StatusCode::OK, StatusCode::BAD_REQUEST]);
    let sessions = [first.session_cookie(), second.session_cookie()];
    assert_eq!(sessions.iter().flatten().count(), 1, "one session");
    th.teardown().await;
    smtp.stop().await;
}

#[tokio::test]
async fn a_server_that_does_not_ask_registers_and_answers_as_it_did() {
    let smtp = FakeSmtp::start_on(0).await;
    let th = Setup::native().smtp(smtp.settings()).start().await;
    let options = th.anonymous().get("/login-options").await.value();
    assert_eq!(options["emailVerification"], false, "{options}");
    let answer = th
        .anonymous()
        .post("/register", &registration("fern", "ko"))
        .await;
    answer.check_status(StatusCode::CREATED);
    assert_eq!(answer.value()["username"], "fern");
    th.anonymous()
        .login("fern", "Pa$$word-1")
        .await
        .check_no_content();
    assert!(th.app.finish_background(Duration::from_secs(5)).await);
    assert!(smtp.messages().await.is_empty(), "no mail");
    th.teardown().await;
    smtp.stop().await;
}

/// Confirms the registration whose mail is `message`, and answers as the server did.
async fn confirm(th: &TestServer, message: &Received) -> crate::common::ApiResponse {
    th.anonymous()
        .post(
            "/email-verification/complete",
            &json!({"token": link_token(message)}),
        )
        .await
}

#[tokio::test]
async fn a_server_of_one_organization_drops_the_other_registrations_once_it_has_its_first_user() {
    let smtp = FakeSmtp::start_on(0).await;
    let th = verifying(&smtp).await;
    for username in ["gina", "hugo"] {
        th.anonymous()
            .post("/register", &registration(username, "en"))
            .await
            .check_status(StatusCode::ACCEPTED);
    }
    let messages = smtp.received(2).await;
    let of = |recipient: &str| {
        messages
            .iter()
            .find(|message| message.recipient == recipient)
            .expect("a message to each")
    };
    confirm(&th, of("hugo@example.test"))
        .await
        .check_status(StatusCode::OK);
    // Gina's registration can start nothing now: it went with Hugo's confirmation, and
    // her name and address are free for an invitation.
    assert!(
        th.app
            .store()
            .live_signup(
                SignupLookup::Username("gina"),
                fvoci_server::auth::now_millis()
            )
            .await
            .expect("read")
            .is_none()
    );
    confirm(&th, of("gina@example.test"))
        .await
        .check_status(StatusCode::BAD_REQUEST);
    th.teardown().await;
    smtp.stop().await;
}

#[tokio::test]
async fn a_server_of_several_organizations_starts_one_for_each_confirmed_registration() {
    if fvoci_store::testing::db_type_of_the_tests() != fvoci_store::DbType::Postgres {
        return;
    }
    let smtp = FakeSmtp::start_on(0).await;
    let th = Setup::native()
        .smtp(smtp.settings())
        .configure(|config| {
            config.mail.email_verification = true;
            config.org_creation = fvoci_server::config::ORG_CREATION_OPEN.to_owned();
        })
        .start()
        .await;
    let mut workspaces = Vec::new();
    for username in ["ivan", "jill"] {
        th.anonymous()
            .post("/register", &registration(username, "en"))
            .await
            .check_status(StatusCode::ACCEPTED);
        let message = smtp
            .received(workspaces.len() + 1)
            .await
            .pop()
            .expect("its message");
        let confirmed = confirm(&th, &message).await;
        confirmed.check_status(StatusCode::OK);
        let session = confirmed.session_cookie().expect("a session");
        let workspace_id = confirmed.value()["workspaceId"]
            .as_str()
            .expect("a workspace")
            .to_owned();
        let me = th.client_with_token(&session).me().await;
        assert_eq!(me.username, username);
        assert_eq!(me.is_admin, username == "ivan", "the first runs the server");
        workspaces.push((session, workspace_id));
    }
    let (ivan, ivans) = &workspaces[0];
    let (_, jills) = &workspaces[1];
    assert_ne!(ivans, jills, "an organization each");
    th.client_with_token(ivan)
        .get(&format!("/w/{jills}"))
        .await
        .check_status(StatusCode::NOT_FOUND);
    th.teardown().await;
    smtp.stop().await;
}
