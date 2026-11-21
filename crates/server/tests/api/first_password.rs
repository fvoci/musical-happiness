//! The first password of an account that has none: one made through a provider is
//! told so, sets one from its session once, and signs in with it from then on, into
//! its own space and into whatever takes a password.

use fvoci_model::{Container, ContainerMember, ContainerType, UserWorkspace, new_id};
use fvoci_store::testing::db_type_of_the_tests;
use fvoci_store::{DbType, Tenant};
use reqwest::StatusCode;
use serde_json::{Value, json};

use crate::common::{Client, PASSWORD, Setup, TestServer};
use crate::fake_oidc::{FakeProvider, Person};
use crate::oidc::{Browser, org_of, server_for, set_provider, with_key_and_private_addresses};
use crate::own_space::workspaces_and_own;
use crate::sso::{Passwords, admin_through_provider, require_sso};

const FIRST_PASSWORD: &str = "a first Pa$$word";

/// Whether the account of the client is told it has a password.
async fn has_password(client: &Client) -> Value {
    let me: Value = client.get("/users/me").await.ok_json();
    me["hasPassword"].clone()
}

/// A board in the user's own space that they administer, and its identifier.
async fn board_in_the_space(ts: &TestServer, own: &UserWorkspace, user_id: &str) -> String {
    let mut board = Container {
        id: new_id(),
        workspace_id: own.id.clone(),
        container_type: ContainerType::Private,
        title: "a board of the space".to_owned(),
        ..Container::default()
    };
    let tenant = Tenant::org(&own.org_id).expect("an identifier");
    ts.app
        .scoped(tenant)
        .store()
        .insert_container_with_admin(&mut board, user_id)
        .await
        .expect("a board of the space");
    board.id
}

/// The boards the session's user is listed as a member of, in every organization.
async fn boards_of(client: &Client) -> Vec<String> {
    let members: Vec<ContainerMember> = client.get("/users/me/memberships").await.ok_json();
    members
        .into_iter()
        .map(|member| member.container_id)
        .collect()
}

/// How many boards the session is told its account's own space holds, where it asks
/// what deleting the account comes to: nothing for a session the space is closed to.
async fn counted_in_the_space(client: &Client) -> Value {
    let deletion: Value = client.get("/users/me/deletion").await.ok_json();
    let space = &deletion["organizations"][0];
    assert_eq!(space["kind"], json!("personal"), "{deletion}");
    space["boards"].clone()
}

/// Signs in with the name and the password, and answers what the server said.
async fn login(ts: &TestServer, username: &str, password: &str) -> (StatusCode, Client) {
    let mut client = ts.anonymous();
    let answer = client.login(username, password).await;
    (answer.status, client)
}

/// An account that a provider's sign-in made has no password and is told so. A
/// session of its made through the provider just now, and no older one, sets the
/// first one without being asked for an old one, once: the other
/// sessions of the account end, this one goes on under its new cookie, and a second
/// first password is refused. The password then signs in, and that session opens the
/// account's own space; an organization that takes only its provider's sessions is not
/// opened by it, and says so, while the own space is.
#[tokio::test]
async fn an_account_made_through_a_provider_sets_its_first_password_once() {
    let fake = FakeProvider::start().await;
    let ts = server_for(&fake).await;
    let org_id = org_of(&ts).await;
    set_provider(&ts, &org_id, &fake).await;
    let browser = Browser::new(&ts, &fake);
    let mut person = Person::new("subject-ada", "ada@example.com");
    person.preferred_username = Some("ada".to_owned());
    fake.sign_in_as(person);
    let client = ts.client_with_token(browser.sign_in(&org_id).await.token());
    let elsewhere = ts.client_with_token(browser.sign_in(&org_id).await.token());
    assert_eq!(has_password(&client).await, json!(false));
    assert_eq!(
        has_password(&ts.client).await,
        json!(true),
        "an account that registered with one"
    );
    let (refused, _) = login(&ts, "ada", FIRST_PASSWORD).await;
    assert_eq!(
        refused,
        StatusCode::UNAUTHORIZED,
        "no password signs in yet"
    );

    // A password is checked as any new one is, and nobody sets one without a session.
    let short = client
        .put("/users/me/password", &json!({"password": "short"}))
        .await;
    short.check_bad_request();
    assert_eq!(short.refused_at(), "/password", "{}", short.text());
    ts.anonymous()
        .put("/users/me/password", &json!({"password": FIRST_PASSWORD}))
        .await
        .check_unauthorized();
    // An account that has a password changes it by the old one, not here.
    ts.client
        .put("/users/me/password", &json!({"password": FIRST_PASSWORD}))
        .await
        .check_status(StatusCode::CONFLICT);
    assert_eq!(has_password(&client).await, json!(false), "nothing was set");

    // A session made a while ago vouches for nothing: who holds one signs in through
    // the provider again, and sets the password from the session that comes of it.
    let ada = client.me().await.id;
    ts.app
        .store()
        .execute_unchecked(&format!(
            "UPDATE sessions SET created_at = created_at - 600000 WHERE user_id = '{ada}'"
        ))
        .await
        .expect("the sessions are aged");
    let stale = client
        .put("/users/me/password", &json!({"password": FIRST_PASSWORD}))
        .await;
    stale.check_forbidden();
    assert_eq!(stale.error_reason(), "sign_in_again", "{}", stale.text());
    assert_eq!(has_password(&client).await, json!(false), "nothing was set");
    let before = client;
    let client = ts.client_with_token(browser.sign_in(&org_id).await.token());

    let set = client
        .put("/users/me/password", &json!({"password": FIRST_PASSWORD}))
        .await;
    set.check_no_content();
    let cookie = set.session_cookie().expect("the session's new cookie");
    client.get("/users/me").await.check_unauthorized();
    elsewhere.get("/users/me").await.check_unauthorized();
    before.get("/users/me").await.check_unauthorized();
    let client = ts.client_with_token(&cookie);
    assert_eq!(has_password(&client).await, json!(true));
    client
        .put(
            "/users/me/password",
            &json!({"password": "another Pa$$word"}),
        )
        .await
        .check_status(StatusCode::CONFLICT);

    // The password signs in, in any letter case of the name, and opens what the
    // provider's session opened.
    let (status, by_password) = login(&ts, "ADA", FIRST_PASSWORD).await;
    assert!(status.is_success(), "the password signs in: {status}");
    let (listed, own) = workspaces_and_own(&by_password).await;
    assert!(!listed.is_empty(), "a member of the organization");
    let own_path = format!("/w/{}", own.id);
    let org_path = format!("/w/{}", listed[0].id);
    by_password.get(&own_path).await.check_ok();
    by_password.get(&org_path).await.check_ok();

    // The organization takes only its provider's sessions from now on: the password
    // opens the account's own space still, and not the organization.
    let admin = admin_through_provider(&ts, &fake, &org_id).await;
    require_sso(&admin, &org_id, &fake, Passwords::Refused)
        .await
        .check_ok();
    let (status, by_password) = login(&ts, "ada", FIRST_PASSWORD).await;
    assert!(status.is_success(), "the password signs in: {status}");
    by_password.get(&own_path).await.check_ok();
    let closed = by_password.get(&org_path).await;
    assert_eq!(closed.error_reason(), "sso_required", "{}", closed.text());
    let (listed, own_after) = workspaces_and_own(&by_password).await;
    assert!(listed.iter().all(|workspace| workspace.sso_required));
    assert_eq!(own_after, own, "their own space asks for no provider");
    ts.teardown().await;
    fake.stop();
}

/// What a session is told is closed to it for want of the account's password.
async fn check_asks_for_the_password(client: &Client, path: &str) {
    let refused = client.get(path).await;
    refused.check_unauthorized();
    assert_eq!(
        refused.error_reason(),
        "password_sign_in_required",
        "{path}: {}",
        refused.text()
    );
}

/// The account's own space takes any session of it until it has a password, and from
/// then on only one made with the password: the provider's session that set it is
/// told so by the routes of the space, by the list of the workspaces, by the list of
/// the boards and by what deleting the account comes to, and goes on opening the
/// organization. A session made with the password opens both.
#[tokio::test]
async fn a_space_takes_the_password_alone_once_its_account_has_one() {
    let fake = FakeProvider::start().await;
    let ts = server_for(&fake).await;
    let org_id = org_of(&ts).await;
    set_provider(&ts, &org_id, &fake).await;
    let browser = Browser::new(&ts, &fake);
    let mut person = Person::new("subject-ada", "ada@example.com");
    person.preferred_username = Some("ada".to_owned());
    fake.sign_in_as(person);
    let client = ts.client_with_token(browser.sign_in(&org_id).await.token());

    // Without a password the space takes the provider's session.
    let (listed, own) = workspaces_and_own(&client).await;
    assert!(!own.password_required, "no password to ask for: {own:?}");
    let own_path = format!("/w/{}", own.id);
    let org_path = format!("/w/{}", listed[0].id);
    client.get(&own_path).await.check_ok();
    let board = board_in_the_space(&ts, &own, &client.me().await.id).await;
    assert!(boards_of(&client).await.contains(&board), "listed");
    assert_eq!(counted_in_the_space(&client).await, json!(1));

    let set = client
        .put("/users/me/password", &json!({"password": FIRST_PASSWORD}))
        .await;
    set.check_no_content();
    let cookie = set.session_cookie().expect("the session's new cookie");
    let client = ts.client_with_token(&cookie);

    // The session that set it was made through the provider, and is told wherever
    // the space is that it takes the password now.
    let (_, closed) = workspaces_and_own(&client).await;
    assert!(closed.password_required, "{closed:?}");
    assert!(!closed.sso_required, "no provider is asked for: {closed:?}");
    check_asks_for_the_password(&client, &own_path).await;
    check_asks_for_the_password(&client, &format!("{own_path}/boards")).await;
    for suffix in ["/archive/export", "/boards/search?q=board", "/audit"] {
        check_asks_for_the_password(&client, &format!("{own_path}{suffix}")).await;
    }
    assert!(!boards_of(&client).await.contains(&board), "left out");
    assert_eq!(counted_in_the_space(&client).await, Value::Null);
    client.get(&org_path).await.check_ok();
    // And so is a session made through the provider afterwards.
    let later = ts.client_with_token(browser.sign_in(&org_id).await.token());
    check_asks_for_the_password(&later, &own_path).await;
    later.get(&org_path).await.check_ok();

    let (status, by_password) = login(&ts, "ada", FIRST_PASSWORD).await;
    assert!(status.is_success(), "the password signs in: {status}");
    let (_, opened) = workspaces_and_own(&by_password).await;
    assert!(!opened.password_required, "by the password: {opened:?}");
    by_password.get(&own_path).await.check_ok();
    by_password.get(&org_path).await.check_ok();
    assert!(boards_of(&by_password).await.contains(&board), "listed");
    assert_eq!(counted_in_the_space(&by_password).await, json!(1));
    ts.teardown().await;
    fake.stop();
}

fn open(config: &mut fvoci_server::config::Configuration) {
    "open".clone_into(&mut config.org_creation);
}

/// An organization that takes only its provider's sessions asks the provider of who
/// has a place in it, and of nobody else. An account that registers without an
/// organization on the same server signs in with its password and opens its own
/// space, which asks for no provider; a member's password opens their own space and
/// not the organization.
#[tokio::test]
async fn no_provider_is_asked_of_an_account_in_no_organization() {
    if db_type_of_the_tests() != DbType::Postgres {
        return;
    }
    let fake = FakeProvider::start().await;
    let ts = Setup::native()
        .trusting_oidc_provider(&fake.certificate_der)
        .configure(with_key_and_private_addresses)
        .configure(open)
        .start()
        .await
        .init_basic()
        .await;
    let org_id = org_of(&ts).await;
    set_provider(&ts, &org_id, &fake).await;
    let admin = admin_through_provider(&ts, &fake, &org_id).await;
    require_sso(&admin, &org_id, &fake, Passwords::Refused)
        .await
        .check_ok();

    let alone = json!({
        "username": "carol", "email": "carol@sample.com", "password": PASSWORD,
    });
    ts.anonymous()
        .post("/register", &alone)
        .await
        .check_created();
    let (status, carol) = login(&ts, "carol", PASSWORD).await;
    assert!(status.is_success(), "the password signs in: {status}");
    let (listed, own) = workspaces_and_own(&carol).await;
    assert_eq!(listed, [], "in no organization");
    assert!(!own.sso_required, "their own space asks for no provider");
    carol.get(&format!("/w/{}", own.id)).await.check_ok();

    // The member's password session: the organization is closed to it and says why.
    let member = ts.client2.me().await;
    let (status, by_password) = login(&ts, &member.username, PASSWORD).await;
    assert!(status.is_success(), "the password signs in: {status}");
    let (listed, own) = workspaces_and_own(&by_password).await;
    assert!(listed.iter().all(|workspace| workspace.sso_required));
    let closed = by_password.get(&format!("/w/{}", listed[0].id)).await;
    assert_eq!(closed.error_reason(), "sso_required", "{}", closed.text());
    by_password.get(&format!("/w/{}", own.id)).await.check_ok();

    let store = ts.app.store();
    let asked = async |user_id: &str| {
        store
            .required_provider_for(user_id, &org_id)
            .await
            .expect("read")
    };
    assert!(asked(&member.id).await.is_some(), "asked of a member");
    assert_eq!(
        asked(&carol.me().await.id).await,
        None,
        "and not of an account that has no place in the organization"
    );
    ts.teardown().await;
    fake.stop();
}
