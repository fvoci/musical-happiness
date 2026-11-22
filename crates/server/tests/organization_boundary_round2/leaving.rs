//! Leaving an organization (`leaveOrganization`) asked about one the asker is not in: by
//! a member or an administrator of another organization, by a guest of its workspace,
//! about an own space, and without a session. Each is answered as about an organization
//! that is not there, and nobody's place changes. On SQLite, which holds one
//! organization that people work in together, every row is run but the other
//! organization's.

use fvoci_model::{OrgKind, UserWorkspace, Workspace, WorkspaceRole, new_id};
use fvoci_store::DbType;
use reqwest::StatusCode;
use serde_json::json;

use super::common::{ApiResponse, Client, ROOT_WORKSPACE_ID, Setup, TestServer, UNKNOWN_ID};
use super::{answer, db_type_of_the_tests};

/// The organization that people work in together and the own space of the account of
/// `client`, by identifier, as its list of workspaces has them.
async fn organizations_of(client: &Client) -> (Option<String>, String) {
    let listed: Vec<UserWorkspace> = client.get("/users/me/workspaces").await.ok_json();
    let shared = listed
        .iter()
        .find(|workspace| workspace.kind == OrgKind::Team)
        .map(|workspace| workspace.org_id.clone());
    let space = listed
        .iter()
        .find(|workspace| workspace.kind == OrgKind::Personal)
        .expect("an own space is listed");
    (shared, space.org_id.clone())
}

/// An account that is a guest of the root workspace and no member of its organization.
async fn a_guest(ts: &TestServer) -> Client {
    let guest = ts.add_user("leaving-guest").await;
    let guest_id = guest.me().await.id;
    let store = ts.app.store();
    for statement in [
        format!("DELETE FROM workspace_members WHERE user_id = '{guest_id}'"),
        format!("DELETE FROM organization_members WHERE user_id = '{guest_id}'"),
    ] {
        store
            .execute_unchecked(&statement)
            .await
            .expect("the account is no member of the organization");
    }
    store
        .add_workspace_member(ROOT_WORKSPACE_ID, &guest_id, WorkspaceRole::Guest)
        .await
        .expect("a guest of the root workspace");
    guest
}

/// An account that is `role` (`admin` or `member`) of another organization, and no
/// member of the first.
async fn of_another_organization(ts: &TestServer, username: &str, role: &str) -> Client {
    let client = ts.add_user(username).await;
    let user_id = client.me().await.id;
    let store = ts.app.store();
    for table in ["workspace_members", "organization_members"] {
        store
            .execute_unchecked(&format!("DELETE FROM {table} WHERE user_id = '{user_id}'"))
            .await
            .expect("the account leaves the first organization");
    }
    let workspace = Workspace {
        id: new_id(),
        ..Workspace::default()
    };
    let org_id = store
        .create_organization(&format!("Elsewhere of {username}"), &workspace)
        .await
        .expect("another organization");
    store
        .for_org(&org_id)
        .expect("an identifier")
        .execute_unchecked(&format!(
            "INSERT INTO organization_members (org_id, user_id, role, create_at) \
             VALUES ('{org_id}', '{user_id}', '{role}', 1)"
        ))
        .await
        .expect("in the other organization");
    client
}

/// Who is in which organization as what, past the routes.
async fn every_place(ts: &TestServer) -> Vec<String> {
    ts.app
        .store()
        .system()
        .texts_unchecked(
            "SELECT org_id || ' ' || user_id || ' ' || role FROM organization_members \
             ORDER BY 1",
        )
        .await
        .expect("read")
}

/// What `client` is answered asking to leave the organization `org_id`.
async fn leave(client: &Client, org_id: &str) -> ApiResponse {
    client
        .post(&format!("/org/{org_id}/leave"), &json!({}))
        .await
}

#[tokio::test]
async fn who_is_not_in_an_organization_does_not_leave_it() {
    let ts = Setup::native().start().await.init_basic().await;
    let (org, founders_space) = organizations_of(&ts.client).await;
    let org = org.expect("the first organization");
    let (_, members_space) = organizations_of(&ts.client2).await;
    let guest = a_guest(&ts).await;
    let mut askers = vec![
        ("a guest of its workspace", guest, org.clone()),
        (
            "its member, about another's own space",
            ts.client.clone(),
            members_space.clone(),
        ),
        (
            "its member, about their own space",
            ts.client2.clone(),
            members_space,
        ),
        (
            "its administrator, about their own space",
            ts.client.clone(),
            founders_space,
        ),
    ];
    if db_type_of_the_tests() == DbType::Postgres {
        for (who, username, role) in [
            (
                "a member of another organization",
                "member-elsewhere",
                "member",
            ),
            (
                "an administrator of another organization",
                "admin-elsewhere",
                "admin",
            ),
        ] {
            let client = of_another_organization(&ts, username, role).await;
            askers.push((who, client, org.clone()));
        }
    }
    let before = every_place(&ts).await;

    let mut violations = Vec::new();
    for (who, client, org_id) in &askers {
        let (asked, unknown) = (leave(client, org_id).await, leave(client, UNKNOWN_ID).await);
        if asked.status != StatusCode::NOT_FOUND || answer(&asked) != answer(&unknown) {
            violations.push(format!(
                "{who}: {:?}, and about nothing {:?}",
                answer(&asked),
                answer(&unknown)
            ));
        }
    }
    let nobody = ts.anonymous();
    let (asked, unknown) = (leave(&nobody, &org).await, leave(&nobody, UNKNOWN_ID).await);
    let after = every_place(&ts).await;
    ts.teardown().await;

    assert!(violations.is_empty(), "{}", violations.join("\n"));
    assert_eq!(asked.status, StatusCode::UNAUTHORIZED, "without a session");
    assert_eq!(answer(&asked), answer(&unknown), "without a session");
    assert_eq!(after, before, "nobody's place changes");
}
