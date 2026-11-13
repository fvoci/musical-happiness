//! What the SSO settings screen may offer for this session.
use crate::fake_oidc::{CLIENT_ID, CLIENT_SECRET, FakeProvider};
use crate::oidc::{org_of, server_for, set_provider};
use crate::sso::admin_through_provider;
use fvoci_model::OidcProvider;
use serde_json::json;

#[tokio::test]
async fn the_settings_name_whether_this_session_proved_this_provider() {
    let fake = FakeProvider::start().await;
    let ts = server_for(&fake).await;
    let org_id = org_of(&ts).await;
    let path = format!("/org/{org_id}/oidc");
    let provider = set_provider(&ts, &org_id, &fake).await;
    assert!(!provider.can_require_sso, "a password proved no provider");
    ts.client2.get(&path).await.check_not_found();
    let admin = admin_through_provider(&ts, &fake, &org_id).await;
    let read: OidcProvider = admin.get(&path).await.ok_json();
    assert!(read.can_require_sso);
    let changed: OidcProvider = admin
        .put(
            &path,
            &json!({
                "issuer": fake.issuer, "clientId": "another-client", "clientSecret": CLIENT_SECRET,
            }),
        )
        .await
        .ok_json();
    assert!(!changed.can_require_sso, "another client was not proved");
    let read: OidcProvider = admin.get(&path).await.ok_json();
    assert!(!read.can_require_sso);
    let restored: OidcProvider = admin
        .put(
            &path,
            &json!({
                "issuer": fake.issuer, "clientId": CLIENT_ID, "clientSecret": CLIENT_SECRET,
            }),
        )
        .await
        .ok_json();
    assert!(
        !restored.can_require_sso,
        "restoring the client creates a new provider"
    );
    ts.stop().await.expect("the server stops");
    fake.stop();
}
