//! Independent O4 regressions for paths outside the permission table.

#[path = "api/common.rs"]
mod common;

use fvoci_model::{Board, BoardMember, User};
use serde_json::json;

use crate::common::TEST_WS;
use common::{ROOT_WORKSPACE_ID, Setup, TEST_WORKSPACE_ID, TestServer};

/// The second user manages a private template without a role on it.
async fn private_template_for_admin() -> (TestServer, Board, String) {
    let server = Setup::native().start().await.init_basic().await;
    let admin = server.client2.me().await.id;
    server
        .app
        .store()
        .execute_unchecked(&format!(
            "UPDATE workspace_members SET role = 'admin' WHERE user_id = '{admin}' \
             AND workspace_id = '{TEST_WORKSPACE_ID}'"
        ))
        .await
        .expect("the second user manages the workspace");
    let template: Board = server
        .client
        .post(
            &format!("{TEST_WS}/boards"),
            &json!({
                "workspaceId": TEST_WORKSPACE_ID,
                "type": "P",
                "title": "Private template",
                "isTemplate": true,
                "description": "Confidential template description",
                "properties": {"confidential": "Confidential template property"}
            }),
        )
        .await
        .created_json();
    (server, template, admin)
}

#[tokio::test]
async fn a_roleless_workspace_admin_gets_no_template_content_from_the_list() {
    let (server, template, admin) = private_template_for_admin().await;
    // This is a supported API state: the board's owner can remove every role.
    server
        .client
        .add_member(&BoardMember {
            board_id: template.id.clone(),
            user_id: admin,
            ..BoardMember::default()
        })
        .await
        .check_ok();
    let direct: Board = server
        .client2
        .get(&format!("{TEST_WS}/boards/{}", template.id))
        .await
        .ok_json();
    assert!(direct.description.is_empty() && direct.properties.is_empty());
    let listed: Vec<Board> = server
        .client2
        .get(&format!("{TEST_WS}/templates"))
        .await
        .ok_json();
    let audit = server
        .app
        .store()
        .get_audit_log(TEST_WORKSPACE_ID)
        .await
        .expect("the audit log is read");
    server.teardown().await;

    assert!(audit.is_empty(), "the admin never opted into reading");
    let from_list = listed
        .iter()
        .find(|board| board.id == template.id)
        .expect("the administrator can list the template");
    assert_eq!(
        from_list, &direct,
        "listing must redact the same content as the individual board response"
    );
}

#[tokio::test]
async fn a_workspace_admin_can_list_a_private_template_without_a_membership() {
    let (server, template, _) = private_template_for_admin().await;
    let direct: Board = server
        .client2
        .get(&format!("{TEST_WS}/boards/{}", template.id))
        .await
        .ok_json();
    let listed: Vec<Board> = server
        .client2
        .get(&format!("{TEST_WS}/templates"))
        .await
        .ok_json();
    server.teardown().await;

    assert!(direct.description.is_empty() && direct.properties.is_empty());
    assert_eq!(
        listed.iter().find(|board| board.id == template.id),
        Some(&direct),
        "the management list must include private templates, with their content hidden"
    );
}

#[tokio::test]
async fn a_former_non_author_is_not_discoverable_through_a_stale_board_role() {
    let server = Setup::native().start().await.init_basic().await;
    let leaver = server.client2.me().await.id;
    let board = server
        .create_board(TEST_WORKSPACE_ID, fvoci_model::BoardType::Private)
        .await;
    server
        .client
        .add_member(&BoardMember {
            board_id: board.id,
            user_id: leaver.clone(),
            scheme_viewer: true,
            ..BoardMember::default()
        })
        .await
        .check_ok();
    for workspace in [ROOT_WORKSPACE_ID, TEST_WORKSPACE_ID] {
        server
            .app
            .store()
            .execute_unchecked(&format!(
                "DELETE FROM workspace_members WHERE user_id = '{leaver}' \
                 AND workspace_id = '{workspace}'"
            ))
            .await
            .expect("the viewer leaves every shared workspace");
    }
    // The viewer authored no content. Keeping author labels cannot require exposing
    // this subsequent profile change or making the account searchable.
    server
        .client2
        .patch("/users/me", &json!({"displayName": "After leaving"}))
        .await
        .check_ok();
    let profile = server
        .client
        .get(&format!("{TEST_WS}/users/{leaver}"))
        .await;
    let found: Vec<User> = server
        .client
        .get(&format!("{TEST_WS}/users?search=user2"))
        .await
        .ok_json();
    server.teardown().await;

    assert!(
        profile.status == reqwest::StatusCode::NOT_FOUND && found.is_empty(),
        "a stale board role is not a shared workspace: profile={}, search={found:?}",
        profile.text()
    );
}
