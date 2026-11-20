//! A user's own space through the routes: every account has one, however it was made,
//! works in it as in any workspace, and reaches it alone.

use fvoci_model::{Card, Container, ContainerType, OrgKind, UserWorkspace, WorkspaceRole};
use reqwest::StatusCode;
use serde_json::json;

use crate::boards::new_board;
use crate::common::{Client, ROOT_WS, Setup, TEST_WS};

/// The workspace of the client's own space: the last the user is listed, and the one
/// of its kind.
pub(crate) async fn own_workspace(client: &Client) -> UserWorkspace {
    workspaces_and_own(client).await.1
}

/// The workspaces the client's user is listed, of the organizations people work in,
/// and the one of the user's own space, which is listed after them.
pub(crate) async fn workspaces_and_own(client: &Client) -> (Vec<UserWorkspace>, UserWorkspace) {
    let mut workspaces: Vec<UserWorkspace> = client.get("/users/me/workspaces").await.ok_json();
    let own = workspaces.pop().expect("the list is never empty");
    assert_eq!(
        (own.kind, own.role, own.name.as_str(), own.sso_required),
        (OrgKind::Personal, WorkspaceRole::Admin, "", false),
        "the user's own space, last: {own:?}"
    );
    assert!(
        workspaces
            .iter()
            .all(|workspace| workspace.kind == OrgKind::Team && workspace.org_id != own.org_id),
        "one space of the user's own: {workspaces:?}"
    );
    (workspaces, own)
}

fn ids(containers: &[Container]) -> Vec<&str> {
    containers.iter().map(|board| board.id.as_str()).collect()
}

/// The account that starts a server's organization and the account a link lets into
/// it each have a space of their own. The first makes a board and a card there and
/// reads them back, by the list, by the identifier and by a search; the second, who
/// works beside the first in the organization, finds nothing of it by any of them,
/// from the first's workspace or from one of their own, and makes nothing there.
#[tokio::test]
async fn an_account_makes_a_board_in_its_own_space_and_nobody_else_finds_it() {
    let th = Setup::native().start().await.init_basic().await;
    let own = own_workspace(&th.client).await;
    let others = own_workspace(&th.client2).await;
    assert_ne!(own.org_id, others.org_id, "an organization for each");
    assert_ne!(own.id, others.id, "a workspace for each");

    let here = format!("/w/{}", own.id);
    th.client.get(&here).await.check_ok();
    let board: Container = th
        .client
        .create_board(&new_board(&own.id, ContainerType::Private, "mine alone"))
        .await
        .created_json();
    assert_eq!(board.workspace_id, own.id);
    let cards = format!("{here}/boards/{}/cards", board.id);
    let card = Card {
        title: "a card of mine".to_owned(),
        ..Card::default()
    };
    let card: Card = th.client.post(&cards, &card).await.created_json();

    let listed: Vec<Container> = th.client.get(&format!("{here}/boards")).await.ok_json();
    assert_eq!(ids(&listed), [board.id.as_str()], "listed");
    let by_id = format!("{here}/containers/{}", board.id);
    let read: Container = th.client.get(&by_id).await.ok_json();
    assert_eq!(read.title, "mine alone", "read by its identifier");
    let read: Vec<Card> = th.client.get(&cards).await.ok_json();
    assert_eq!(read, [card], "its card");
    let search = format!("{here}/boards/search?q=alone");
    let found: Vec<Container> = th.client.get(&search).await.ok_json();
    assert_eq!(ids(&found), [board.id.as_str()], "found");

    // The other account: in the first's workspace nothing is there.
    for path in [&here, &format!("{here}/boards"), &by_id, &cards, &search] {
        th.client2.get(path).await.check_not_found();
    }
    let theirs = new_board(&own.id, ContainerType::Private, "theirs");
    th.client2.create_board(&theirs).await.check_not_found();
    th.client2.post(&cards, &json!({})).await.check_not_found();
    // And from the workspaces they work in, the board is not one of them.
    for theirs in [ROOT_WS, TEST_WS, &format!("/w/{}", others.id)] {
        let by_id = format!("{theirs}/containers/{}", board.id);
        th.client2.get(&by_id).await.check_not_found();
        let found: Vec<Container> = th
            .client2
            .get(&format!("{theirs}/boards/search?q=alone"))
            .await
            .ok_json();
        assert_eq!(ids(&found), [""; 0], "{theirs}");
    }
    // Nor is it the first's to reach from the organization's workspace.
    let elsewhere = format!("{ROOT_WS}/containers/{}", board.id);
    th.client.get(&elsewhere).await.check_not_found();
    th.teardown().await;
}

/// Nobody administers a user's own space, and it has no people: to its owner the
/// routes of an organization answer 404 for it, and under its workspace the routes of
/// invitations, of guests and of the groups of people do. The same routes answer the same user in the
/// organization they started.
#[tokio::test]
async fn an_own_space_has_no_organization_to_administer_and_no_people() {
    let th = Setup::native().start().await.init_basic().await;
    let own = own_workspace(&th.client).await;
    let started: Vec<UserWorkspace> = th.client.get("/users/me/workspaces").await.ok_json();
    let organization = format!("/org/{}", started[0].org_id);
    let space = format!("/org/{}", own.org_id);
    for route in ["", "/members", "/workspaces", "/audit"] {
        th.client
            .get(&format!("{organization}{route}"))
            .await
            .check_ok();
        th.client
            .get(&format!("{space}{route}"))
            .await
            .check_not_found();
    }
    // The organization has no provider and says so; the space says what it says of
    // everything else.
    let provider = th.client.get(&format!("{organization}/oidc")).await;
    provider.check_error(StatusCode::NOT_FOUND, "oidc provider not found");
    let provider = th.client.get(&format!("{space}/oidc")).await;
    provider.check_error(StatusCode::NOT_FOUND, "organization not found");
    let renamed = json!({"name": "renamed"});
    th.client.patch(&space, &renamed).await.check_not_found();
    let workspaces = format!("{space}/workspaces");
    let made = th.client.post(&workspaces, &renamed).await;
    made.check_not_found();

    let here = format!("/w/{}", own.id);
    for route in ["/invitations", "/teams"] {
        th.client.get(&format!("{ROOT_WS}{route}")).await.check_ok();
        th.client
            .get(&format!("{here}{route}"))
            .await
            .check_not_found();
    }
    let nothing = json!({});
    for route in ["/invitations", "/invitations/mail", "/teams"] {
        let posted = th.client.post(&format!("{here}{route}"), &nothing).await;
        posted.check_not_found();
    }
    let guests = format!("{here}/guest-invitations");
    th.client.put(&guests, &nothing).await.check_not_found();
    // The workspace itself is there, and its people are its owner.
    th.client.get(&here).await.check_ok();
    th.teardown().await;
}
