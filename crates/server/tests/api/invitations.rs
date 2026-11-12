//! Signing up with the name of an organization, and the links that let others into it.

use fvoci_model::{Board, BoardType, INVITATION_LIFETIME_MILLIS, Invitation, UserWorkspace};
use fvoci_store::DbType;
use fvoci_store::testing::db_type_of_the_tests;
use reqwest::StatusCode;
use serde_json::{Value, json};

use crate::common::{
    ApiResponse, Client, ORG_NAME, PASSWORD, ROOT_WORKSPACE_ID, Setup, TestServer,
};

fn open(config: &mut fvoci_server::config::Configuration) {
    "open".clone_into(&mut config.org_creation);
}

/// A registration that starts the organization `organization_name`.
fn registration(username: &str, organization_name: &str) -> Value {
    json!({
        "username": username,
        "email": format!("{username}@sample.com"),
        "password": PASSWORD,
        "organizationName": organization_name,
    })
}

/// The account `username` asks for when they accept a link.
fn account(username: &str) -> Value {
    json!({
        "username": username,
        "email": format!("{username}@sample.com"),
        "password": PASSWORD,
    })
}

/// What an answer says, without what names the one request.
fn said(response: &ApiResponse) -> (StatusCode, Value) {
    let mut body: Value = serde_json::from_slice(&response.body).unwrap_or(Value::Null);
    if let Some(object) = body.as_object_mut() {
        object.remove("instance");
        object.remove("requestId");
    }
    (response.status, body)
}

/// Makes a link for members into the root workspace, over the route.
async fn member_link(client: &Client) -> Invitation {
    client
        .post(
            &format!("/w/{ROOT_WORKSPACE_ID}/invitations"),
            &json!({"role": "member"}),
        )
        .await
        .created_json()
}

/// One column of a link, as text; nothing if there is no such link.
async fn of_the_link(th: &TestServer, link_id: &str, column: &str) -> Vec<String> {
    th.app
        .store()
        .texts_unchecked(&format!(
            "SELECT CAST({column} AS TEXT) FROM invitations WHERE CAST(id AS TEXT) = '{link_id}'"
        ))
        .await
        .expect("the link is read")
}

/// How many links the server has stored.
async fn links(th: &TestServer) -> i64 {
    th.app
        .store()
        .system()
        .count_rows("invitations")
        .await
        .expect("the links are counted")
}

/// A server that holds one organization takes registrations until it has its first user, and
/// says so to the page that offers to register.
#[tokio::test]
async fn a_server_takes_registrations_until_it_has_its_organization() {
    let th = Setup::native().start().await;
    let options = th.anonymous().get("/login-options").await.value();
    assert_eq!(options["registrationOpen"], json!(true), "{options}");

    th.anonymous()
        .post("/register", &registration("first", "  The Firm "))
        .await
        .check_created();

    let options = th.anonymous().get("/login-options").await.value();
    assert_eq!(options["registrationOpen"], json!(false), "{options}");
    assert_eq!(options["organization"]["name"], json!("The Firm"));
    let refused = th
        .anonymous()
        .post("/register", &registration("second", "Another Firm"))
        .await;
    refused.check_status(StatusCode::FORBIDDEN);
    assert_eq!(refused.error_reason(), "invitation_required");
    assert_eq!(
        refused.problem_type(),
        "/problems/invitation-required",
        "a kind of its own"
    );
    // Nobody registers after the first user, whatever they send: no word about the
    // request, and no password hashed.
    for body in [json!({}), json!({"username": "first", "password": "x"})] {
        let refused = th.anonymous().post("/register", &body).await;
        refused.check_status(StatusCode::FORBIDDEN);
        assert_eq!(refused.error_reason(), "invitation_required", "{body}");
        assert_eq!(refused.refused_places(), Vec::<String>::new(), "{body}");
    }
    assert_eq!(
        th.app.registered_user_count().await.expect("user count"),
        1,
        "nobody else got in"
    );
    // Nothing of the refused ones is there: no second organization, and the first
    // keeps its name, which is its first workspace's too, without the white space.
    let first = th.login("first", PASSWORD).await;
    let workspaces: Vec<UserWorkspace> = first.get("/users/me/workspaces").await.ok_json();
    assert_eq!(workspaces.len(), 1, "{workspaces:?}");
    assert_eq!(workspaces[0].name, "The Firm");
    assert_eq!(workspaces[0].role, fvoci_model::WorkspaceRole::Admin);

    th.teardown().await;
}

/// Two who signed up on a server where everybody starts an organization
/// (`org_creation` is `open`): the server, and each of them with the workspace they
/// started.
struct TwoOrganizations {
    th: TestServer,
    alice: Client,
    alpha: UserWorkspace,
    bob: Client,
    beta: UserWorkspace,
}

async fn two_organizations() -> TwoOrganizations {
    let th = Setup::native()
        .on_an_empty_database()
        .configure(open)
        .start()
        .await;
    for (username, name) in [("alice", "Alpha"), ("bob", "Beta")] {
        let options = th.anonymous().get("/login-options").await.value();
        assert_eq!(options["registrationOpen"], json!(true), "{options}");
        let made = th
            .anonymous()
            .post("/register", &registration(username, name))
            .await;
        made.check_created();
        assert_eq!(
            made.value()["isAdmin"],
            json!(username == "alice"),
            "only the first user administers the server"
        );
    }
    let alice = th.login("alice", PASSWORD).await;
    let bob = th.login("bob", PASSWORD).await;
    let of = async |client: &Client| -> UserWorkspace {
        let workspaces: Vec<UserWorkspace> = client.get("/users/me/workspaces").await.ok_json();
        assert_eq!(workspaces.len(), 1, "the one they started: {workspaces:?}");
        workspaces
            .into_iter()
            .next()
            .expect("a workspace of their own")
    };
    let (alpha, beta) = (of(&alice).await, of(&bob).await);
    TwoOrganizations {
        th,
        alice,
        alpha,
        bob,
        beta,
    }
}

/// Makes a board of the workspace that is open to its members.
async fn open_board(client: &Client, workspace_id: &str) -> Board {
    let board: Board = client
        .post(
            "/boards",
            &json!({"workspaceId": workspace_id, "type": "O", "title": "Open"}),
        )
        .await
        .created_json();
    assert_eq!(board.board_type, BoardType::Open, "{board:?}");
    board
}

/// On a server where everybody starts an organization, two who sign up have one each,
/// and neither reaches anything of the other's.
#[tokio::test]
async fn where_everybody_starts_an_organization_each_has_one_of_their_own() {
    if db_type_of_the_tests() != DbType::Postgres {
        return;
    }
    let TwoOrganizations {
        th,
        alice,
        alpha,
        bob,
        beta,
    } = two_organizations().await;
    assert_eq!((alpha.name.as_str(), beta.name.as_str()), ("Alpha", "Beta"));
    assert_ne!(alpha.org_id, beta.org_id, "two organizations");
    assert_eq!(beta.role, fvoci_model::WorkspaceRole::Admin);

    // Each works in their own, and the other's is not there for them.
    let board = open_board(&alice, &alpha.id).await;
    for path in [
        format!("/w/{}", alpha.id),
        format!("/w/{}/boards", alpha.id),
        format!("/w/{}/users", alpha.id),
        format!("/boards/{}", board.id),
    ] {
        bob.get(&path).await.check_status(StatusCode::NOT_FOUND);
    }
    let boards: Vec<Board> = bob.get(&format!("/w/{}/boards", beta.id)).await.ok_json();
    assert_eq!(boards, Vec::new(), "nothing of Alpha in Beta");

    // Behind the wall, each organization has its own rows and none of the other's.
    for (org_id, boards) in [(&alpha.org_id, 1), (&beta.org_id, 0)] {
        let store = th.app.store().for_org(org_id).expect("an identifier");
        for (table, rows) in [
            ("organizations", 1),
            ("workspaces", 1),
            ("organization_members", 1),
            ("workspace_members", 1),
            ("boards", boards),
        ] {
            assert_eq!(
                store.count_rows(table).await.expect("the rows are counted"),
                rows,
                "{table} of {org_id}"
            );
        }
    }

    // An account is the server's, not an organization's: a name is taken across them.
    let taken = th
        .anonymous()
        .post(
            "/register",
            &json!({
                "username": "ALICE", "email": "other@sample.com", "password": PASSWORD,
                "organizationName": "Gamma",
            }),
        )
        .await;
    taken.check_status(StatusCode::CONFLICT);
    assert_eq!(taken.error_reason(), "username_taken");
    assert_eq!(
        th.app
            .store()
            .system()
            .count_rows("organizations")
            .await
            .expect("the rows are counted"),
        // The two organizations, and the system's own.
        3,
        "a refused registration leaves no organization behind"
    );

    th.teardown().await;
}

/// Somebody of another organization who accepts a link for members becomes a member of
/// the link's workspace and of its organization, and stays what they are in their own.
#[tokio::test]
async fn who_accepts_a_link_of_another_organization_is_a_member_of_both() {
    if db_type_of_the_tests() != DbType::Postgres {
        return;
    }
    let TwoOrganizations {
        th,
        alice,
        alpha,
        bob,
        beta,
    } = two_organizations().await;
    let board = open_board(&alice, &alpha.id).await;
    bob.get(&format!("/boards/{}", board.id))
        .await
        .check_status(StatusCode::NOT_FOUND);

    let link = alice
        .post(
            &format!("/w/{}/invitations", alpha.id),
            &json!({"role": "member"}),
        )
        .await
        .created_json::<Invitation>()
        .token
        .expect("the token of a new link");
    let accepted = bob
        .post("/invitations/accept", &json!({"token": link}))
        .await;
    accepted.check_ok();
    assert_eq!(accepted.value(), json!({"workspaceId": alpha.id}));
    let mut workspaces: Vec<UserWorkspace> = bob.get("/users/me/workspaces").await.ok_json();
    workspaces.sort_by(|a, b| a.name.cmp(&b.name));
    let seen: Vec<(&str, &str, fvoci_model::WorkspaceRole)> = workspaces
        .iter()
        .map(|workspace| {
            (
                workspace.name.as_str(),
                workspace.org_id.as_str(),
                workspace.role,
            )
        })
        .collect();
    assert_eq!(
        seen,
        [
            (
                "Alpha",
                alpha.org_id.as_str(),
                fvoci_model::WorkspaceRole::Member
            ),
            (
                "Beta",
                beta.org_id.as_str(),
                fvoci_model::WorkspaceRole::Admin
            ),
        ]
    );
    bob.get(&format!("/boards/{}", board.id)).await.check_ok();
    let alphas = th
        .app
        .store()
        .for_org(&alpha.org_id)
        .expect("an identifier");
    assert_eq!(
        alphas
            .count_rows("organization_members")
            .await
            .expect("the rows are counted"),
        2,
        "a member of the organization too"
    );

    th.teardown().await;
}

/// A workspace's administrator makes a link for members; nobody else does. The link's
/// token is told once and stored as its digest; whoever accepts the link, with a new
/// account or with the one they have, is a member of the workspace.
#[tokio::test]
async fn a_workspace_administrator_invites_members_by_a_link() {
    let th = Setup::native().start().await.init_basic().await;
    let admin = th.client.me().await;
    let invitations = format!("/w/{ROOT_WORKSPACE_ID}/invitations");

    // Only an administrator of the workspace, and only for members.
    let refused = th
        .client2
        .post(&invitations, &json!({"role": "member"}))
        .await;
    refused.check_error(
        StatusCode::FORBIDDEN,
        "only the administrators of the workspace invite its members",
    );
    th.anonymous()
        .post(&invitations, &json!({"role": "member"}))
        .await
        .check_unauthorized();
    let before = links(&th).await;
    for (body, place) in [
        (json!({"role": "admin"}), "/role"),
        (json!({"role": "member", "boardId": admin.id}), "/boardId"),
        (
            json!({"role": "member", "boardRole": "editor"}),
            "/boardRole",
        ),
    ] {
        let refused = th.client.post(&invitations, &body).await;
        refused.check_bad_request();
        assert_eq!(refused.refused_at(), place, "{body}");
    }
    assert_eq!(links(&th).await, before, "a refused request makes no link");

    let link = member_link(&th.client).await;
    assert_eq!(links(&th).await, before + 1);
    let token = link.token.clone().expect("the token of a new link");
    assert_eq!(
        token.len(),
        fvoci_server::auth::secret::SECRET_LENGTH,
        "256 random bits: {link:?}"
    );
    assert_eq!(link.workspace_id, ROOT_WORKSPACE_ID);
    assert_eq!(link.role, fvoci_model::WorkspaceRole::Member);
    assert_eq!(link.created_by, admin.id);
    assert_eq!(link.expires_at - link.create_at, INVITATION_LIFETIME_MILLIS);
    assert_eq!(
        (link.max_uses, link.uses),
        (None, 0),
        "as often as it is used"
    );
    assert_eq!((&link.board_id, link.board_role), (&None, None));

    // The store has the digest of the token, and the token nowhere.
    let stored = of_the_link(&th, &link.id, "token_hash").await;
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].len(), 64, "a SHA-256 digest in hexadecimal");
    assert!(!stored[0].contains(&token) && stored[0] != token);

    // Somebody without an account accepts with one to make. It signs nobody in.
    let accepted = th
        .anonymous()
        .post(
            "/invitations/accept",
            &json!({"token": token, "account": account("newcomer")}),
        )
        .await;
    accepted.check_ok();
    assert_eq!(accepted.value(), json!({"workspaceId": ROOT_WORKSPACE_ID}));
    assert_eq!(accepted.set_cookies(), Vec::<&str>::new());
    let newcomer = th.login("newcomer", PASSWORD).await;
    assert!(!newcomer.me().await.is_admin);
    let workspaces: Vec<UserWorkspace> = newcomer.get("/users/me/workspaces").await.ok_json();
    assert_eq!(
        workspaces.len(),
        1,
        "the workspace of the link: {workspaces:?}"
    );
    assert_eq!(workspaces[0].id, ROOT_WORKSPACE_ID);
    assert_eq!(workspaces[0].role, fvoci_model::WorkspaceRole::Member);
    assert_eq!(workspaces[0].name, ORG_NAME);
    assert_eq!(of_the_link(&th, &link.id, "uses").await, ["1"]);

    // Who is a member already is let in again, and uses nothing of the link; a role
    // above a member's stays what it is.
    for client in [&newcomer, &th.client] {
        let again = client
            .post("/invitations/accept", &json!({"token": token}))
            .await;
        again.check_ok();
        assert_eq!(again.value(), json!({"workspaceId": ROOT_WORKSPACE_ID}));
    }
    assert_eq!(of_the_link(&th, &link.id, "uses").await, ["1"]);
    let workspaces: Vec<UserWorkspace> = th.client.get("/users/me/workspaces").await.ok_json();
    assert_eq!(workspaces[0].role, fvoci_model::WorkspaceRole::Admin);

    // The link lets in as many as use it.
    th.anonymous()
        .post(
            "/invitations/accept",
            &json!({"token": token, "account": account("another")}),
        )
        .await
        .check_ok();
    assert_eq!(of_the_link(&th, &link.id, "uses").await, ["2"]);

    th.teardown().await;
}

/// A request that accepts says who accepts in one way: a session, or an account to
/// make. Neither, and both, are refused for what they are, whatever the token.
#[tokio::test]
async fn a_request_that_accepts_has_a_session_or_an_account_to_make() {
    let th = Setup::native().start().await.init_basic().await;
    for token in [th.member_link().await, "no-link".to_owned()] {
        let both = th
            .client2
            .post(
                "/invitations/accept",
                &json!({"token": token, "account": account("a-third")}),
            )
            .await;
        both.check_bad_request();
        assert_eq!(both.refused_at(), "/account");
        th.anonymous()
            .post("/invitations/accept", &json!({"token": token}))
            .await
            .check_unauthorized();
    }
    th.teardown().await;
}

/// Whoever holds a link sees where it leads and who made it, signed in or not, and
/// nothing else: a name each. Looking uses nothing of the link.
#[tokio::test]
async fn a_link_shows_whoever_holds_it_where_it_leads_and_who_made_it() {
    let th = Setup::native().start().await.init_basic().await;
    let admin = th.client.me().await;
    let link = member_link(&th.client).await;
    let token = link.token.clone().expect("the token of a new link");

    for client in [&th.anonymous(), &th.client2] {
        let preview = client
            .post("/invitations/preview", &json!({"token": token}))
            .await;
        preview.check_ok();
        assert_eq!(
            preview.value(),
            json!({"workspaceName": ORG_NAME, "inviterName": admin.username})
        );
    }
    th.client
        .patch("/users/me", &json!({"displayName": "Ada Lovelace"}))
        .await
        .check_ok();
    let preview = th
        .anonymous()
        .post("/invitations/preview", &json!({"token": token}))
        .await
        .value();
    assert_eq!(preview["inviterName"], json!("Ada Lovelace"), "{preview}");
    assert_eq!(of_the_link(&th, &link.id, "uses").await, ["0"]);

    th.teardown().await;
}

/// A token that lets nobody in is answered in one way, whatever is wrong with it and
/// whoever asks: no such link, one that ended, and one whose maker may no longer
/// invite. Nothing is stored for it.
#[tokio::test]
async fn a_link_that_lets_nobody_in_answers_as_no_link_at_all() {
    let th = Setup::native().start().await.init_basic().await;
    let third = th.add_user("third").await;
    let users = th.app.registered_user_count().await.expect("the users");

    let ended = member_link(&th.client).await;
    th.app
        .store()
        .execute_unchecked(&format!(
            "UPDATE invitations SET expires_at = create_at WHERE CAST(id AS TEXT) = '{}'",
            ended.id
        ))
        .await
        .expect("the link is ended");
    // The maker of this one stops administering the workspace after making it.
    th.app
        .store()
        .execute_unchecked(&format!(
            "UPDATE workspace_members SET role = 'admin' WHERE CAST(user_id AS TEXT) = '{}'",
            third.me().await.id
        ))
        .await
        .expect("a second administrator");
    let orphaned = member_link(&third).await;
    th.me_as_no_admin(&third).await;

    let tokens = [
        ("no such link", fvoci_server::auth::secret::new_secret()),
        ("no token", String::new()),
        ("an ended link", ended.token.clone().expect("a token")),
        (
            "a link of a former administrator",
            orphaned.token.clone().expect("a token"),
        ),
    ];
    let mut answers = Vec::new();
    for (case, token) in &tokens {
        let signed_in = th
            .client2
            .post("/invitations/accept", &json!({"token": token}))
            .await;
        let with_account = th
            .anonymous()
            .post(
                "/invitations/accept",
                &json!({"token": token, "account": account("nobody")}),
            )
            .await;
        let looked_at = th
            .anonymous()
            .post("/invitations/preview", &json!({"token": token}))
            .await;
        for answer in [signed_in, with_account, looked_at] {
            answer.check_status(StatusCode::NOT_FOUND);
            assert_eq!(answer.error_reason(), "invitation_unavailable", "{case}");
            assert_eq!(
                answer.problem_type(),
                "/problems/invitation-unavailable",
                "{case}"
            );
            answers.push((*case, said(&answer)));
        }
    }
    for (case, answer) in &answers {
        assert_eq!(answer, &answers[0].1, "{case}: one answer for all");
    }
    assert_eq!(
        th.app.registered_user_count().await.expect("the users"),
        users,
        "no account was made for a link that lets nobody in"
    );
    for link_id in [&ended.id, &orphaned.id] {
        assert_eq!(of_the_link(&th, link_id, "uses").await, ["0"]);
    }

    th.teardown().await;
}
