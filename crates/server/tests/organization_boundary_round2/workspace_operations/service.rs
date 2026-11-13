//! Positive cases outside workspace paths, including the deliberately global admin role.

use serde_json::json;

use super::super::fake_oidc::FakeProvider;
use super::{Case, Client, PASSWORD, TestServer, identity, shared};

pub(super) async fn exercise(case: &mut Case, server: &TestServer, fake: &FakeProvider) {
    for operation in ["hello", "ping", "ready", "getClientConfig"] {
        case.run(operation, &[], json!({})).await;
    }
    account(case).await;
    organization(case).await;
    shared::exercise(case).await;
    invitation(case, server).await;
    identity::exercise(case, server, fake).await;
    let request = case
        .request("setUserAdmin", &[("userId", &case.user)])
        .json(&json!({"isAdmin": true}));
    let response = Client::send(request).await;
    assert_eq!(
        response.status.as_u16(),
        if case.instance_admin { 200 } else { 403 },
        "organization ownership does not grant instance authority"
    );
    if case.instance_admin {
        assert_eq!(
            response.value()["id"],
            case.user,
            "admin response names the requested account"
        );
        assert_eq!(
            response.value()["isAdmin"],
            true,
            "instance administrator remains an administrator"
        );
    }
    assert_eq!(
        case.client.me().await.is_admin,
        case.instance_admin,
        "instance authority stays as assigned"
    );
    case.record("setUserAdmin", &response);
    let changed = case
        .run(
            "changePassword",
            &[],
            json!({"oldPassword": PASSWORD, "newPassword": "changed parity password"}),
        )
        .await;
    case.client.token = changed.session_cookie().expect("rotated session cookie");
    case.run("logout", &[], json!({})).await;
}

async fn account(case: &mut Case) {
    case.run(
        "patchMe",
        &[],
        json!({"displayName": format!("{} display", case.label)}),
    )
    .await;
    let user = case.user.clone();
    case.present("getMe", &[], &user).await;
    let workspace = case.workspace.clone();
    case.present("getMyWorkspaces", &[], &workspace).await;
    let memberships = case.run("getMyMemberships", &[], json!({})).await.value();
    assert!(
        !memberships.as_array().expect("memberships").is_empty(),
        "founder has board memberships"
    );
    case.run(
        "patchClientState",
        &[],
        json!({"updatedFields": {"parity": case.label}}),
    )
    .await;
    let label = case.label.clone();
    case.present("getClientState", &[], &label).await;
    case.run("patchUserSettings", &[], json!({"language": "ko"}))
        .await;
    let settings = case.run("getUserSettings", &[], json!({})).await.value();
    assert_eq!(settings["language"], "ko", "account setting round trip");
    let options = super::with_query(case.request("getLoginOptions", &[]), "org", &case.org);
    let options = case.send("getLoginOptions", options).await.value();
    assert_eq!(
        options["organization"]["id"], case.org,
        "login options name the requested organization"
    );
}

async fn organization(case: &mut Case) {
    let org = case.org.clone();
    case.present("getOrganization", &[], &org).await;
    case.run(
        "patchOrganization",
        &[],
        json!({"name": format!("{} renamed", case.label)}),
    )
    .await;
    let peer = case.peer.clone();
    case.present("getOrgMembers", &[], &peer).await;
    for role in ["admin", "member"] {
        case.run("putOrgRole", &[("userId", &peer)], json!({"role": role}))
            .await;
    }
    let made = case
        .run("createOrgWorkspace", &[], json!({"name": "Additional"}))
        .await
        .value();
    let workspace = case.remember(&made, "id");
    let at = &[("workspaceId", workspace.as_str())];
    let joined = case.run("joinOrgWorkspace", at, json!({})).await.value();
    assert_eq!(
        joined["joinedAsOrgAdmin"], true,
        "organization administrator joins the new workspace"
    );
    let left = case.run("leaveOrgWorkspace", at, json!({})).await.value();
    assert_eq!(
        left["joinedAsOrgAdmin"], false,
        "leaving ends administrative participation"
    );
    case.run(
        "renameOrgWorkspace",
        at,
        json!({"name": "Additional renamed"}),
    )
    .await;
    case.run("deleteOrgWorkspace", at, json!({})).await;
    let restored = case.run("restoreOrgWorkspace", at, json!({})).await.value();
    assert_eq!(restored["deletedAt"], 0, "workspace is restored");
    case.present("getOrgWorkspaces", &[], &workspace).await;
    case.present("getOrgAudit", &[], &workspace).await;
}

async fn invitation(case: &mut Case, server: &TestServer) {
    let invitation = case
        .run("createInvitation", &[], json!({"role": "member"}))
        .await
        .value();
    let token = invitation["token"].as_str().expect("invitation token");
    let preview = case
        .run("previewInvitation", &[], json!({"token": token}))
        .await
        .value();
    assert!(
        preview.to_string().contains(&case.label),
        "preview identifies this invitation"
    );
    let accepted = server
        .anonymous()
        .post(
            "/invitations/accept",
            &json!({
                "token": token, "account": {"username": format!("{}-invited", case.label),
                "email": format!("{}-invited@example.test", case.label), "password": PASSWORD}
            }),
        )
        .await;
    assert!(accepted.status.is_success(), "{}", accepted.text());
    assert_eq!(
        accepted.value()["workspaceId"],
        case.workspace,
        "invitation joins its own workspace"
    );
    case.record("acceptInvitation", &accepted);
}
