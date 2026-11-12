//! An administrator of a workspace lists and manages its private boards (sees them,
//! changes their members, deletes and restores them) and reads what is on one only as
//! its member: making themselves one is recorded.

use fvoci_model::{Board, BoardMember, BoardType, WorkspaceRole};
use fvoci_store::AuditAction;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

use crate::boards::{member, new_board};
use crate::common::{ApiResponse, Client, Setup, TEST_WORKSPACE_ID, TestServer, UNKNOWN_ID};

/// The problem of an answer without what identifies the request.
fn answer(response: &ApiResponse) -> (StatusCode, Value) {
    let mut body = response.value();
    if let Some(object) = body.as_object_mut() {
        object.remove("instance");
        object.remove("requestId");
    }
    (response.status, body)
}

/// Makes the second user an administrator of the test workspace; returns their identifier.
async fn workspace_admin(th: &TestServer) -> String {
    let user_id = th.client2.me().await.id;
    th.app
        .store()
        .execute_unchecked(&format!(
            "UPDATE workspace_members SET role = '{}' WHERE user_id = '{user_id}' \
             AND workspace_id = '{TEST_WORKSPACE_ID}'",
            WorkspaceRole::Admin.as_str()
        ))
        .await
        .expect("the second user administers the test workspace");
    user_id
}

/// A private board of the first user with one card; returns the board and the card.
async fn private_board_with_a_card(th: &TestServer) -> (Board, String) {
    let board: Board = th
        .client
        .create_board(&new_board(TEST_WORKSPACE_ID, BoardType::Private, "Private"))
        .await
        .created_json();
    let card = th
        .client
        .post(
            &format!("/boards/{}/cards", board.id),
            &json!({"title": "Secret"}),
        )
        .await;
    card.check_created();
    let card_id = card.value()["id"].as_str().expect("a card").to_owned();
    (board, card_id)
}

/// The content routes of a board, each with the same route of a board that is not there.
fn content(board_id: &str, card_id: &str) -> Vec<(Method, String, String)> {
    let get = |hidden: String, absent: String| (Method::GET, hidden, absent);
    vec![
        get(
            format!("/boards/{board_id}/blocks"),
            format!("/boards/{UNKNOWN_ID}/blocks"),
        ),
        get(
            format!("/boards/{board_id}/cards"),
            format!("/boards/{UNKNOWN_ID}/cards"),
        ),
        get(format!("/cards/{card_id}"), format!("/cards/{UNKNOWN_ID}")),
        get(
            format!("/boards/{board_id}/sharing"),
            format!("/boards/{UNKNOWN_ID}/sharing"),
        ),
        get(
            format!("/boards/{board_id}/archive/export"),
            format!("/boards/{UNKNOWN_ID}/archive/export"),
        ),
        (
            Method::POST,
            format!("/boards/{board_id}/duplicate"),
            format!("/boards/{UNKNOWN_ID}/duplicate"),
        ),
        (
            Method::POST,
            format!("/boards/{board_id}/cards"),
            format!("/boards/{UNKNOWN_ID}/cards"),
        ),
    ]
}

async fn send(client: &Client, method: &Method, path: &str) -> ApiResponse {
    Client::send(client.request(method.clone(), path).json(&json!({}))).await
}

#[tokio::test]
async fn a_workspace_admin_lists_and_manages_a_private_board_and_reads_none_of_it() {
    let th = Setup::native().start().await.init_basic().await;
    workspace_admin(&th).await;
    let (board, card_id) = private_board_with_a_card(&th).await;
    let third = th.add_user("third").await.me().await.id;

    let listed: Vec<Board> = th
        .client2
        .get(&format!("/w/{TEST_WORKSPACE_ID}/boards"))
        .await
        .ok_json();
    let seen = th.client2.get(&format!("/boards/{}", board.id)).await;
    let members = th
        .client2
        .get(&format!("/boards/{}/members", board.id))
        .await;
    let mut hidden = Vec::new();
    for (method, path, absent) in content(&board.id, &card_id) {
        let response = send(&th.client2, &method, &path).await;
        let missing = send(&th.client2, &method, &absent).await;
        if response.status != StatusCode::NOT_FOUND || answer(&response) != answer(&missing) {
            hidden.push(format!("{method} {path}: {:?}", answer(&response)));
        }
    }
    let added = th
        .client2
        .add_member(&BoardMember {
            scheme_viewer: true,
            ..member(&board.id, &third)
        })
        .await;
    let deleted = th.client2.delete(&format!("/boards/{}", board.id)).await;
    let in_the_trash: Vec<Value> = th
        .client2
        .get(&format!("/w/{TEST_WORKSPACE_ID}/trash/boards"))
        .await
        .ok_json();
    let restored = th
        .client2
        .post(&format!("/boards/{}/undelete", board.id), &json!({}))
        .await;
    let recorded = th
        .app
        .store()
        .get_audit_log(TEST_WORKSPACE_ID)
        .await
        .expect("the log is read");
    th.teardown().await;

    assert!(
        listed.iter().any(|listed| listed.id == board.id),
        "{listed:?}"
    );
    seen.check_ok();
    members.check_ok();
    assert!(hidden.is_empty(), "content reached:\n{}", hidden.join("\n"));
    added.check_ok();
    deleted.check_no_content();
    assert!(
        in_the_trash
            .iter()
            .any(|item| item["id"] == json!(board.id)),
        "{in_the_trash:?}"
    );
    restored.check_ok();
    assert!(
        recorded.is_empty(),
        "nothing the admin did is recorded: {recorded:?}"
    );
}

#[tokio::test]
async fn a_workspace_admin_who_makes_themselves_a_member_reads_the_board_and_is_recorded_once() {
    let th = Setup::native().start().await.init_basic().await;
    let admin = workspace_admin(&th).await;
    let (board, card_id) = private_board_with_a_card(&th).await;
    let as_viewer = BoardMember {
        scheme_viewer: true,
        ..member(&board.id, &admin)
    };

    th.client2.add_member(&as_viewer).await.check_ok();
    let card = th.client2.get(&format!("/cards/{card_id}")).await;
    // Adding themselves again changes nothing, and records nothing.
    th.client2.add_member(&as_viewer).await.check_ok();
    let recorded = th
        .app
        .store()
        .get_audit_log(TEST_WORKSPACE_ID)
        .await
        .expect("the log is read");
    th.teardown().await;

    card.check_ok();
    assert_eq!(recorded.len(), 1, "{recorded:?}");
    let entry = &recorded[0];
    assert_eq!(
        (
            entry.action,
            entry.actor_id.as_str(),
            entry.target_id.as_str(),
            entry.workspace_id.as_str()
        ),
        (
            AuditAction::SelfAddedToBoard,
            admin.as_str(),
            board.id.as_str(),
            TEST_WORKSPACE_ID
        )
    );
    assert!(entry.create_at > 0, "{entry:?}");
}

#[tokio::test]
async fn a_member_who_is_no_admin_neither_lists_nor_manages_a_private_board() {
    let th = Setup::native().start().await.init_basic().await;
    let (board, _) = private_board_with_a_card(&th).await;
    let missing = th
        .client2
        .get(&format!("/boards/{UNKNOWN_ID}/members"))
        .await;

    let listed: Vec<Board> = th
        .client2
        .get(&format!("/w/{TEST_WORKSPACE_ID}/boards"))
        .await
        .ok_json();
    let members = th
        .client2
        .get(&format!("/boards/{}/members", board.id))
        .await;
    let deleted = th.client2.delete(&format!("/boards/{}", board.id)).await;
    th.teardown().await;

    assert!(
        listed.iter().all(|listed| listed.id != board.id),
        "{listed:?}"
    );
    assert_eq!(answer(&members), answer(&missing));
    deleted.check_not_found();
}
