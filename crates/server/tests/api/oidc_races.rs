//! Signing in through a provider when things happen meanwhile: the provider changed
//! while the person is at it, a sign-in that ended or comes back twice, a start that
//! another site's page or a forged host would make. First written by an independent
//! review of the branch (`fvoci/review-oidc-i1`).

use std::time::Duration;

use base64::Engine as _;
use fvoci_model::{OidcProvider, OidcStart, UserWorkspace};
use fvoci_server::app::{Callback, LoginFailure};
use fvoci_server::auth::sealed::SecretKey;
use fvoci_server::auth::secret::token_digest;
use fvoci_server::config::Configuration;
use fvoci_server::error::Reason;
use serde_json::json;

use crate::common::{Setup, TestServer};
use crate::fake_oidc::{self, CLIENT_ID, CLIENT_SECRET, FakeProvider};

fn configure(config: &mut Configuration) {
    config.secret_key = Some(
        SecretKey::from_base64(&base64::engine::general_purpose::STANDARD.encode([42_u8; 32]))
            .expect("a test key"),
    );
    config.oidc_allow_private_addresses = true;
}

async fn server_for(fake: &FakeProvider) -> (TestServer, String) {
    let ts = Setup::native()
        .trusting_oidc_provider(&fake.certificate_der)
        .configure(configure)
        .start()
        .await
        .init_basic()
        .await;
    let workspaces: Vec<UserWorkspace> = ts.client.get("/users/me/workspaces").await.ok_json();
    let org_id = workspaces
        .first()
        .expect("the administrator's workspace")
        .org_id
        .clone();
    ts.client
        .put(
            &format!("/org/{org_id}/oidc"),
            &json!({ "issuer": fake.issuer, "clientId": CLIENT_ID, "clientSecret": CLIENT_SECRET }),
        )
        .await
        .check_ok();
    (ts, org_id)
}

/// A real flow and a code issued for its nonce, redirect address and PKCE challenge.
async fn callback_for(ts: &TestServer, fake: &FakeProvider, org_id: &str) -> Callback {
    let started = ts
        .app
        .start_oidc_login(org_id, None)
        .await
        .expect("the sign-in starts");
    let http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(10))
        .tls_certs_merge([
            reqwest::Certificate::from_der(&fake.certificate_der).expect("a certificate")
        ])
        .build()
        .expect("a client");
    let authorized = http
        .get(&started.authorization_url)
        .send()
        .await
        .expect("the provider answers");
    assert_eq!(
        authorized.status(),
        reqwest::StatusCode::FOUND,
        "the provider sends the browser back"
    );
    let location = authorized
        .headers()
        .get(reqwest::header::LOCATION)
        .expect("the callback address");
    let url = reqwest::Url::parse(location.to_str().expect("an address")).expect("a callback URL");
    let get = |name: &str| {
        url.query_pairs()
            .find(|(key, _)| key == name)
            .expect("the callback parameter")
            .1
            .into_owned()
    };
    Callback {
        state: get("state"),
        code: get("code"),
        browser_secret: started.browser_secret,
        error: None,
    }
}

#[derive(Debug, Clone, Copy)]
enum Change {
    Disable,
    Delete,
    CloseJoining,
}

/// Pause only this callback after its token request reaches the real fake provider.
/// The administrator's HTTP request then completes before verification and joining
/// continue. No production code, clock or provider response is replaced.
async fn changed_provider_refuses_pending_callback(change: Change) {
    let fake = FakeProvider::start().await;
    let (ts, org_id) = server_for(&fake).await;
    let callback = callback_for(&ts, &fake, &org_id).await;
    let users_before = ts
        .app
        .store()
        .count_rows("users")
        .await
        .expect("users counted");
    let (_, outcome) = {
        let finishing = ts.app.finish_oidc_login(&callback, "independent review");
        tokio::pin!(finishing);
        tokio::time::timeout(Duration::from_secs(10), async {
            tokio::select! {
                biased;
                () = async {
                    while fake.token_requests() == 0 {
                        tokio::task::yield_now().await;
                    }
                } => {}
                result = &mut finishing => panic!("the callback completed before the checkpoint: {result:?}"),
            }
        })
        .await
        .expect("the callback reaches the token exchange");
        let path = format!("/org/{org_id}/oidc");
        match change {
            Change::Delete => {
                let deleted = ts.client.delete(&path).await;
                assert_eq!(
                    deleted.status,
                    reqwest::StatusCode::NO_CONTENT,
                    "the provider is deleted"
                );
                ts.client.get(&path).await.check_not_found();
            }
            Change::Disable | Change::CloseJoining => {
                let changed: OidcProvider = ts
                    .client
                    .put(
                        &path,
                        &json!({
                            "issuer": fake.issuer,
                            "clientId": CLIENT_ID,
                            "enabled": !matches!(change, Change::Disable),
                            "autoJoin": !matches!(change, Change::CloseJoining),
                        }),
                    )
                    .await
                    .ok_json();
                assert_eq!(
                    changed.enabled,
                    !matches!(change, Change::Disable),
                    "the enabled flag was updated"
                );
                assert_eq!(
                    changed.auto_join,
                    !matches!(change, Change::CloseJoining),
                    "the auto-join flag was updated"
                );
            }
        }
        tokio::time::timeout(Duration::from_secs(10), finishing)
            .await
            .expect("the callback finishes")
    };
    let live_session = match &outcome {
        Ok(finished) => ts
            .app
            .store()
            .get_session(&token_digest(&finished.token), 0, 0)
            .await
            .is_ok(),
        Err(_) => false,
    };
    let users_after = ts
        .app
        .store()
        .count_rows("users")
        .await
        .expect("users counted");
    ts.teardown().await;
    fake.stop();
    let expected = match change {
        Change::Disable | Change::Delete => Reason::OidcUnavailable,
        Change::CloseJoining => Reason::OidcJoinClosed,
    };
    assert_eq!(
        outcome.err(),
        Some(LoginFailure::Reason(expected)),
        "{change:?} completed before the callback finished, but users changed from {users_before} to {users_after}, live session: {live_session}",
    );
    assert_eq!(users_after, users_before, "no newcomer after {change:?}");
    assert!(!live_session, "no session after {change:?}");
}

#[tokio::test]
async fn disabling_a_provider_during_token_exchange_prevents_a_new_session() {
    changed_provider_refuses_pending_callback(Change::Disable).await;
}

#[tokio::test]
async fn deleting_a_provider_during_token_exchange_prevents_a_new_session() {
    changed_provider_refuses_pending_callback(Change::Delete).await;
}

#[tokio::test]
async fn closing_auto_join_during_token_exchange_prevents_a_new_member() {
    changed_provider_refuses_pending_callback(Change::CloseJoining).await;
}

#[tokio::test]
async fn an_expired_flow_is_consumed_without_exchanging_its_code() {
    let fake = FakeProvider::start().await;
    let (ts, org_id) = server_for(&fake).await;
    let callback = callback_for(&ts, &fake, &org_id).await;
    ts.app
        .store()
        .for_org(&org_id)
        .expect("the organization")
        .execute_unchecked("UPDATE oidc_login_flows SET expires_at = 0")
        .await
        .expect("the flow expires");
    let (_, outcome) = ts
        .app
        .finish_oidc_login(&callback, "independent review")
        .await;
    let flow_count = ts
        .app
        .store()
        .for_org(&org_id)
        .expect("the organization")
        .count_rows("oidc_login_flows")
        .await
        .expect("flows counted");
    let token_requests = fake.token_requests();
    ts.teardown().await;
    fake.stop();
    assert_eq!(
        outcome.err(),
        Some(LoginFailure::Reason(Reason::OidcStateInvalid)),
        "an expired flow is refused",
    );
    assert_eq!(flow_count, 0, "the expired flow is removed");
    assert_eq!(
        token_requests, 0,
        "no code is exchanged for an expired flow"
    );
}

#[tokio::test]
async fn concurrent_callbacks_consume_a_flow_once() {
    let fake = FakeProvider::start().await;
    let (ts, org_id) = server_for(&fake).await;
    let callback = callback_for(&ts, &fake, &org_id).await;
    let ((_, first), (_, second)) = tokio::join!(
        ts.app.finish_oidc_login(&callback, "first return"),
        ts.app.finish_oidc_login(&callback, "second return"),
    );
    let accepted = usize::from(first.is_ok()) + usize::from(second.is_ok());
    let refusals: Vec<_> = [first.err(), second.err()].into_iter().flatten().collect();
    let token_requests = fake.token_requests();
    ts.teardown().await;
    fake.stop();
    assert_eq!(accepted, 1, "only one callback opens a session");
    assert_eq!(
        refusals,
        [LoginFailure::Reason(Reason::OidcStateInvalid)],
        "the other callback finds no flow"
    );
    assert_eq!(token_requests, 1, "only one code exchange");
}

#[tokio::test]
async fn a_start_with_a_forged_host_keeps_the_configured_callback_address() {
    let fake = FakeProvider::start().await;
    let (ts, org_id) = server_for(&fake).await;
    let http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(10))
        .build()
        .expect("a client");
    let started = http
        .post(format!(
            "{}/api/v1/auth/oidc/start?org={org_id}",
            ts.base_url
        ))
        .header("X-Requested-With", "XMLHttpRequest")
        .header("Host", "attacker.example")
        .header("X-Forwarded-Host", "attacker.example")
        .header("X-Forwarded-Proto", "https")
        .send()
        .await
        .expect("the server answers");
    let status = started.status();
    let answer: OidcStart = started.json().await.expect("where the browser goes");
    let authorization = reqwest::Url::parse(&answer.authorization_url).expect("a URL");
    let redirect = authorization
        .query_pairs()
        .find(|(key, _)| key == "redirect_uri")
        .expect("the callback address")
        .1
        .into_owned();
    let expected = format!(
        "{}/api/v1/auth/oidc/callback",
        ts.app.config().server_root.trim_end_matches('/')
    );
    ts.teardown().await;
    fake.stop();
    assert_eq!(
        status,
        reqwest::StatusCode::OK,
        "the start answers where the browser goes"
    );
    assert_eq!(
        redirect, expected,
        "request headers cannot choose the callback host"
    );
}
/// Fetch Metadata is omitted for ordinary HTTP origins. The same absent-header
/// request can come from a real browser following a link on another site's page;
/// it is not evidence that the caller is a non-browser client. The provider used
/// here signs every arriving browser in as an account chosen by its operator. So no
/// start may be one that another site's page can make a browser send: not an address
/// it is sent to, not a form posted to it, whatever headers of Fetch Metadata come or
/// do not come. Only a request of the server's own page starts a sign-in.
#[tokio::test]
async fn a_cross_site_navigation_without_fetch_metadata_cannot_sign_in() {
    let fake = FakeProvider::start().await;
    let (ts, org_id) = server_for(&fake).await;
    fake.sign_in_as(fake_oidc::Person::new("attacker", "attacker@example.com"));
    let http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(10))
        .build()
        .expect("a browser client");
    let start = format!("{}/api/v1/auth/oidc/start?org={org_id}", ts.base_url);
    let flows_before = flows(&ts, &org_id).await;
    let sends = [
        // Followed from a link or sent there by a script of another page.
        http.get(&start)
            .header("Referer", "http://attacker.example/"),
        // A form of another page, posted: no header of the server's scripts.
        http.post(&start)
            .header("Origin", "http://attacker.example")
            .header("Content-Type", "application/x-www-form-urlencoded"),
        // Even with that header (which a page of another site cannot add), a foreign origin.
        http.post(&start)
            .header("Origin", "http://attacker.example")
            .header("X-Requested-With", "XMLHttpRequest"),
    ];
    let mut answers = Vec::new();
    for send in sends {
        let answer = send.send().await.expect("the server answers");
        let flow_cookie = answer
            .headers()
            .get_all(reqwest::header::SET_COOKIE)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .any(|cookie| cookie.starts_with("fvoci_oidc="));
        answers.push((answer.status(), flow_cookie));
    }
    let flows_after = flows(&ts, &org_id).await;
    ts.teardown().await;
    fake.stop();
    assert_eq!(
        answers,
        [
            (reqwest::StatusCode::METHOD_NOT_ALLOWED, false),
            (reqwest::StatusCode::FORBIDDEN, false),
            (reqwest::StatusCode::FORBIDDEN, false),
        ],
        "no sign-in starts from another site's page"
    );
    assert_eq!(flows_after, flows_before, "no sign-in was stored");
}

/// The sign-ins under way of an organization.
async fn flows(ts: &TestServer, org_id: &str) -> i64 {
    ts.app
        .store()
        .for_org(org_id)
        .expect("the organization")
        .count_rows("oidc_login_flows")
        .await
        .expect("flows counted")
}
