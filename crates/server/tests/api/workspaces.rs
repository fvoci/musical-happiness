//! The workspaces of a user and the routes under `/w/{workspaceId}`: the first routes
//! behind the wall of the workspace. A user who is no member of a workspace is told
//! what somebody is told who names none (`visibility.rs` holds the other routes to the
//! same rule).

use fvoci_model::{
    Board, BoardMember, BoardType, OrgRole, UserWorkspace, Workspace, WorkspaceRole,
};
use fvoci_store::{DbType, Store};
use serde_json::{Value, json};

use crate::common::{
    ApiResponse, Client, HIDDEN_WORKSPACE_ID, ROOT_WORKSPACE_ID, Setup, TEST_WORKSPACE_ID,
    TestServer, UNKNOWN_ID,
};

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
    // Whoever registers gets into the default workspace, the root workspace, which the
    // first user administers; the test server lets its users into its second workspace too.
    let of = |workspaces: &[UserWorkspace]| -> Vec<(String, WorkspaceRole)> {
        workspaces
            .iter()
            .map(|workspace| (workspace.id.clone(), workspace.role))
            .collect()
    };
    let in_both = |root_role| {
        vec![
            (ROOT_WORKSPACE_ID.to_owned(), root_role),
            (TEST_WORKSPACE_ID.to_owned(), WorkspaceRole::Member),
        ]
    };
    assert_eq!(of(&first), in_both(WorkspaceRole::Admin), "the first user");
    assert_eq!(
        of(&second),
        in_both(WorkspaceRole::Member),
        "the second user"
    );
    assert_eq!(first[0].org_id, second[0].org_id, "one organization");
    assert_eq!(
        ts.client.get("/users/me/workspaces").await.value()[0],
        serde_json::json!({
            "id": ROOT_WORKSPACE_ID,
            "orgId": first[0].org_id,
            "name": "",
            "role": "admin",
        }),
        "the members of a workspace in the list"
    );

    // A further workspace of the organization shows once the user is let into it.
    let user2 = ts.client2.me().await.id;
    ts.app
        .store()
        .add_workspace_member(HIDDEN_WORKSPACE_ID, &user2, WorkspaceRole::Guest)
        .await
        .expect("the user is let in");
    let second: Vec<UserWorkspace> = ts.client2.get("/users/me/workspaces").await.ok_json();
    let mut expected = in_both(WorkspaceRole::Member);
    expected.push((HIDDEN_WORKSPACE_ID.to_owned(), WorkspaceRole::Guest));
    assert_eq!(of(&second), expected, "oldest workspace first");

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
    let open = ts.create_board(ROOT_WORKSPACE_ID, BoardType::Open).await;
    let private = ts.create_board(ROOT_WORKSPACE_ID, BoardType::Private).await;
    // A board of the hidden workspace, which no user of the test could make there.
    let stranger = ts.stranger().await;
    let mut elsewhere = Board {
        id: fvoci_model::new_id(),
        workspace_id: HIDDEN_WORKSPACE_ID.to_owned(),
        board_type: BoardType::Open,
        ..Board::default()
    };
    ts.app
        .store()
        .insert_board(&mut elsewhere, &stranger)
        .await
        .expect("a board of the hidden workspace is stored");

    // A member gets what the workspace's route gets: the boards they are on, and the open
    // ones.
    let boards: Vec<Board> = ts
        .client
        .get(&format!("/w/{ROOT_WORKSPACE_ID}/boards"))
        .await
        .ok_json();
    assert_eq!(ids(&boards), [open.id.as_str(), private.id.as_str()]);
    let of_the_workspace: Vec<Board> = ts
        .client
        .get(&format!("/w/{ROOT_WORKSPACE_ID}/boards"))
        .await
        .ok_json();
    assert_eq!(boards, of_the_workspace, "as the workspace's route answers");
    let boards: Vec<Board> = ts
        .client2
        .get(&format!("/w/{ROOT_WORKSPACE_ID}/boards"))
        .await
        .ok_json();
    assert_eq!(ids(&boards), [open.id.as_str()], "the open board only");

    // Nobody is a member of the hidden workspace: it is there for neither user.
    let hidden = not_there(
        &ts.client
            .get(&format!("/w/{HIDDEN_WORKSPACE_ID}/boards"))
            .await,
    );
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
    assert_eq!(listed.len(), 2, "the system's is not listed: {listed:?}");
    ts.anonymous()
        .get(&format!("/w/{ROOT_WORKSPACE_ID}/boards"))
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
        .add_workspace_member(HIDDEN_WORKSPACE_ID, &user1, WorkspaceRole::Member)
        .await
        .expect("the first user is let in");
    store
        .add_workspace_member(HIDDEN_WORKSPACE_ID, &user2, WorkspaceRole::Guest)
        .await
        .expect("the second user is let in as a guest");
    let boards: Vec<Board> = ts
        .client
        .get(&format!("/w/{HIDDEN_WORKSPACE_ID}/boards"))
        .await
        .ok_json();
    assert_eq!(ids(&boards), [elsewhere.id.as_str()]);
    let boards: Vec<Board> = ts
        .client2
        .get(&format!("/w/{HIDDEN_WORKSPACE_ID}/boards"))
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
    let workspace = Workspace {
        id: fvoci_model::new_id(),
        signup_token: "signup-token".to_owned(),
        title: "theirs".to_owned(),
        ..Workspace::default()
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
        workspace_id: workspace.id.clone(),
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

/// The workspaces a user lists as their own, by identifier.
async fn workspaces_of(client: &Client) -> Vec<String> {
    let listed: Vec<UserWorkspace> = client.get("/users/me/workspaces").await.ok_json();
    listed.into_iter().map(|workspace| workspace.id).collect()
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
    let mine = ts.create_board(ROOT_WORKSPACE_ID, BoardType::Open).await;

    let hidden = not_there(&ts.client.get(&format!("/w/{their_workspace}/boards")).await);
    let absent = not_there(&ts.client.get(&format!("/w/{UNKNOWN_ID}/boards")).await);
    assert_eq!(hidden, absent, "theirs answers as one nobody has");
    let mine_before = workspaces_of(&ts.client).await;
    assert_eq!(
        mine_before,
        [ROOT_WORKSPACE_ID, TEST_WORKSPACE_ID],
        "theirs is not"
    );

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
        [
            (ROOT_WORKSPACE_ID, ""),
            (TEST_WORKSPACE_ID, ""),
            (their_workspace.as_str(), "theirs")
        ],
        "the older organization first"
    );
    assert_ne!(listed[0].org_id, listed[2].org_id, "two organizations");

    let boards: Vec<Board> = ts
        .client
        .get(&format!("/w/{their_workspace}/boards"))
        .await
        .ok_json();
    assert_eq!(ids(&boards), [their_board.as_str()], "their open board");
    let boards: Vec<Board> = ts
        .client
        .get(&format!("/w/{ROOT_WORKSPACE_ID}/boards"))
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
    let of_the_guest = workspaces_of(&ts.client2).await;
    assert!(of_the_guest.contains(&their_workspace), "{of_the_guest:?}");

    // The order of the list is by the age of the organization, not by its identifier:
    // made older than the server's, theirs comes first.
    let aged = theirs
        .execute_unchecked("UPDATE organizations SET create_at = 1")
        .await
        .expect("their organization is made the older");
    assert_eq!(aged, 1, "their organization, and no other");
    assert_eq!(
        workspaces_of(&ts.client).await,
        [
            their_workspace.as_str(),
            ROOT_WORKSPACE_ID,
            TEST_WORKSPACE_ID
        ],
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
    // A route that names the workspace in its body, not its path, is one of them.
    let board = json!({"workspaceId": their_workspace, "type": "P", "title": "new"});
    let refused = ts.client.post("/boards", &board).await;
    refused.check_error(reqwest::StatusCode::BAD_REQUEST, "no such workspace");

    ts.teardown().await;
}

/// The routes that moved under `/w/{workspaceId}` read the workspace in the server's one
/// organization, as they did before: a workspace of another organization is not there
/// for them, also for a member of it who works in it, while `getWorkspaceBoards` lists
/// its boards.
#[tokio::test]
async fn the_moved_routes_find_no_workspace_of_another_organization() {
    // SQLite holds one organization.
    if fvoci_store::testing::db_type_of_the_tests() != DbType::Postgres {
        return;
    }
    let ts = Setup::native().start().await.init_basic().await;
    let (theirs, their_workspace, _) = another_organization(&ts).await;
    let user1 = ts.client.me().await.id;
    theirs
        .add_organization_member(&user1, OrgRole::Member)
        .await
        .expect("the user joins their organization");
    theirs
        .add_workspace_member(&their_workspace, &user1, WorkspaceRole::Member)
        .await
        .expect("the user is let into their workspace");
    let hidden = not_there(&ts.client.get(&format!("/w/{UNKNOWN_ID}/categories")).await);
    for path in ["", "/categories", "/templates", "/users", "/trash/boards"] {
        let refused = ts.client.get(&format!("/w/{their_workspace}{path}")).await;
        assert_eq!(not_there(&refused), hidden, "{path}");
    }
    let search = format!("/w/{their_workspace}/boards/search?q=x");
    assert_eq!(not_there(&ts.client.get(&search).await), hidden);
    ts.client
        .get(&format!("/w/{their_workspace}/boards"))
        .await
        .check_ok();
    ts.teardown().await;
}
