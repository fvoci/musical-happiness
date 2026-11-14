//! A registration that waits for its address is no account: no lookup of people finds
//! its name, its address or the identifier its account will have, and nothing takes
//! that identifier for a person, whether it registered for itself or by a link.

use fvoci_model::{Board, BoardType, Invitation};
use fvoci_store::{Signup, SignupLookup};
use reqwest::StatusCode;
use serde_json::json;

use crate::common::{ApiResponse, Client, Setup, TestServer};
use crate::email_verification::{SIGNUP_PASSWORD, complete_elsewhere, link_token, registration};
use crate::fake_smtp::FakeSmtp;

/// The registration with the user name, which waits.
async fn waiting(th: &TestServer, username: &str) -> Signup {
    th.app
        .store()
        .live_signup(
            SignupLookup::Username(username),
            fvoci_server::auth::now_millis(),
        )
        .await
        .expect("read")
        .unwrap_or_else(|| panic!("{username} waits"))
}

/// Accepts the link of `invitation` with a new account `username`, which waits.
async fn accept(th: &TestServer, invitation: &Invitation, username: &str) {
    th.anonymous()
        .post(
            "/invitations/accept",
            &json!({"token": invitation.token, "account": {
                "username": username,
                "email": format!("{username}@unseen.test"),
                "password": SIGNUP_PASSWORD,
            }}),
        )
        .await
        .check_status(StatusCode::ACCEPTED);
}

/// Checks that an answer names nothing of the registrations.
#[track_caller]
fn check_unseen(answer: &ApiResponse, what: &str, hidden: &[Signup]) {
    let text = answer.text().to_lowercase();
    for signup in hidden {
        for part in [&signup.username, &signup.email, &signup.id] {
            assert!(
                !text.contains(&part.to_lowercase()),
                "{what} names {part} of a registration: {text}"
            );
        }
    }
}

#[tokio::test]
async fn a_registration_that_waits_is_in_no_lookup_of_people() {
    // A server that takes several organizations has registrations of their own wait
    // beside its accounts; SQLite holds one organization.
    let several = fvoci_store::testing::db_type_of_the_tests() == fvoci_store::DbType::Postgres;
    let smtp = FakeSmtp::start_on(0).await;
    let mut setup = Setup::native()
        .smtp(smtp.settings())
        .configure(|config| config.mail.email_verification = true);
    if several {
        setup = setup.configure(|config| {
            config.org_creation = fvoci_server::config::ORG_CREATION_OPEN.to_owned();
        });
    }
    let th = setup.start().await;
    // The first user, who confirmed, and whose lookups these are.
    th.anonymous()
        .post("/register", &registration("ada", "en"))
        .await
        .check_status(StatusCode::ACCEPTED);
    let confirmed = complete_elsewhere(
        &th,
        &link_token(&smtp.received(1).await[0]),
        SIGNUP_PASSWORD,
    )
    .await;
    confirmed.check_status(StatusCode::OK);
    let ada = th.client_with_token(&confirmed.session_cookie().expect("a session"));
    let workspace_id = confirmed.value()["workspaceId"]
        .as_str()
        .expect("her workspace")
        .to_owned();
    let board: Board = ada
        .create_board(&Board {
            workspace_id: workspace_id.clone(),
            board_type: BoardType::Open,
            ..Board::default()
        })
        .await
        .created_json();

    // Those that wait: by a link for members, by a link for a guest of the board, and
    // for an organization of its own where the server takes another.
    let mut usernames = vec!["unseen-member", "unseen-guest"];
    if several {
        let mut own = registration("unseen-own", "en");
        own["email"] = json!("unseen-own@unseen.test");
        th.anonymous()
            .post("/register", &own)
            .await
            .check_status(StatusCode::ACCEPTED);
        usernames.push("unseen-own");
    }
    let links = format!("/w/{workspace_id}/invitations");
    let for_members: Invitation = ada
        .post(&links, &json!({"role": "member"}))
        .await
        .created_json();
    accept(&th, &for_members, "unseen-member").await;
    let for_a_guest: Invitation = ada
        .post(
            &links,
            &json!({"role": "guest", "boardId": board.id, "boardRole": "viewer"}),
        )
        .await
        .created_json();
    accept(&th, &for_a_guest, "unseen-guest").await;
    let mut hidden = Vec::new();
    for username in usernames {
        hidden.push(waiting(&th, username).await);
    }

    let me = ada.me().await;
    let org_id = th
        .app
        .store()
        .resolve_workspace(&me.id, &workspace_id)
        .await
        .expect("read")
        .expect("her workspace")
        .org_id;
    let users = format!("/w/{workspace_id}/users");
    let ids: Vec<&str> = hidden.iter().map(|signup| signup.id.as_str()).collect();
    let places = Places {
        workspace: &workspace_id,
        org: &org_id,
        board: &board.id,
    };
    lists_name_nothing(&ada, places, &hidden).await;
    // Found by what was typed: its name, its address, a part of either.
    for typed in ["unseen", "unseen-member", "unseen.test", "UNSEEN-GUEST"] {
        let found = ada.get(&format!("{users}?search={typed}")).await;
        found.check_status(StatusCode::OK);
        assert_eq!(found.value(), json!([]), "searching {typed}");
    }
    identifiers_are_nobodys(&ada, places, &ids).await;
    // Still nowhere after those were asked.
    lists_name_nothing(&ada, places, &hidden).await;
    th.teardown().await;
    smtp.stop().await;
}

/// Every list of people `ada` can ask for names nothing of the registrations.
async fn lists_name_nothing(ada: &Client, places: Places<'_>, hidden: &[Signup]) {
    let Places {
        workspace: workspace_id,
        org: org_id,
        board: board_id,
    } = places;
    for path in [
        format!("/w/{workspace_id}/users"),
        format!("/w/{workspace_id}/members"),
        format!("/w/{workspace_id}/boards/{board_id}/members"),
        format!("/org/{org_id}/members"),
        format!("/org/{org_id}/members?perPage=200"),
        format!("/w/{workspace_id}/invitations"),
        "/users/me/memberships".to_owned(),
        "/users/me/workspaces".to_owned(),
    ] {
        let answer = ada.get(&path).await;
        answer.check_status(StatusCode::OK);
        check_unseen(&answer, &path, hidden);
    }
}

/// Where `ada` looks people up.
#[derive(Clone, Copy)]
struct Places<'a> {
    workspace: &'a str,
    org: &'a str,
    board: &'a str,
}

/// By the identifier its account will have, a registration is nobody, as an identifier
/// of nobody is; nor is the identifier taken for a person, on the board or in the
/// organization.
async fn identifiers_are_nobodys(ada: &Client, places: Places<'_>, ids: &[&str]) {
    let Places {
        workspace: workspace_id,
        org: org_id,
        board: board_id,
    } = places;
    let users = format!("/w/{workspace_id}/users");
    let members = format!("/w/{workspace_id}/boards/{board_id}/members");
    let nobody = fvoci_model::new_id();
    let by_ids = ada.post(&users, &ids).await;
    by_ids.check_status(StatusCode::OK);
    assert_eq!(by_ids.value(), json!([]), "by their identifiers");
    for id in ids {
        let one = ada.get(&format!("{users}/{id}")).await;
        let none = ada.get(&format!("{users}/{nobody}")).await;
        assert_eq!(one.status, none.status, "as nobody: {}", one.text());
        one.check_status(StatusCode::NOT_FOUND);
        let added = ada
            .post(&members, &json!({"userId": id, "role": "viewer"}))
            .await;
        let added_nobody = ada
            .post(&members, &json!({"userId": nobody, "role": "viewer"}))
            .await;
        assert_eq!(added.status, added_nobody.status, "{}", added.text());
        assert!(added.status.is_client_error(), "{}", added.text());
        let role = ada
            .put(
                &format!("/org/{org_id}/members/{id}/role"),
                &json!({"role": "admin"}),
            )
            .await;
        assert!(role.status.is_client_error(), "{}", role.text());
    }
}
