//! Whom a user sees of the other users: who shares a workspace they work in, who is on a
//! board with them in a workspace they are in, and themselves. A guest sees who is on
//! its boards. Anybody else is as one who does not exist.

use fvoci_model::{BoardMember, BoardType, User, WorkspaceRole};
use reqwest::StatusCode;
use serde_json::{Value, json};

use crate::boards::{member, new_board};
use crate::common::{
    ApiResponse, Client, HIDDEN_WORKSPACE_ID, ROOT_WORKSPACE_ID, Setup, TEST_WORKSPACE_ID,
    TestServer, UNKNOWN_ID,
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

fn names(users: &[User]) -> Vec<&str> {
    let mut names: Vec<&str> = users.iter().map(|user| user.username.as_str()).collect();
    names.sort_unstable();
    names
}

/// A user who is in the hidden workspace only.
async fn somebody_elsewhere(th: &TestServer) -> String {
    let user_id = th.add_user("elsewhere").await.me().await.id;
    for workspace_id in [ROOT_WORKSPACE_ID, TEST_WORKSPACE_ID] {
        th.app
            .store()
            .execute_unchecked(&format!(
                "DELETE FROM workspace_members WHERE user_id = '{user_id}' \
                 AND workspace_id = '{workspace_id}'"
            ))
            .await
            .expect("the user leaves the workspace");
    }
    th.app
        .store()
        .add_workspace_member(HIDDEN_WORKSPACE_ID, &user_id, WorkspaceRole::Member)
        .await
        .expect("the user works in the hidden workspace");
    user_id
}

async fn search(client: &Client, workspace_id: &str, term: &str) -> Vec<User> {
    client
        .get(&format!("/w/{workspace_id}/users?search={term}"))
        .await
        .ok_json()
}

#[tokio::test]
async fn a_user_sees_who_shares_a_workspace_and_nobody_else() {
    let th = Setup::native().start().await.init_basic().await;
    let user2 = th.client2.me().await.id;
    let elsewhere = somebody_elsewhere(&th).await;

    let listed: Vec<User> = th
        .client
        .get(&format!("/w/{TEST_WORKSPACE_ID}/users"))
        .await
        .ok_json();
    let found = search(&th.client, TEST_WORKSPACE_ID, "elsewhere").await;
    let profile = th.client.get(&format!("/users/{elsewhere}")).await;
    let unknown = th.client.get(&format!("/users/{UNKNOWN_ID}")).await;
    let by_ids = th.client.post("/users", &[&user2, &elsewhere]).await;
    let in_workspace = th
        .client
        .post(&format!("/w/{TEST_WORKSPACE_ID}/users"), &[&elsewhere])
        .await;
    let coworker = th.client.get(&format!("/users/{user2}")).await;
    th.teardown().await;

    assert_eq!(names(&listed), ["user1", "user2"], "the workspace's people");
    assert!(found.is_empty(), "{found:?}");
    assert_eq!(answer(&profile), answer(&unknown), "a stranger is nobody");
    by_ids.check_not_found();
    in_workspace.check_not_found();
    coworker.check_ok();
}

#[tokio::test]
async fn a_guest_sees_who_is_on_its_boards() {
    let th = Setup::native().start().await.init_basic().await;
    let guest = th.client2.me().await.id;
    th.app
        .store()
        .execute_unchecked(&format!(
            "UPDATE workspace_members SET role = 'guest' WHERE user_id = '{guest}' \
             AND workspace_id = '{TEST_WORKSPACE_ID}'"
        ))
        .await
        .expect("the second user becomes a guest of the test workspace");
    // A third user works in the test workspace and shares no board with the guest.
    let third = th.add_user("third").await.me().await.id;
    let board: fvoci_model::Board = th
        .client
        .create_board(&new_board(TEST_WORKSPACE_ID, BoardType::Private, "Shared"))
        .await
        .created_json();
    th.client
        .add_member(&BoardMember {
            scheme_viewer: true,
            ..member(&board.id, &guest)
        })
        .await
        .check_ok();

    let listed: Vec<User> = th
        .client2
        .get(&format!("/w/{TEST_WORKSPACE_ID}/users"))
        .await
        .ok_json();
    let found = search(&th.client2, TEST_WORKSPACE_ID, "third").await;
    let third_by_id = th
        .client2
        .post(&format!("/w/{TEST_WORKSPACE_ID}/users"), &[&third])
        .await;
    // In the root workspace, where the second user works, the third is a coworker.
    let third_from_root = search(&th.client2, ROOT_WORKSPACE_ID, "third").await;
    th.teardown().await;

    assert_eq!(names(&listed), ["user1", "user2"], "the board's people");
    assert!(found.is_empty(), "{found:?}");
    third_by_id.check_not_found();
    assert_eq!(names(&third_from_root), ["third"]);
}

/// Who left a workspace keeps the author of what they wrote: they are named to who
/// reads it, and found by nobody; a role they still have on a board shows nobody
/// (user decision of O4).
#[tokio::test]
async fn who_left_a_workspace_is_named_where_they_wrote_and_found_nowhere() {
    let th = Setup::native().start().await.init_basic().await;
    let leaver = th.client2.me().await.id;
    let board: fvoci_model::Board = th
        .client
        .create_board(&new_board(TEST_WORKSPACE_ID, BoardType::Private, "Ours"))
        .await
        .created_json();
    th.client
        .add_member(&BoardMember {
            scheme_editor: true,
            ..member(&board.id, &leaver)
        })
        .await
        .check_ok();
    th.client2
        .post(
            &format!("/boards/{}/cards", board.id),
            &json!({"title": "Mine"}),
        )
        .await
        .check_created();
    for workspace_id in [ROOT_WORKSPACE_ID, TEST_WORKSPACE_ID] {
        th.app
            .store()
            .execute_unchecked(&format!(
                "DELETE FROM workspace_members WHERE user_id = '{leaver}' \
                 AND workspace_id = '{workspace_id}'"
            ))
            .await
            .expect("the second user leaves every workspace");
    }

    // The first user reads the board, where the leaver wrote a card.
    let by_id: Vec<User> = th
        .client
        .post(&format!("/w/{TEST_WORKSPACE_ID}/users"), &[&leaver])
        .await
        .ok_json();
    let profile = th.client.get(&format!("/users/{leaver}")).await;
    let found: Vec<User> = th
        .client
        .get(&format!("/w/{TEST_WORKSPACE_ID}/users?search=user2"))
        .await
        .ok_json();
    let members: Vec<BoardMember> = th
        .client
        .get(&format!("/boards/{}/members", board.id))
        .await
        .ok_json();
    // The leaver sees nobody but themselves.
    let user1 = th.client.me().await.id;
    let seen_by_leaver = th.client2.get(&format!("/users/{user1}")).await;
    let themselves = th.client2.get(&format!("/users/{leaver}")).await;
    th.teardown().await;

    assert_eq!(names(&by_id), ["user2"], "the card's author is named");
    profile.check_ok();
    assert!(found.is_empty(), "nobody finds who left: {found:?}");
    assert!(
        members.iter().all(|member| member.user_id != leaver),
        "the board's members leave out who left: {members:?}"
    );
    seen_by_leaver.check_not_found();
    themselves.check_ok();
}

#[tokio::test]
async fn a_guest_sees_by_identifier_who_is_on_a_board_with_a_role_and_nobody_else() {
    let th = Setup::native().start().await.init_basic().await;
    let guest = th.client2.me().await.id;
    th.app
        .store()
        .execute_unchecked(&format!(
            "UPDATE workspace_members SET role = 'guest' WHERE user_id = '{guest}'"
        ))
        .await
        .expect("the second user becomes a guest of every workspace");
    let user1 = th.client.me().await.id;
    let third = th.add_user("third").await.me().await.id;
    let shared: fvoci_model::Board = th
        .client
        .create_board(&new_board(TEST_WORKSPACE_ID, BoardType::Private, "Shared"))
        .await
        .created_json();
    th.client
        .add_member(&BoardMember {
            scheme_viewer: true,
            ..member(&shared.id, &guest)
        })
        .await
        .check_ok();
    // On this board the guest has a membership without a role, which shares nothing.
    let roleless: fvoci_model::Board = th
        .client
        .create_board(&new_board(TEST_WORKSPACE_ID, BoardType::Private, "No role"))
        .await
        .created_json();
    for user in [&guest, &third] {
        let membership = if *user == third {
            BoardMember {
                scheme_viewer: true,
                ..member(&roleless.id, user)
            }
        } else {
            member(&roleless.id, user)
        };
        th.client.add_member(&membership).await.check_ok();
    }

    let coworker = th.client2.get(&format!("/users/{user1}")).await;
    let not_shared = th.client2.post("/users", &[&third]).await;
    let missing = th.client2.post("/users", &[UNKNOWN_ID]).await;
    let found = search(&th.client2, TEST_WORKSPACE_ID, "third").await;
    th.teardown().await;

    coworker.check_ok();
    not_shared.check_not_found();
    assert_eq!(
        answer(&not_shared),
        answer(&missing),
        "unseen is as missing"
    );
    assert!(found.is_empty(), "{found:?}");
}

#[tokio::test]
async fn a_guest_editing_an_open_board_adds_nobody() {
    let th = Setup::native().start().await.init_basic().await;
    let guest = th.client2.me().await.id;
    th.app
        .store()
        .execute_unchecked(&format!(
            "UPDATE workspace_members SET role = 'guest' WHERE user_id = '{guest}' \
             AND workspace_id = '{TEST_WORKSPACE_ID}'"
        ))
        .await
        .expect("the second user becomes a guest of the test workspace");
    let third = th.add_user("third").await.me().await.id;
    let board: fvoci_model::Board = th
        .client
        .create_board(&new_board(TEST_WORKSPACE_ID, BoardType::Open, "Open"))
        .await
        .created_json();
    th.client
        .add_member(&BoardMember {
            scheme_editor: true,
            ..member(&board.id, &guest)
        })
        .await
        .check_ok();

    let added = th.client2.add_member(&member(&board.id, &third)).await;
    let nobody = th.client2.add_member(&member(&board.id, UNKNOWN_ID)).await;
    th.teardown().await;

    added.check_forbidden();
    assert_eq!(
        answer(&added),
        answer(&nobody),
        "a guest learns nothing of who is there"
    );
}
