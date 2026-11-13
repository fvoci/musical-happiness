//! Another secret, client or issuer saved while the organization takes no password: kept
//! aside as the provider's pending settings, nobody signs in through them, and people
//! go on signing in through the provider as it is; a trial through them that ends well
//! makes them the provider's, one that fails changes nothing, and they can be thrown
//! away.

use fvoci_model::{OidcProvider, OidcStart};
use reqwest::StatusCode;
use serde_json::{Value, json};

use crate::common::{Client, ROOT_WORKSPACE_ID, Setup, TestServer};
use crate::fake_oidc::{CLIENT_ID, CLIENT_SECRET, FakeProvider, NEXT_CLIENT_SECRET, Person};
use crate::oidc::{Browser, org_of, server_for, set_provider, with_key_and_private_addresses};
use crate::sso::{ADMIN_SUBJECT, Passwords, admin_through_provider, require_sso};

/// A secret the fake provider does not take at its token endpoint.
const WRONG_SECRET: &str = "a secret the provider never issued";

/// Where a trial came back to, and the session it set if it set one.
#[derive(Debug)]
struct Tried {
    location: String,
    token: Option<String>,
}

/// Tries the pending settings from `admin`'s browser, as the organization's page does,
/// with the fake provider signing `user1` in.
async fn try_pending(ts: &TestServer, fake: &FakeProvider, admin: &Client, org_id: &str) -> Tried {
    try_as(ts, fake, admin, org_id, ADMIN_SUBJECT).await
}

/// Tries the pending settings as [`try_pending`] does, with the fake provider signing
/// in the person `subject`.
async fn try_as(
    ts: &TestServer,
    fake: &FakeProvider,
    admin: &Client,
    org_id: &str,
    subject: &str,
) -> Tried {
    fake.sign_in_as(Person::new(subject, &format!("{subject}@example.com")));
    let http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .tls_certs_merge([
            reqwest::Certificate::from_der(&fake.certificate_der).expect("a certificate")
        ])
        .build()
        .expect("a client");
    let started = http
        .post(format!(
            "{}/api/v1/org/{org_id}/oidc/pending/sign-in",
            ts.base_url
        ))
        .header("X-Requested-With", "XMLHttpRequest")
        .header("Cookie", format!("fvoci_session={}", admin.token))
        .send()
        .await
        .expect("the server answers");
    assert_eq!(started.status(), StatusCode::OK, "the trial starts");
    let flow = cookie_of(&started, "fvoci_oidc").expect("the cookie of the sign-in");
    let started: OidcStart = started.json().await.expect("where the browser goes");
    let authorized = http
        .get(&started.authorization_url)
        .send()
        .await
        .expect("the provider answers");
    let back = location_of(&authorized);
    let root = ts.app.config().server_root.trim_end_matches('/').to_owned();
    let path = back
        .strip_prefix(&root)
        .expect("under the server's address");
    let answer = http
        .get(format!("{}{path}", ts.base_url))
        .header(
            "Cookie",
            format!("fvoci_oidc={flow}; fvoci_session={}", admin.token),
        )
        .send()
        .await
        .expect("the server answers");
    assert_eq!(answer.status(), StatusCode::SEE_OTHER, "a return sends on");
    Tried {
        location: location_of(&answer)
            .strip_prefix(&root)
            .expect("under the server's address")
            .to_owned(),
        token: cookie_of(&answer, "fvoci_session"),
    }
}

fn cookie_of(response: &reqwest::Response, name: &str) -> Option<String> {
    response
        .headers()
        .get_all(reqwest::header::SET_COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .find_map(|cookie| {
            cookie
                .split(';')
                .next()?
                .strip_prefix(&format!("{name}="))
                .map(str::to_owned)
        })
        .filter(|value| !value.is_empty())
}

fn location_of(response: &reqwest::Response) -> String {
    response
        .headers()
        .get(reqwest::header::LOCATION)
        .and_then(|value| value.to_str().ok())
        .expect("a Location header")
        .to_owned()
}

/// The actions of the organization's own records, newest first.
async fn recorded(client: &Client, org_id: &str) -> Vec<String> {
    let records: Vec<Value> = client.get(&format!("/org/{org_id}/audit")).await.ok_json();
    records
        .iter()
        .map(|record| record["action"].as_str().expect("an action").to_owned())
        .collect()
}

/// An organization that takes no password, set up by `user1` through the fake provider:
/// the server, the organization and the administrator's proving session.
async fn enforced(fake: &FakeProvider) -> (TestServer, String, Client) {
    let ts = server_for(fake).await;
    let org_id = org_of(&ts).await;
    set_provider(&ts, &org_id, fake).await;
    let admin = admin_through_provider(&ts, fake, &org_id).await;
    require_sso(&admin, &org_id, fake, Passwords::Refused)
        .await
        .check_ok();
    (ts, org_id, admin)
}

fn with_secret(fake: &FakeProvider, secret: &str) -> Value {
    json!({
        "issuer": fake.issuer, "clientId": CLIENT_ID, "clientSecret": secret,
        "ssoRequired": true,
    })
}

/// A wrong secret saved while the organization takes no password locks nobody out: it
/// waits aside, the provider signs people in as before, and a trial through it fails at
/// the provider and changes nothing.
#[tokio::test]
async fn a_wrong_secret_waits_aside_and_locks_nobody_out() {
    let fake = FakeProvider::start().await;
    let (ts, org_id, admin) = enforced(&fake).await;
    let path = format!("/org/{org_id}/oidc");
    let before: OidcProvider = admin.get(&path).await.ok_json();

    let saved: OidcProvider = admin
        .put(&path, &with_secret(&fake, WRONG_SECRET))
        .await
        .ok_json();
    fake.sign_in_as(Person::new(ADMIN_SUBJECT, "user1@example.com"));
    let meanwhile = Browser::new(&ts, &fake).sign_in(&org_id).await;
    let tried = try_pending(&ts, &fake, &admin, &org_id).await;
    let after: OidcProvider = admin.get(&path).await.ok_json();

    let pending = saved.pending.as_ref().expect("saved aside");
    assert!(pending.has_client_secret && pending.client_id == CLIENT_ID);
    assert!(
        saved.can_require_sso,
        "the provider as it was is still proved"
    );
    assert_eq!(
        saved.update_at, before.update_at,
        "the provider itself unchanged"
    );
    let fresh = ts.client_with_token(meanwhile.token());
    fresh
        .get(&format!("/w/{ROOT_WORKSPACE_ID}/boards"))
        .await
        .check_ok();
    assert_eq!(
        tried.location,
        format!("/org/{org_id}?oidc_pending_error=oidc-token-refused")
    );
    assert_eq!(tried.token, None, "no session");
    assert_eq!(after.pending, saved.pending, "still aside");
    assert_eq!(after.update_at, before.update_at);
    let answered = admin.get(&path).await;
    assert!(
        !answered.text().contains(WRONG_SECRET),
        "{}",
        answered.text()
    );
    assert_eq!(
        recorded(&admin, &org_id).await[0],
        "oidc_provider_pending_saved"
    );
    ts.teardown().await;
    fake.stop();
}

/// The next secret saved aside becomes the provider's by a trial that ends well: the
/// trial's session is made at the new revision and proves it, the session made with
/// the secret before proves it no more, and the records say who saved and who applied
/// it. The stored secret sent again is no change, and sets nothing aside.
#[tokio::test]
async fn the_next_secret_is_applied_by_a_trial() {
    let fake = FakeProvider::start().await;
    let (ts, org_id, admin) = enforced(&fake).await;
    let path = format!("/org/{org_id}/oidc");
    let same: OidcProvider = admin
        .put(&path, &with_secret(&fake, CLIENT_SECRET))
        .await
        .ok_json();
    let saved: OidcProvider = admin
        .put(&path, &with_secret(&fake, NEXT_CLIENT_SECRET))
        .await
        .ok_json();

    let tried = try_pending(&ts, &fake, &admin, &org_id).await;
    let applied = ts.client_with_token(tried.token.as_deref().unwrap_or_default());
    let read: OidcProvider = applied.get(&path).await.ok_json();
    let old_session: OidcProvider = admin.get(&path).await.ok_json();

    assert_eq!(
        same.pending, None,
        "the stored secret sent again is no change"
    );
    assert!(saved.pending.is_some());
    assert_eq!(
        tried.location,
        format!("/org/{org_id}?oidc_pending=applied")
    );
    assert!(tried.token.is_some(), "the trial's session is set");
    assert_eq!(read.pending, None, "taken");
    assert!(read.sso_required, "still taking no password");
    assert!(
        read.can_require_sso,
        "the trial's session proves the provider now"
    );
    assert!(
        !old_session.can_require_sso,
        "the session made with the secret before proves it no more"
    );
    let actions = recorded(&applied, &org_id).await;
    assert_eq!(
        actions[..2],
        [
            "oidc_provider_pending_applied",
            "oidc_provider_pending_saved"
        ]
    );
    ts.teardown().await;
    fake.stop();
}

/// Pending settings are thrown away once, and no trial starts without them.
#[tokio::test]
async fn pending_settings_are_thrown_away() {
    let fake = FakeProvider::start().await;
    let (ts, org_id, admin) = enforced(&fake).await;
    let path = format!("/org/{org_id}/oidc");
    admin
        .put(&path, &with_secret(&fake, WRONG_SECRET))
        .await
        .check_ok();

    admin
        .delete(&format!("{path}/pending"))
        .await
        .check_no_content();
    admin
        .delete(&format!("{path}/pending"))
        .await
        .check_no_content();
    let read: OidcProvider = admin.get(&path).await.ok_json();
    let started = admin.post_empty(&format!("{path}/pending/sign-in")).await;

    assert_eq!(read.pending, None);
    started.check_status(StatusCode::CONFLICT);
    assert_eq!(started.error_reason(), "oidc_unavailable");
    let actions = recorded(&admin, &org_id).await;
    assert_eq!(
        actions[..2],
        [
            "oidc_provider_pending_discarded",
            "oidc_provider_pending_saved"
        ],
        "thrown away once"
    );
    ts.teardown().await;
    fake.stop();
}

/// Another issuer or client saved while the organization takes no password is set aside
/// as well, after the new issuer's discovery is read; the provider people sign in
/// through stays as it is, and the settings saved last are the pending ones. Switching
/// the provider off with them is refused.
#[tokio::test]
async fn another_issuer_or_client_waits_aside_too() {
    let fake = FakeProvider::start().await;
    let other = FakeProvider::start().await;
    let ts = Setup::native()
        .trusting_oidc_provider(&fake.certificate_der)
        .trusting_oidc_provider(&other.certificate_der)
        .configure(with_key_and_private_addresses)
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
    let path = format!("/org/{org_id}/oidc");

    let mut saved = Vec::new();
    for (issuer, client_id) in [(&other.issuer, CLIENT_ID), (&fake.issuer, "another-client")] {
        let provider: OidcProvider = admin
            .put(
                &path,
                &json!({ "issuer": issuer, "clientId": client_id, "clientSecret": "s" }),
            )
            .await
            .ok_json();
        saved.push(provider);
    }
    let switched_off = admin
        .put(
            &path,
            &json!({ "issuer": other.issuer, "clientId": CLIENT_ID, "clientSecret": "s", "enabled": false }),
        )
        .await;

    for (provider, (issuer, client_id)) in saved
        .iter()
        .zip([(&other.issuer, CLIENT_ID), (&fake.issuer, "another-client")])
    {
        let pending = provider.pending.as_ref().expect("set aside");
        assert_eq!(
            (&pending.issuer, pending.client_id.as_str()),
            (issuer, client_id)
        );
        assert_eq!(
            (provider.issuer.as_str(), provider.client_id.as_str()),
            (fake.issuer.as_str(), CLIENT_ID),
            "the provider as it was"
        );
        assert!(provider.sso_required);
    }
    switched_off.check_status(StatusCode::CONFLICT);
    assert_eq!(switched_off.error_reason(), "sso_enforced");
    assert_eq!(switched_off.refused_at(), "/enabled");
    admin
        .get(&format!("/w/{ROOT_WORKSPACE_ID}/boards"))
        .await
        .check_ok();
    ts.teardown().await;
    fake.stop();
    other.stop();
}

/// A trial that ends in another account than the administrator's who started it puts
/// nothing in use and makes no account: settings that let only a newcomer in would
/// leave the organization without an administrator who signs in.
#[tokio::test]
async fn a_trial_in_another_account_changes_nothing() {
    let fake = FakeProvider::start().await;
    let (ts, org_id, admin) = enforced(&fake).await;
    let path = format!("/org/{org_id}/oidc");
    admin
        .put(&path, &with_secret(&fake, NEXT_CLIENT_SECRET))
        .await
        .check_ok();
    let users_before = ts.app.store().count_rows("users").await.expect("counted");

    let tried = try_as(&ts, &fake, &admin, &org_id, "a-newcomer").await;
    let read: OidcProvider = admin.get(&path).await.ok_json();
    let users_after = ts.app.store().count_rows("users").await.expect("counted");

    assert_eq!(
        tried.location,
        format!("/org/{org_id}?oidc_pending_error=oidc-trial-account")
    );
    assert_eq!(tried.token, None);
    assert!(read.pending.is_some(), "still aside");
    assert!(read.can_require_sso, "the provider as it was");
    assert_eq!(users_after, users_before, "no account made");
    ts.teardown().await;
    fake.stop();
}

/// Another issuer is put in use by a trial of the administrator: their identity at it,
/// tied to nobody, is tied to them by the trial, as nothing else could tie it while
/// the organization signs in through the issuer before. The sessions made through
/// that one end; the trial's goes on, proves the provider, and is of the same account.
#[tokio::test]
async fn another_issuer_is_applied_by_its_administrators_trial() {
    let fake = FakeProvider::start().await;
    let other = FakeProvider::start().await;
    let ts = Setup::native()
        .trusting_oidc_provider(&fake.certificate_der)
        .trusting_oidc_provider(&other.certificate_der)
        .configure(with_key_and_private_addresses)
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
    let path = format!("/org/{org_id}/oidc");
    admin
        .put(
            &path,
            &json!({ "issuer": other.issuer, "clientId": CLIENT_ID, "clientSecret": CLIENT_SECRET }),
        )
        .await
        .check_ok();

    let tried = try_pending(&ts, &other, &admin, &org_id).await;
    let applied = ts.client_with_token(tried.token.as_deref().unwrap_or_default());
    let read: OidcProvider = applied.get(&path).await.ok_json();

    assert_eq!(
        tried.location,
        format!("/org/{org_id}?oidc_pending=applied")
    );
    assert_eq!(
        applied.me().await.id,
        ts.client.me().await.id,
        "the administrator's own account"
    );
    assert_eq!(read.issuer, other.issuer);
    assert!(read.sso_required && read.can_require_sso && read.pending.is_none());
    admin.get("/users/me").await.check_unauthorized();
    ts.teardown().await;
    fake.stop();
    other.stop();
}
