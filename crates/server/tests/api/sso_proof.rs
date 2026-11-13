//! What a session made through the organization's provider proves: that the provider,
//! as it was stored when the session was made, lets people in. A change since that the
//! session did not go through is proved by nobody, and taking no password is not
//! switched on with it.

use fvoci_model::OidcProvider;
use reqwest::StatusCode;
use serde_json::json;

use crate::fake_oidc::{CLIENT_ID, FakeProvider, Person};
use crate::oidc::{Browser, org_of, server_for, set_provider};
use crate::sso::{Passwords, admin_through_provider, require_sso};

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
