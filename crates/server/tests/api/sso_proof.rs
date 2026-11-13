//! What a session made through the organization's provider proves: that the provider,
//! as it was stored when the session was made, lets people in. A change since that the
//! session did not go through is proved by nobody, and taking no password is not
//! switched on with it.

use fvoci_model::OidcProvider;
use reqwest::StatusCode;
use serde_json::json;

use crate::common::ROOT_WORKSPACE_ID;
use crate::fake_oidc::{CLIENT_ID, CLIENT_SECRET, FakeProvider, Person};
use crate::oidc::{Browser, org_of, server_for, set_provider};
use crate::sso::{ADMIN_SUBJECT, Passwords, admin_through_provider, require_sso, through_provider};

/// A secret the fake provider does not take at its token endpoint.
const WRONG_SECRET: &str = "a secret the provider never issued";

/// The client secret is what the server shows the provider at the end of every
/// sign-in. A session made with the secret before proves nothing of another one: kept
/// as the proof, it lets an administrator switch taking no password on with a secret
/// nobody has signed in with, and once it is wrong nobody gets in again (passwords are
/// refused, the provider refuses the server) but through the operator's release.
#[tokio::test]
async fn another_secret_is_not_proved_by_a_session_made_with_the_one_before() {
    let fake = FakeProvider::start().await;
    let ts = server_for(&fake).await;
    let org_id = org_of(&ts).await;
    let path = format!("/org/{org_id}/oidc");
    set_provider(&ts, &org_id, &fake).await;
    let admin = admin_through_provider(&ts, &fake, &org_id).await;

    let changed: OidcProvider = admin
        .put(
            &path,
            &json!({
                "issuer": fake.issuer, "clientId": CLIENT_ID, "clientSecret": WRONG_SECRET,
            }),
        )
        .await
        .ok_json();
    let enforcement = require_sso(&admin, &org_id, &fake, Passwords::Refused).await;
    fake.sign_in_as(Person::new("newcomer", "newcomer@example.com"));
    let sign_in_now = Browser::new(&ts, &fake).sign_in(&org_id).await;

    assert_eq!(
        enforcement.status,
        StatusCode::CONFLICT,
        "switched on with a secret no session proved; a sign-in through the provider \
         now comes to {sign_in_now:?}: {}",
        enforcement.text()
    );
    assert_eq!(enforcement.error_reason(), "sso_unproven");
    assert!(
        !changed.can_require_sso,
        "the session was made with another secret than the one stored now"
    );
    ts.teardown().await;
    fake.stop();
}

/// A change of client ends no session: the one made through the provider before goes
/// on. Once the organization takes only the provider it was replaced by, that session
/// opens nothing of it, the provider's settings among it.
#[tokio::test]
async fn the_session_of_a_replaced_client_opens_nothing_once_the_new_one_is_required() {
    let fake = FakeProvider::start().await;
    let ts = server_for(&fake).await;
    let org_id = org_of(&ts).await;
    let path = format!("/org/{org_id}/oidc");
    let boards = format!("/w/{ROOT_WORKSPACE_ID}/boards");
    set_provider(&ts, &org_id, &fake).await;
    let old = admin_through_provider(&ts, &fake, &org_id).await;
    // Away to another client and back: the same client, another provider.
    for client_id in ["another-client", CLIENT_ID] {
        ts.client
            .put(
                &path,
                &json!({
                    "issuer": fake.issuer, "clientId": client_id, "clientSecret": CLIENT_SECRET,
                }),
            )
            .await
            .check_ok();
    }
    old.get(&boards).await.check_ok();
    let new = through_provider(&ts, &fake, &org_id, ADMIN_SUBJECT).await;
    require_sso(&new, &org_id, &fake, Passwords::Refused)
        .await
        .check_ok();

    for opened in [&boards, &path] {
        let response = old.get(opened).await;
        assert_eq!(
            response.status,
            StatusCode::UNAUTHORIZED,
            "{opened}: {}",
            response.text()
        );
        assert_eq!(
            response.check_problem()["type"],
            "/problems/sso-required",
            "{opened}"
        );
    }
    new.get(&boards).await.check_ok();
    ts.teardown().await;
    fake.stop();
}
