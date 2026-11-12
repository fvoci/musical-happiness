//! Who has left a workspace, and is still in the organization, finds none of its boards
//! through the routes that list or search boards; and a workspace the user is no member
//! of exports nothing but a 404.

use fvoci_model::{Board, BoardMember, BoardType};
use reqwest::StatusCode;
use serde_json::{Value, json};

use crate::boards::new_board;
use crate::common::{
    ApiResponse, HIDDEN_WORKSPACE_ID, Setup, TEST_WORKSPACE_ID, TestServer, UNKNOWN_ID,
};

/// The problem of an answer without what identifies the request.
fn answer(response: &ApiResponse) -> (StatusCode, Value) {
    let mut body = response.value();
    if let Some(object) = body.as_object_mut() {
        object.remove("instance");
        object.remove("requestId");
    }
    (response.status, body)
}

/// Ends the second user's membership of the test workspace, and of nothing else.
async fn remove_user2_from_the_test_workspace(th: &TestServer, user_id: &str) {
    th.app
        .store()
        .execute_unchecked(&format!(
            "DELETE FROM workspace_members WHERE user_id = '{user_id}' \
             AND workspace_id = '{TEST_WORKSPACE_ID}'"
        ))
        .await
        .expect("the membership of the test workspace ends");
}

#[tokio::test]
async fn who_left_a_workspace_finds_none_of_its_boards() {
    let th = Setup::native().start().await.init_basic().await;
    let user2 = th.client2.me().await.id;
    let board: Board = th
        .client2
        .create_board(&new_board(
            TEST_WORKSPACE_ID,
            BoardType::Private,
            "Quarterly secret",
        ))
        .await
        .created_json();
    remove_user2_from_the_test_workspace(&th, &user2).await;

    let found_everywhere: Vec<Board> = th.client2.get("/boards/search?q=Quarterly").await.ok_json();
    let found_from_root: Vec<Board> = th
        .client2
        .get(&format!(
            "/w/{}/boards/search?q=Quarterly",
            crate::common::ROOT_WORKSPACE_ID
        ))
        .await
        .ok_json();
    let memberships: Vec<BoardMember> = th.client2.get("/users/me/memberships").await.ok_json();
    let export = th
        .client2
        .get(&format!("/w/{TEST_WORKSPACE_ID}/archive/export"))
        .await;
    th.teardown().await;

    assert!(found_everywhere.is_empty(), "{found_everywhere:?}");
    assert!(found_from_root.is_empty(), "{found_from_root:?}");
    assert!(
        memberships.iter().all(|member| member.board_id != board.id),
        "{memberships:?}"
    );
    export.check_not_found();
}

#[tokio::test]
async fn a_workspace_the_user_is_no_member_of_exports_a_404() {
    let th = Setup::native().start().await.init_basic().await;
    let hidden = th
        .client
        .get(&format!("/w/{HIDDEN_WORKSPACE_ID}/archive/export"))
        .await;
    let missing = th
        .client
        .get(&format!("/w/{UNKNOWN_ID}/archive/export"))
        .await;
    let own = th
        .client
        .get(&format!("/w/{TEST_WORKSPACE_ID}/archive/export"))
        .await;
    th.teardown().await;

    hidden.check_not_found();
    assert_eq!(answer(&hidden), answer(&missing));
    own.check_ok();
}

#[tokio::test]
async fn an_admin_who_cannot_act_as_one_does_not_keep_a_board_from_its_last_admin() {
    let th = Setup::native().start().await.init_basic().await;
    let user1 = th.client.me().await.id;
    let user2 = th.client2.me().await.id;
    let board: Board = th
        .client
        .create_board(&new_board(TEST_WORKSPACE_ID, BoardType::Private, "Ours"))
        .await
        .created_json();
    // The second user is stored as an admin of the board, past the routes, and then
    // leaves the workspace: the role is left and opens nothing.
    th.app
        .store()
        .save_member(&BoardMember {
            board_id: board.id.clone(),
            user_id: user2.clone(),
            scheme_admin: true,
            ..BoardMember::default()
        })
        .await
        .expect("a second admin is stored");
    remove_user2_from_the_test_workspace(&th, &user2).await;

    let left = th
        .client
        .post(&format!("/boards/{}/leave", board.id), &json!({}))
        .await;
    let stored = th.app.store().get_member_for_board(&board.id, &user1).await;
    th.teardown().await;

    assert_eq!(left.status, StatusCode::CONFLICT, "{}", left.text());
    assert_eq!(left.error_reason(), "last_admin");
    assert!(stored.is_ok(), "the last admin who can act stays");
}
