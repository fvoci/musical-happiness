//! The workspaces of a user and the routes under `/w/{workspaceId}`: the first routes
//! behind the wall of the workspace. A user who is no member of a workspace is told
//! what somebody is told who names none (`visibility.rs` holds the other routes to the
//! same rule).

use fvoci_model::{Board, BoardMember, BoardType, OrgRole, Team, UserWorkspace, WorkspaceRole};
use fvoci_store::{DbType, Store};
use serde_json::Value;

use crate::common::{ApiResponse, ROOT_TEAM_ID, Setup, TEST_TEAM_ID, TestServer, UNKNOWN_ID};

/// The answer to a request for the boards of a workspace that is not there for the
/// user: the problem `not-found`, and the same words whatever the reason. What names
/// the one request (`instance`, `requestId`) is the only thing that differs.
#[track_caller]
fn not_there(response: &ApiResponse) -> Value {
    response.check_not_found();
    let mut body = response.check_problem();
    assert_eq!(body["type"], "/problems/not-found", "{body}");
    let members = body.as_object_mut().expect("a problem");
    members.remove("instance");
    members.remove("requestId");
    body
}

/// The identifiers of the boards an answer lists.
fn ids(boards: &[Board]) -> Vec<&str> {
    boards.iter().map(|board| board.id.as_str()).collect()
}

/// A user finds the workspaces they are a member of, with what they are in each, and
/// nobody finds them without a session.
#[tokio::test]
async fn a_user_lists_the_workspaces_they_are_a_member_of() {
    let ts = Setup::native().start().await.init_basic().await;

    let first: Vec<UserWorkspace> = ts.client.get("/users/me/workspaces").await.ok_json();
    let second: Vec<UserWorkspace> = ts.client2.get("/users/me/workspaces").await.ok_json();
    // Whoever registers gets into the default workspace, the root team; the first user
    // administers it.
    assert_eq!(first.len(), 1, "{first:?}");
    assert_eq!(first[0].id, ROOT_TEAM_ID);
    assert_eq!(first[0].role, WorkspaceRole::Admin);
    assert_eq!(second.len(), 1, "{second:?}");
    assert_eq!(second[0].id, ROOT_TEAM_ID);
    assert_eq!(second[0].role, WorkspaceRole::Member);
    assert_eq!(first[0].org_id, second[0].org_id, "one organization");
    assert_eq!(
        ts.client.get("/users/me/workspaces").await.value()[0],
        serde_json::json!({
            "id": ROOT_TEAM_ID,
            "orgId": first[0].org_id,
            "name": "",
            "role": "admin",
        }),
        "the members of a workspace in the list"
    );

    // A second workspace of the organization shows once the user is let into it.
    let user2 = ts.client2.me().await.id;
    ts.app
        .store()
        .add_workspace_member(TEST_TEAM_ID, &user2, WorkspaceRole::Guest)
        .await
        .expect("the user is let in");
    let second: Vec<UserWorkspace> = ts.client2.get("/users/me/workspaces").await.ok_json();
    let listed: Vec<(&str, WorkspaceRole)> = second
        .iter()
        .map(|workspace| (workspace.id.as_str(), workspace.role))
        .collect();
    assert_eq!(
        listed,
        [
            (ROOT_TEAM_ID, WorkspaceRole::Member),
            (TEST_TEAM_ID, WorkspaceRole::Guest)
        ],
        "oldest workspace first"
    );

    ts.anonymous()
        .get("/users/me/workspaces")
        .await
        .check_unauthorized();
    ts.teardown().await;
}

/// The boards of a workspace are for its members. For anybody else the workspace is
/// not there: a workspace of the same organization that the user is not in, an
/// identifier nobody has, and text that is no identifier all answer alike.
#[tokio::test]
async fn the_boards_of_a_workspace_are_for_its_members() {
    let ts = Setup::native().start().await.init_basic().await;
    let open = ts.create_board(ROOT_TEAM_ID, BoardType::Open).await;
    let private = ts.create_board(ROOT_TEAM_ID, BoardType::Private).await;
    let elsewhere = ts.create_board(TEST_TEAM_ID, BoardType::Open).await;

    // A member gets what the team's route gets: the boards they are on, and the open
    // ones.
    let boards: Vec<Board> = ts
        .client
        .get(&format!("/w/{ROOT_TEAM_ID}/boards"))
        .await
        .ok_json();
    assert_eq!(ids(&boards), [open.id.as_str(), private.id.as_str()]);
    let of_the_team: Vec<Board> = ts
        .client
        .get(&format!("/teams/{ROOT_TEAM_ID}/boards"))
        .await
        .ok_json();
    assert_eq!(boards, of_the_team, "as the team's route answers");
    let boards: Vec<Board> = ts
        .client2
        .get(&format!("/w/{ROOT_TEAM_ID}/boards"))
        .await
        .ok_json();
    assert_eq!(ids(&boards), [open.id.as_str()], "the open board only");

    // Nobody is a member of the second team: it is there for neither user, though the
    // first made a board in it through the team's route.
    let hidden = not_there(&ts.client.get(&format!("/w/{TEST_TEAM_ID}/boards")).await);
    assert_eq!(hidden["detail"], "workspace not found", "{hidden}");
    // The system's workspace is nobody's to work in, a member's neither.
    let user1 = ts.client.me().await.id;
    ts.system_store()
        .add_workspace_member(
            fvoci_store::SYSTEM_WORKSPACE_ID,
            &user1,
            WorkspaceRole::Member,
        )
        .await
        .expect("a membership of the system's workspace is stored");
    for nothing in [
        UNKNOWN_ID,
        "no-identifier",
        "%FF",
        fvoci_store::SYSTEM_WORKSPACE_ID,
    ] {
        let absent = not_there(&ts.client.get(&format!("/w/{nothing}/boards")).await);
        assert_eq!(absent, hidden, "{nothing} answers as a workspace of others");
    }
    let listed: Vec<UserWorkspace> = ts.client.get("/users/me/workspaces").await.ok_json();
    assert_eq!(listed.len(), 1, "the system's is not listed: {listed:?}");
    ts.anonymous()
        .get(&format!("/w/{ROOT_TEAM_ID}/boards"))
        .await
        .check_unauthorized();
    ts.anonymous()
        .get(&format!("/w/{UNKNOWN_ID}/boards"))
        .await
        .check_unauthorized();

    // Let in, the user finds the workspace and its boards; a guest finds only the
    // boards they are on.
    let user2 = ts.client2.me().await.id;
    let store = ts.app.store();
    store
        .add_workspace_member(TEST_TEAM_ID, &user1, WorkspaceRole::Member)
        .await
        .expect("the first user is let in");
    store
        .add_workspace_member(TEST_TEAM_ID, &user2, WorkspaceRole::Guest)
        .await
        .expect("the second user is let in as a guest");
    let boards: Vec<Board> = ts
        .client
        .get(&format!("/w/{TEST_TEAM_ID}/boards"))
        .await
        .ok_json();
    assert_eq!(ids(&boards), [elsewhere.id.as_str()]);
    let boards: Vec<Board> = ts
        .client2
        .get(&format!("/w/{TEST_TEAM_ID}/boards"))
        .await
        .ok_json();
    assert!(
        boards.is_empty(),
        "an open board is not a guest's: {boards:?}"
    );

    ts.teardown().await;
}

/// A second organization on the server of a test, with a workspace and an open board
/// in it that a user of its own made. Returns a handle that reaches its rows, and the
/// identifiers of the workspace and the board.
async fn another_organization(ts: &TestServer) -> (Store, String, String) {
    let workspace = Team {
        id: fvoci_model::new_id(),
        signup_token: "signup-token".to_owned(),
        title: "theirs".to_owned(),
        ..Team::default()
    };
    let org_id = ts
        .app
        .store()
        .create_organization("another", &workspace)
        .await
        .expect("a second organization is made");
    let theirs = ts.app.store().for_org(&org_id).expect("an identifier");
    let mut owner = fvoci_model::User {
        id: fvoci_model::new_id(),
        username: "owner.elsewhere".to_owned(),
        email: "owner.elsewhere@sample.com".to_owned(),
        ..fvoci_model::User::default()
    };
    theirs
        .create_user(&mut owner)
        .await
        .expect("their user is stored");
    let mut board = Board {
        id: fvoci_model::new_id(),
        team_id: workspace.id.clone(),
        board_type: BoardType::Open,
        title: "theirs".to_owned(),
        ..Board::default()
    };
    theirs
        .insert_board_with_admin(&mut board, &owner.id)
        .await
        .expect("their board is stored");
    (theirs, workspace.id, board.id)
}

/// A workspace of another organization is not there, and answers as one that nobody
/// has. For a user who is let into it, the route works in that organization: it lists
/// the boards there, which the routes that name no workspace, working in the server's
/// one organization, do not reach.
#[tokio::test]
async fn a_workspace_of_another_organization_is_reached_by_its_members_only() {
    // SQLite holds one organization.
    if fvoci_store::testing::db_type_of_the_tests() != DbType::Postgres {
        return;
    }
    let ts = Setup::native().start().await.init_basic().await;
    let (theirs, their_workspace, their_board) = another_organization(&ts).await;
    let mine = ts.create_board(ROOT_TEAM_ID, BoardType::Open).await;

    let hidden = not_there(&ts.client.get(&format!("/w/{their_workspace}/boards")).await);
    let absent = not_there(&ts.client.get(&format!("/w/{UNKNOWN_ID}/boards")).await);
    assert_eq!(hidden, absent, "theirs answers as one nobody has");
    let listed: Vec<UserWorkspace> = ts.client.get("/users/me/workspaces").await.ok_json();
    assert_eq!(listed.len(), 1, "theirs is not among mine: {listed:?}");

    // Let into their organization and workspace, the user reaches both organizations,
    // each through its own workspace.
    let user1 = ts.client.me().await.id;
    theirs
        .add_organization_member(&user1, OrgRole::Member)
        .await
        .expect("the user joins their organization");
    theirs
        .add_workspace_member(&their_workspace, &user1, WorkspaceRole::Member)
        .await
        .expect("the user is let into their workspace");
    let listed: Vec<UserWorkspace> = ts.client.get("/users/me/workspaces").await.ok_json();
    let found: Vec<(&str, &str)> = listed
        .iter()
        .map(|workspace| (workspace.id.as_str(), workspace.name.as_str()))
        .collect();
    assert_eq!(
        found,
        [(ROOT_TEAM_ID, ""), (their_workspace.as_str(), "theirs")],
        "the older organization first"
    );
    assert_ne!(listed[0].org_id, listed[1].org_id, "two organizations");

    let boards: Vec<Board> = ts
        .client
        .get(&format!("/w/{their_workspace}/boards"))
        .await
        .ok_json();
    assert_eq!(ids(&boards), [their_board.as_str()], "their open board");
    let boards: Vec<Board> = ts
        .client
        .get(&format!("/w/{ROOT_TEAM_ID}/boards"))
        .await
        .ok_json();
    assert_eq!(ids(&boards), [mine.id.as_str()], "and mine in mine");
    // The second user was let into nothing.
    not_there(
        &ts.client2
            .get(&format!("/w/{their_workspace}/boards"))
            .await,
    );
    // Let into the workspace as a guest, without belonging to their organization, the
    // user finds the workspace, and in it the boards they are on: none.
    let user2 = ts.client2.me().await.id;
    theirs
        .add_workspace_member(&their_workspace, &user2, WorkspaceRole::Guest)
        .await
        .expect("the second user is let in as a guest");
    let boards: Vec<Board> = ts
        .client2
        .get(&format!("/w/{their_workspace}/boards"))
        .await
        .ok_json();
    assert!(boards.is_empty(), "a guest is on no board: {boards:?}");
    let listed: Vec<UserWorkspace> = ts.client2.get("/users/me/workspaces").await.ok_json();
    assert_eq!(listed.len(), 2, "theirs is among the guest's: {listed:?}");

    // The order of the list is by the age of the organization, not by its identifier:
    // made older than the server's, theirs comes first.
    let aged = theirs
        .execute_unchecked("UPDATE organizations SET create_at = 1")
        .await
        .expect("their organization is made the older");
    assert_eq!(aged, 1, "their organization, and no other");
    let listed: Vec<UserWorkspace> = ts.client.get("/users/me/workspaces").await.ok_json();
    let found: Vec<&str> = listed
        .iter()
        .map(|workspace| workspace.id.as_str())
        .collect();
    assert_eq!(
        found,
        [their_workspace.as_str(), ROOT_TEAM_ID],
        "the older first"
    );

    // The routes that name no workspace work in the server's one organization: their
    // board is not there for them, also for a member of it.
    theirs
        .save_member(&BoardMember {
            board_id: their_board.clone(),
            user_id: user1.clone(),
            scheme_editor: true,
            ..BoardMember::default()
        })
        .await
        .expect("the user is put on their board");
    ts.client
        .get(&format!("/boards/{their_board}"))
        .await
        .check_not_found();
    ts.client
        .get(&format!("/teams/{their_workspace}/boards"))
        .await
        .check_not_found();

    ts.teardown().await;
}
