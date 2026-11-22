//! Leaving an organization at the account's own asking (`leaveOrganization`): what the
//! member who leaves and the others are told, what stands in the way, and how somebody
//! who left comes back through the organization's provider.

use std::time::Duration;

use futures_util::{SinkExt as _, StreamExt as _};
use fvoci_model::{Container, OrgKind, UserWorkspace};
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message;

use crate::common::{ApiResponse, Client, ROOT_WORKSPACE_ID, Setup, TestServer, WebSocket};
use crate::fake_oidc::{FakeProvider, Person};
use crate::oidc::{Browser, Outcome, org_of, server_for, set_provider};
use crate::sso::{through_provider, tie};

/// How long a connection is waited on for what it is to be told.
const WAIT: Duration = Duration::from_secs(5);

/// The subject the member has at the fake provider.
const MEMBER_SUBJECT: &str = "subject-of-user2";

/// What `client` is answered asking to leave the organization `org_id`.
async fn leave(client: &Client, org_id: &str) -> ApiResponse {
    client
        .post(&format!("/org/{org_id}/leave"), &json!({}))
        .await
}

/// A connection of `client`'s that follows the root workspace.
async fn following_the_root(ts: &TestServer, client: &Client) -> WebSocket {
    let mut socket = client.connect_websocket().await;
    let subscribe = json!({"action": "SUBSCRIBE_WORKSPACE", "workspaceId": ROOT_WORKSPACE_ID});
    socket
        .send(Message::Text(subscribe.to_string().into()))
        .await
        .expect("the subscription is sent");
    crate::common::carried_out(ts.app.ws(), &mut socket).await;
    socket
}

/// The first message with the action that the connection is told.
async fn told(socket: &mut WebSocket, action: &str) -> Value {
    tokio::time::timeout(WAIT, async {
        while let Some(frame) = socket.next().await {
            if let Message::Text(text) = frame.expect("a websocket frame") {
                let message: Value = serde_json::from_str(&text).expect("JSON");
                if message["action"] == action {
                    return message;
                }
            }
        }
        panic!("the connection ended before {action}");
    })
    .await
    .unwrap_or_else(|_| panic!("{action} is told"))
}

/// The organizations that people work in together that `client`'s account is listed
/// workspaces of, by identifier.
async fn shared_organizations_of(client: &Client) -> Vec<String> {
    let listed: Vec<UserWorkspace> = client.get("/users/me/workspaces").await.ok_json();
    listed
        .into_iter()
        .filter(|workspace| workspace.kind == OrgKind::Team)
        .map(|workspace| workspace.org_id)
        .collect()
}

/// Who is in which workspace and organization as what, past the routes.
async fn every_place(ts: &TestServer) -> Vec<String> {
    ts.app
        .store()
        .system()
        .texts_unchecked(
            "SELECT 'org ' || org_id || ' ' || user_id || ' ' || role \
             FROM organization_members UNION ALL \
             SELECT 'workspace ' || workspace_id || ' ' || user_id || ' ' || role \
             FROM workspace_members ORDER BY 1",
        )
        .await
        .expect("read")
}

/// The member leaves: the connection of theirs that follows the organization's
/// workspace is told it follows it no more and stays open, a board they alone
/// administered is announced to who follows the workspace with its new admin, and the
/// workspace, the organization and the leaving are not there for them from then on. Their
/// session stays.
#[tokio::test]
async fn a_member_who_leaves_is_told_and_finds_the_organization_no_more() {
    let ts = Setup::native().start().await.init_basic().await;
    let org_id = org_of(&ts).await;
    let (admin_id, member_id) = (ts.client.me().await.id, ts.client2.me().await.id);
    let board: Container = ts
        .client2
        .post(
            &format!("/w/{ROOT_WORKSPACE_ID}/boards"),
            &json!({"workspaceId": ROOT_WORKSPACE_ID, "type": "P", "title": "The member's"}),
        )
        .await
        .created_json();
    let mut other_tab = following_the_root(&ts, &ts.client2).await;
    let mut admins = following_the_root(&ts, &ts.client).await;

    leave(&ts.client2, &org_id).await.check_no_content();

    let ended = told(&mut other_tab, "UNSUBSCRIBED_WORKSPACE").await;
    assert_eq!(ended["workspaceId"], ROOT_WORKSPACE_ID, "{ended}");
    let passed = told(&mut admins, "UPDATE_MEMBER").await;
    assert_eq!(
        (
            &passed["member"]["containerId"],
            &passed["member"]["userId"]
        ),
        (&json!(board.id), &json!(admin_id)),
        "{passed}"
    );
    crate::common::carried_out(ts.app.ws(), &mut other_tab).await;
    ts.client2
        .get(&format!("/w/{ROOT_WORKSPACE_ID}"))
        .await
        .check_not_found();
    assert_eq!(
        shared_organizations_of(&ts.client2).await,
        Vec::<String>::new()
    );
    assert_eq!(ts.client2.me().await.id, member_id, "the session stays");
    leave(&ts.client2, &org_id).await.check_not_found();
    ts.teardown().await;
}

/// What stands in the way of leaving is refused with its reason, and nothing changes:
/// the server's only administrator stays in its first organization, the only
/// administrator of one with somebody else in it stays, and so does its only person.
#[tokio::test]
async fn what_stands_in_the_way_of_leaving_is_refused_and_changes_nothing() {
    let ts = Setup::native().start().await.init_basic().await;
    let org_id = org_of(&ts).await;
    let member_id = ts.client2.me().await.id;
    let mut refusals = Vec::new();
    let mut refused = async |label: &str| {
        let before = every_place(&ts).await;
        let answer = leave(&ts.client, &org_id).await;
        assert_eq!(every_place(&ts).await, before, "{label}: nothing changes");
        assert_eq!(answer.status.as_u16(), 409, "{label}: {}", answer.text());
        let problem = answer.value();
        refusals.push((
            label.to_owned(),
            problem["reason"].clone(),
            problem["type"].clone(),
        ));
    };

    refused("the server's only administrator").await;
    // Somebody else runs the server.
    ts.app
        .store()
        .execute_unchecked(&format!(
            "UPDATE users SET is_admin = true WHERE id = '{member_id}'"
        ))
        .await
        .expect("another administrator of the server");
    refused("the organization's only administrator").await;
    leave(&ts.client2, &org_id).await.check_no_content();
    // Who left the server's own organization runs the server no more.
    let made = ts
        .client2
        .put(
            &format!("/users/{member_id}/admin"),
            &json!({"isAdmin": true}),
        )
        .await;
    assert_eq!(
        (made.status.as_u16(), made.error_reason()),
        (403, "admin_required".to_owned()),
        "{}",
        made.text()
    );
    // Somebody else runs it from outside the organization.
    ts.app
        .store()
        .execute_unchecked(&format!(
            "UPDATE users SET is_admin = true WHERE id = '{member_id}'"
        ))
        .await
        .expect("an administrator of the server outside the organization");
    refused("the organization's only person").await;

    assert_eq!(
        refusals,
        [
            (
                "the server's only administrator".to_owned(),
                json!("last_instance_admin"),
                json!("/problems/last-instance-admin")
            ),
            (
                "the organization's only administrator".to_owned(),
                json!("sole_org_admin"),
                json!("/problems/sole-org-admin")
            ),
            (
                "the organization's only person".to_owned(),
                json!("alone_in_organization"),
                json!("/problems/alone-in-organization")
            ),
        ]
    );
    ts.teardown().await;
}

/// Who leaves is put out of the rooms of the organization's cards as it leaves, not at
/// the next periodic asking; who stays goes on writing.
#[tokio::test]
async fn who_leaves_is_put_out_of_the_rooms_at_once() {
    let ts = crate::rooms_changes::seldom()
        .start()
        .await
        .init_basic()
        .await;
    let org_id = org_of(&ts).await;
    let body = crate::rooms::paragraphs(&["first"]);
    let (_table, mut one, mut two) = crate::rooms::two_in_a_room(&ts, &body).await;
    let changed_at = tokio::time::Instant::now();
    leave(&ts.client2, &org_id).await.check_no_content();
    let left = crate::rooms_changes::soon(&mut two, "DOC_LEFT", changed_at).await;
    assert_eq!(
        left,
        json!({"action": "DOC_LEFT", "doc": two.doc, "reason": "not_found"})
    );
    one.write_acked("after the member left").await;
    ts.teardown().await;
}

/// Somebody who left an organization whose provider lets newcomers in (`autoJoin`)
/// comes back as a member by signing in through it, with no invitation; where it lets
/// nobody new in, they are refused as a newcomer is. This pins what is so now.
#[tokio::test]
async fn who_left_comes_back_through_the_provider_as_a_newcomer_would() {
    let fake = FakeProvider::start().await;
    let ts = server_for(&fake).await;
    let org_id = org_of(&ts).await;
    set_provider(&ts, &org_id, &fake).await;
    let member_id = ts.client2.me().await.id;
    tie(&ts, &ts.client2, &fake, MEMBER_SUBJECT).await;
    leave(&ts.client2, &org_id).await.check_no_content();
    let closed =
        json!({"issuer": fake.issuer, "clientId": crate::fake_oidc::CLIENT_ID, "autoJoin": false});
    ts.client
        .put(&format!("/org/{org_id}/oidc"), &closed)
        .await
        .check_ok();
    fake.sign_in_as(Person::new(
        MEMBER_SUBJECT,
        &format!("{MEMBER_SUBJECT}@example.com"),
    ));
    let refused = Browser::new(&ts, &fake).sign_in(&org_id).await;

    set_provider(&ts, &org_id, &fake).await;
    let back = through_provider(&ts, &fake, &org_id, MEMBER_SUBJECT).await;

    assert_eq!(refused, Outcome::Refused("oidc-join-closed".to_owned()));
    assert_eq!(back.me().await.id, member_id, "the same account");
    assert_eq!(
        shared_organizations_of(&back).await,
        [org_id],
        "a member again"
    );
    ts.teardown().await;
    fake.stop();
}
