//! Ports of `server/integrationtests/user_test.go` and of the user and login cases of
//! `server/app/auth_test.go`, plus tests of what the port changes on purpose.
//!
//! Not ported here: `TestGetUserList` (runs in the host-application mode this server does
//! not have) and `TestTeamUploadFile` (in the `files` module).

use fvoci_model::{ChangePasswordRequest, LoginRequest, RegisterRequest, Session, User, new_id};
use fvoci_server::auth::secret::new_secret;
use reqwest::StatusCode;
use serde_json::json;

use crate::common::{PASSWORD, ROOT_TEAM_ID, Setup, TEST_TEAM_ID, TestServer, UNKNOWN_ID};

const FAKE_USERNAME: &str = "fakeUsername";
const FAKE_EMAIL: &str = "mock@test.com";

fn register_request(password: &str) -> RegisterRequest {
    RegisterRequest {
        username: FAKE_USERNAME.to_owned(),
        email: FAKE_EMAIL.to_owned(),
        password: password.to_owned(),
        token: String::new(),
    }
}

fn login_request(username: &str, email: &str, password: &str) -> LoginRequest {
    LoginRequest {
        login_type: "normal".to_owned(),
        username: username.to_owned(),
        email: email.to_owned(),
        password: password.to_owned(),
        mfa_token: String::new(),
    }
}

/// `TestUserRegister`
#[tokio::test]
async fn user_register() {
    let th = Setup::native().start().await;
    let request = register_request(&new_id());

    th.client.post("/register", &request).await.check_ok();

    // The same user a second time: the first user exists, so a signup token is needed.
    let again = th.client.post("/register", &request).await;
    again.check_error(StatusCode::FORBIDDEN, "registration needs the signup token");
    assert_eq!(again.error_reason(), "signup_token_required");

    // With the token, the name is found to be taken.
    let mut with_token = register_request(&new_id());
    with_token.token = th.app.root_team().await.expect("root team").signup_token;
    let taken = th.client.post("/register", &with_token).await;
    taken.check_status(StatusCode::CONFLICT);
    assert_eq!(taken.error_reason(), "username_taken", "the name is taken");

    with_token.username = "another".to_owned();
    let taken = th.client.post("/register", &with_token).await;
    taken.check_status(StatusCode::CONFLICT);
    assert_eq!(taken.error_reason(), "email_taken", "the address is taken");

    th.teardown().await;
}

/// `TestUserLogin/"with nonexist user"`
#[tokio::test]
async fn user_login_with_nonexist_user() {
    let th = Setup::native().start().await;
    let request = login_request("nonexistuser", "", &new_id());

    let response = th.client.post("/login", &request).await;

    response.check_unauthorized();
    assert_eq!(
        response.error_message(),
        "incorrect login",
        "the answer does not say why"
    );
    th.teardown().await;
}

/// `TestUserLogin/"with registered user"`
#[tokio::test]
async fn user_login_with_registered_user() {
    let th = Setup::native().start().await;
    let password = new_id();
    th.client
        .post("/register", &register_request(&password))
        .await
        .check_ok();

    let response = th
        .client
        .post(
            "/login",
            &login_request(FAKE_USERNAME, FAKE_EMAIL, &password),
        )
        .await;

    let token = response.ok_json::<serde_json::Value>()["token"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    assert_eq!(
        token.len(),
        fvoci_server::auth::secret::SECRET_LENGTH,
        "the session token is a secret, not an identifier: its length"
    );
    assert!(
        !fvoci_model::is_id(&token),
        "the session token does not have the form of an identifier"
    );
    th.teardown().await;
}

/// `TestLogin` in `server/app/auth_test.go`, through the API: the failures all look the
/// same to the client, and a user signs in by name or by email address.
#[tokio::test]
async fn login_cases() {
    let th = Setup::native().start().await;
    th.client
        .post("/register", &register_request("testPassword"))
        .await
        .check_ok();

    let failures = [
        ("fail, missing login information", "", "", ""),
        ("fail, invalid username", "badUsername", "", ""),
        ("fail, invalid email", "", "badEmail", ""),
        ("fail, invalid password", FAKE_USERNAME, "", "badPassword"),
    ];
    for (title, username, email, password) in failures {
        let response = th
            .client
            .post("/login", &login_request(username, email, password))
            .await;
        assert_eq!(response.status, StatusCode::UNAUTHORIZED, "{title}");
        assert_eq!(response.error_message(), "incorrect login", "{title}");
    }

    let successes = [
        ("success, using username", FAKE_USERNAME, ""),
        ("success, using email", "", FAKE_EMAIL),
    ];
    for (title, username, email) in successes {
        let response = th
            .client
            .post("/login", &login_request(username, email, "testPassword"))
            .await;
        assert_eq!(
            response.status,
            StatusCode::OK,
            "{title}: {}",
            response.text()
        );
    }

    // A login type other than "normal" is refused before anything is looked up.
    let mut other_type = login_request(FAKE_USERNAME, "", "testPassword");
    other_type.login_type = "saml".to_owned();
    let response = th.client.post("/login", &other_type).await;
    response.check_bad_request();
    assert_eq!(
        response.error_message(),
        "invalid login type",
        "the type is the reason"
    );

    // A body is a JSON object. The members in a list, in the order the server happens to
    // declare them, are no login.
    let response = th
        .client
        .send_raw_body(
            reqwest::Method::POST,
            "/login",
            &format!(r#"["normal","{FAKE_USERNAME}","","testPassword",""]"#),
        )
        .await;
    response.check_bad_request();
    assert_eq!(response.error_code(), "bad_request", "{}", response.text());

    // A body that is not JSON is the sender's mistake.
    let response = th
        .client
        .send_raw_body(reqwest::Method::POST, "/login", "{not json")
        .await;
    response.check_bad_request();
    assert_eq!(response.error_code(), "bad_request", "{}", response.text());

    th.teardown().await;
}

/// `TestRegisterUser` in `server/app/auth_test.go`, through the API.
#[tokio::test]
async fn register_user_cases() {
    let th = Setup::native().start().await;

    // "fail, missing login information" and "fail, invalid password": the request is
    // checked before a user is created.
    let cases = [
        (
            json!({"username": "", "email": "", "password": ""}),
            "username is required",
        ),
        (
            json!({"username": "newUsername", "email": "", "password": "testPassword"}),
            "email is required",
        ),
        (
            json!({"username": "newUsername", "email": "not-an-address", "password": "testPassword"}),
            "invalid email format",
        ),
        (
            json!({"username": "newUsername", "email": "new@example.com", "password": ""}),
            "password is required",
        ),
        (
            json!({"username": "newUsername", "email": "new@example.com", "password": "test"}),
            "password must be at least",
        ),
    ];
    for (body, message) in cases {
        let response = th.client.post("/register", &body).await;
        assert_eq!(
            response.status,
            StatusCode::BAD_REQUEST,
            "{body}: {}",
            response.text()
        );
        assert!(
            response.error_message().contains(message),
            "{body}: expected {message:?} in {:?}",
            response.error_message()
        );
    }
    assert_eq!(
        th.app.registered_user_count().await.expect("user count"),
        0,
        "no refused request created a user"
    );

    // "success"
    let body = json!({"username": " newUsername ", "email": " new@example.com ", "password": "testPassword"});
    th.client.post("/register", &body).await.check_ok();
    let user = th
        .app
        .store()
        .get_user_by_username("newUsername")
        .await
        .expect("the name is stored trimmed");
    assert_eq!(
        user.email, "new@example.com",
        "the address is stored trimmed"
    );
    assert!(
        user.password_hash
            .starts_with("$argon2id$v=19$m=19456,t=2,p=1$"),
        "the password is stored as an Argon2id hash with the current parameters: {}",
        user.password_hash
    );
    assert!(
        !user.password_hash.contains("testPassword"),
        "and not as itself"
    );

    // A wrong signup token is refused.
    let body = json!({"username": "second", "email": "second@example.com", "password": "testPassword", "token": "wrong"});
    let response = th.client.post("/register", &body).await;
    response.check_error(StatusCode::FORBIDDEN, "the signup token is not valid");
    assert_eq!(response.error_reason(), "signup_token_invalid");

    th.teardown().await;
}

/// `TestGetMe/"not login yet"`
#[tokio::test]
async fn get_me_not_login_yet() {
    let th = Setup::native().start().await;

    let response = th.client.get("/users/me").await;

    response.check_unauthorized();
    assert_eq!(
        response.error_message(),
        "no session token",
        "there is no session"
    );
    th.teardown().await;
}

/// `TestGetMe/"logged in"`
#[tokio::test]
async fn get_me_logged_in() {
    let th = Setup::native().start().await;
    let password = new_id();
    let client = th
        .register_and_login(FAKE_USERNAME, FAKE_EMAIL, &password, "")
        .await;

    let response = client.get("/users/me").await;

    let me: User = response.ok_json();
    assert_eq!(me.email, "", "the address is not sent");
    assert_eq!(me.username, FAKE_USERNAME, "the name is");
    let raw = response.value();
    assert!(raw.get("password").is_none(), "no password member: {raw}");
    assert!(raw.get("email").is_none(), "no email member: {raw}");
    assert!(raw.get("permissions").is_none(), "no permissions: {raw}");

    // The original reports a permission of its host application for a channel named in
    // the query. There are no channels; the parameter means nothing.
    let with_channel = client.get("/users/me?channelID=any-channel").await.value();
    assert!(with_channel.get("permissions").is_none(), "{with_channel}");

    th.teardown().await;
}

/// `TestGetUser/"me's id"` and `"nonexist user"`
#[tokio::test]
async fn get_user() {
    let th = Setup::native().start().await;
    let client = th
        .register_and_login(FAKE_USERNAME, FAKE_EMAIL, &new_id(), "")
        .await;
    let me = client.me().await;

    // "me's id"
    let user: User = client.get(&format!("/users/{}", me.id)).await.ok_json();
    assert_eq!(user.id, me.id, "the same user");
    assert_eq!(user.username, me.username, "the same name");

    // "nonexist user"
    let response = client.get("/users/nonexistid").await;
    response.check_not_found();

    th.teardown().await;
}

/// `TestGetUser` in `server/app/auth_test.go`.
#[tokio::test]
async fn app_get_user_cases() {
    let th = Setup::native().start().await.init_basic().await;
    let me = th.client.me().await;

    assert!(th.app.user("").await.is_err(), "fail, missing id");
    assert!(th.app.user("badID").await.is_err(), "fail, invalid id");
    assert_eq!(
        th.app.user(&me.id).await.expect("success").id,
        me.id,
        "success"
    );

    th.teardown().await;
}

/// The users-list routes and the profile fields other users see.
#[tokio::test]
async fn users_list_and_profiles() {
    let th = Setup::native().start().await.init_basic().await;
    let user1 = th.client.me().await;
    let user2 = th.client2.me().await;

    let listed: Vec<User> = th
        .client
        .post("/users", &[&user1.id, &user2.id])
        .await
        .ok_json();
    let mut names: Vec<&str> = listed.iter().map(|user| user.username.as_str()).collect();
    names.sort_unstable();
    assert_eq!(names, ["user1", "user2"], "both users are listed");

    let empty: Vec<String> = Vec::new();
    let response = th.client.post("/users", &empty).await;
    response.check_bad_request();
    assert_eq!(
        response.error_message(),
        "the list of user IDs is empty",
        "an empty list is refused"
    );

    th.client
        .post("/users", &["nonexistent"])
        .await
        .check_not_found();

    let team_users: Vec<User> = th
        .client
        .get(&format!("/teams/{ROOT_TEAM_ID}/users"))
        .await
        .ok_json();
    assert_eq!(team_users.len(), 2, "the team has both users");
    let found: Vec<User> = th
        .client
        .get(&format!("/teams/{ROOT_TEAM_ID}/users?search=user2"))
        .await
        .ok_json();
    assert_eq!(found.len(), 1, "the search narrows the list");
    assert_eq!(found[0].id, user2.id, "to the user that matches");

    let by_id: Vec<User> = th
        .client
        .post(&format!("/teams/{ROOT_TEAM_ID}/users"), &[&user2.id])
        .await
        .ok_json();
    assert_eq!(by_id.len(), 1, "one user was asked for");

    th.teardown().await;
}

/// `TestUserChangePassword`
#[tokio::test]
async fn user_change_password() {
    let th = Setup::native().start().await;
    let password = new_id();
    let client = th
        .register_and_login(FAKE_USERNAME, FAKE_EMAIL, &password, "")
        .await;
    let me = client.me().await;
    let new_password = new_id();

    let request = ChangePasswordRequest {
        old_password: password.clone(),
        new_password: new_password.clone(),
    };
    client
        .post(&format!("/users/{}/changepassword", me.id), &request)
        .await
        .check_ok();

    // The new password signs in and the old one no longer does.
    th.login(FAKE_USERNAME, &new_password).await;
    let mut old = th.anonymous();
    old.login(FAKE_USERNAME, &password)
        .await
        .check_unauthorized();

    th.teardown().await;
}

/// `TestChangePassword` in `server/app/auth_test.go`.
#[tokio::test]
async fn app_change_password_cases() {
    let th = Setup::native().start().await.init_basic().await;
    let session = th
        .app
        .authenticator()
        .get_session(&th.client.token)
        .await
        .expect("the first client has a session");

    // "fail, missing login information" and "fail, invalid userId": a session of nobody,
    // and one of a user who is not there.
    for user_id in [String::new(), "badID".to_owned(), UNKNOWN_ID.to_owned()] {
        let of_nobody = fvoci_model::Session {
            user_id,
            ..session.clone()
        };
        assert!(
            th.app.change_password(&of_nobody, "", "").await.is_err(),
            "a session without a user changes no password: {of_nobody:?}"
        );
    }
    assert!(
        th.app
            .change_password(&session, "wrongPassword", "newPassword")
            .await
            .is_err(),
        "fail, invalid password"
    );
    th.app
        .change_password(&session, PASSWORD, "newPassword")
        .await
        .expect("success, with the right old password");

    th.teardown().await;
}

/// `TestUpdateUserPassword` in `server/app/auth_test.go`.
#[tokio::test]
async fn app_update_user_password_cases() {
    let th = Setup::native().start().await.init_basic().await;

    assert!(
        th.app.reset_user_password("", "").await.is_err(),
        "fail, missing login information"
    );
    assert!(
        th.app.reset_user_password("badUsername", "").await.is_err(),
        "fail, invalid username"
    );
    th.app
        .reset_user_password("user1", "testPassword")
        .await
        .expect("success, username");
    th.login("user1", "testPassword").await;

    th.teardown().await;
}

/// Security fix: the original lets any signed-in user set another user's password if
/// they know that user's current one (`server/api/auth.go:247`). The port refuses.
#[tokio::test]
async fn change_password_of_another_user_is_forbidden() {
    let th = Setup::native().start().await.init_basic().await;
    let user2 = th.client2.me().await;
    let request = ChangePasswordRequest {
        old_password: PASSWORD.to_owned(),
        new_password: "another-password".to_owned(),
    };

    let response = th
        .client
        .post(&format!("/users/{}/changepassword", user2.id), &request)
        .await;

    response.check_forbidden();
    // The password did not change.
    th.login("user2", PASSWORD).await;

    // A wrong old password on one's own account is a bad request, as in the original.
    let user1 = th.client.me().await;
    let wrong = ChangePasswordRequest {
        old_password: "not-the-password".to_owned(),
        new_password: "another-password".to_owned(),
    };
    let response = th
        .client
        .post(&format!("/users/{}/changepassword", user1.id), &wrong)
        .await;
    response.check_bad_request();

    th.teardown().await;
}

/// Logging out ends the session.
#[tokio::test]
async fn logout_ends_the_session() {
    let th = Setup::native().start().await.init_basic().await;

    th.client.post_empty("/logout").await.check_ok();

    th.client.get("/users/me").await.check_unauthorized();
    // The other user's session is untouched.
    th.client2.get("/users/me").await.check_ok();
    th.teardown().await;
}

/// The session token is accepted from the cookie, both authorization schemes and the
/// query, as in `server/services/auth/request_parser.go`.
#[tokio::test]
async fn session_token_locations() {
    let th = Setup::native().start().await.init_basic().await;
    let token = th.client.token.clone();
    let anonymous = th.anonymous();
    let request = || anonymous.request(reqwest::Method::GET, "/users/me");

    let by_cookie = request().header("Cookie", format!("FVOCIAUTHTOKEN={token}"));
    assert_eq!(
        crate::common::Client::send(by_cookie).await.status,
        StatusCode::OK,
        "cookie"
    );
    let by_bearer = request().header("Authorization", format!("Bearer {token}"));
    assert_eq!(
        crate::common::Client::send(by_bearer).await.status,
        StatusCode::OK,
        "bearer"
    );
    let by_token = request().header("Authorization", format!("token {token}"));
    assert_eq!(
        crate::common::Client::send(by_token).await.status,
        StatusCode::OK,
        "token scheme"
    );
    let by_query = anonymous.request(
        reqwest::Method::GET,
        &format!("/users/me?access_token={token}"),
    );
    assert_eq!(
        crate::common::Client::send(by_query).await.status,
        StatusCode::OK,
        "query"
    );

    let wrong = request().header("Authorization", "Bearer not-a-session");
    assert_eq!(
        crate::common::Client::send(wrong).await.status,
        StatusCode::UNAUTHORIZED,
        "an unknown token"
    );

    th.teardown().await;
}

/// Preferences are read and patched by their owner only.
#[tokio::test]
async fn user_preferences() {
    let th = Setup::native().start().await.init_basic().await;
    let user1 = th.client.me().await;
    let user2 = th.client2.me().await;

    let before = th.client.get("/users/me/config").await.value();
    assert_eq!(before, json!([]), "a new user has no preferences");

    let patch = json!({"updatedFields": {"theme": "dark", "welcomePageViewed": "1"}});
    let after = th
        .client
        .put(&format!("/users/{}/config", user1.id), &patch)
        .await;
    let preferences = after.value();
    assert_eq!(
        preferences.as_array().map(Vec::len),
        Some(2),
        "{preferences}"
    );
    for preference in preferences.as_array().into_iter().flatten() {
        assert_eq!(preference["user_id"], json!(user1.id), "{preference}");
    }

    let patch = json!({"deletedFields": ["theme"]});
    let after = th
        .client
        .put(&format!("/users/{}/config", user1.id), &patch)
        .await
        .value();
    assert_eq!(
        after.as_array().map(Vec::len),
        Some(1),
        "one preference is left: {after}"
    );
    assert_eq!(after[0]["name"], json!("welcomePageViewed"), "{after}");
    assert_eq!(after[0]["value"], json!("1"), "{after}");

    // The settings of another user do not exist for this one, as those of nobody.
    for other in [user2.id.as_str(), "nobody"] {
        th.client
            .put(&format!("/users/{other}/config"), &patch)
            .await
            .check_error(StatusCode::NOT_FOUND, "user not found");
    }

    th.teardown().await;
}

/// The team routes.
#[tokio::test]
async fn teams() {
    let th = Setup::native().start().await.init_basic().await;

    let teams = th.client.get("/teams").await.value();
    assert_eq!(
        teams.as_array().map(Vec::len),
        Some(2),
        "the two teams of the test server: {teams}"
    );
    assert_eq!(
        teams[0]["id"],
        json!(ROOT_TEAM_ID),
        "the oldest team, the root team, comes first: {teams}"
    );

    // A team is asked for by its identifier. Any other identifier, well-formed or
    // not, names no team; the original answered the root team to whatever was asked.
    for no_team in [UNKNOWN_ID, "some-other-team", "0"] {
        let response = th.client.get(&format!("/teams/{no_team}")).await;
        response.check_error(StatusCode::NOT_FOUND, "team not found");
    }
    let team = th
        .client
        .get(&format!("/teams/{ROOT_TEAM_ID}"))
        .await
        .value();
    assert_eq!(team["id"], json!(ROOT_TEAM_ID), "{team}");
    let signup_token = team["signupToken"].as_str().unwrap_or_default().to_owned();
    assert_eq!(
        signup_token.len(),
        fvoci_server::auth::secret::SECRET_LENGTH,
        "the signup token is a secret and part of the team: {team}"
    );

    th.client
        .post_empty(&format!("/teams/{ROOT_TEAM_ID}/regenerate_signup_token"))
        .await
        .check_ok();
    let regenerated = th
        .client
        .get(&format!("/teams/{ROOT_TEAM_ID}"))
        .await
        .value();
    assert_ne!(
        regenerated["signupToken"],
        json!(signup_token),
        "the token changed"
    );
    let other = th
        .client
        .get(&format!("/teams/{TEST_TEAM_ID}"))
        .await
        .value();
    assert_ne!(
        other["signupToken"], regenerated["signupToken"],
        "each team has a token of its own"
    );
    th.client
        .post_empty(&format!("/teams/{UNKNOWN_ID}/regenerate_signup_token"))
        .await
        .check_error(StatusCode::NOT_FOUND, "team not found");

    th.anonymous()
        .get(&format!("/teams/{ROOT_TEAM_ID}"))
        .await
        .check_unauthorized();
    th.teardown().await;
}

/// `TestGetRootTeam` in `server/app/teams_test.go`: the root team is created once and
/// then returned as it is.
#[tokio::test]
async fn app_get_root_team() {
    let th = Setup::native().start().await;

    let first = th
        .app
        .root_team()
        .await
        .expect("the root team exists after startup");
    let second = th.app.root_team().await.expect("and is returned again");

    assert_eq!(
        first.id, ROOT_TEAM_ID,
        "the root team has the global identifier"
    );
    assert!(!first.signup_token.is_empty(), "it has a signup token");
    assert_eq!(first.modified_by, "", "nobody modified it");
    assert_eq!(
        first.signup_token, second.signup_token,
        "reading does not change the token"
    );
    th.teardown().await;
}

// -- password hashes and session tokens -----------------------------------------------

/// The session token is handed out once, to whoever signed in. What the server keeps is
/// its digest: nothing in the store is something a session could be opened with.
#[tokio::test]
async fn the_store_keeps_the_digest_of_a_session_token_and_not_the_token() {
    let th = Setup::native().start().await.init_basic().await;
    let token = th.client.token.clone();
    let now = fvoci_server::auth::now_millis();
    let store = th.app.store();

    let session = store
        .get_session(&fvoci_server::auth::secret::token_digest(&token), now, 0)
        .await
        .expect("the session is stored under the digest of its token");
    assert_eq!(
        session.user_id,
        th.client.me().await.id,
        "the user's session"
    );
    let by_token = store.get_session(&token, now, 0).await;
    assert!(
        by_token.is_err(),
        "nothing is stored under the token itself: {by_token:?}"
    );
    // And the digest is no token: presenting it does not sign anyone in.
    th.client_with_token(&fvoci_server::auth::secret::token_digest(&token))
        .get("/users/me")
        .await
        .check_unauthorized();

    th.teardown().await;
}

// -- how long a session lasts ---------------------------------------------------------

const MINUTE: i64 = 60 * 1000;
const DAY: i64 = 24 * 60 * MINUTE;

/// The stored session of the harness's first client, whatever its age.
async fn stored_session(th: &TestServer) -> Session {
    th.app
        .store()
        .get_session(
            &fvoci_server::auth::secret::token_digest(&th.client.token),
            0,
            i64::MIN,
        )
        .await
        .expect("the session is stored")
}

/// A session that nobody used for the idle limit has ended, and one that was used a
/// little more recently has not. The limit is `session_idle_days`.
#[tokio::test]
async fn a_session_ends_when_it_was_not_used_for_the_idle_limit() {
    let th = Setup::native()
        .configure(|config| config.session_idle_days = 3)
        .start()
        .await
        .init_basic()
        .await;
    let session = stored_session(&th).await;
    let store = th.app.store();
    let now = fvoci_server::auth::now_millis();

    store
        .touch_session(&session.id, now - 3 * DAY + MINUTE)
        .await
        .expect("the last use is moved back");
    th.client.get("/users/me").await.check_ok();

    store
        .touch_session(&session.id, now - 3 * DAY - MINUTE)
        .await
        .expect("the last use is moved back further");
    th.client.get("/users/me").await.check_unauthorized();
    // The other client's session is its own.
    th.client2.get("/users/me").await.check_ok();

    th.teardown().await;
}

/// A session ends `session_max_days` after it began, however recently it was used.
#[tokio::test]
async fn a_session_ends_at_its_absolute_limit_however_much_it_is_used() {
    let th = Setup::native()
        .configure(|config| config.session_max_days = 7)
        .start()
        .await
        .init_basic()
        .await;
    let session = stored_session(&th).await;
    assert_eq!(
        session.expires_at,
        session.created_at + 7 * DAY,
        "a new session ends seven days after it began"
    );
    assert_eq!(
        session.last_used_at, session.created_at,
        "and counts as used when it began"
    );

    // A session that began long enough ago, used a moment ago.
    let user = th.client.me().await;
    let now = fvoci_server::auth::now_millis();
    let old_token = new_secret();
    let old = Session {
        id: new_id(),
        user_id: user.id,
        created_at: now - 7 * DAY - MINUTE,
        expires_at: now - MINUTE,
        ..Session::default()
    };
    let store = th.app.store();
    store
        .create_session(&old, &fvoci_server::auth::secret::token_digest(&old_token))
        .await
        .expect("the old session is stored");
    store
        .touch_session(&old.id, now)
        .await
        .expect("the old session was just used");

    th.client_with_token(&old_token)
        .get("/users/me")
        .await
        .check_unauthorized();
    th.client.get("/users/me").await.check_ok();

    th.teardown().await;
}

/// When a session was last used is written down at most every ten minutes: requests in
/// between do not write to the database.
#[tokio::test]
async fn the_use_of_a_session_is_recorded_at_most_every_ten_minutes() {
    let th = Setup::native().start().await.init_basic().await;
    let session = stored_session(&th).await;
    let store = th.app.store();

    th.client.get("/users/me").await.check_ok();
    assert_eq!(
        stored_session(&th).await.last_used_at,
        session.last_used_at,
        "a use right after the last recorded one is not recorded"
    );

    let now = fvoci_server::auth::now_millis();
    let nine_minutes_ago = now - 9 * MINUTE;
    store
        .touch_session(&session.id, nine_minutes_ago)
        .await
        .expect("the last use is moved back");
    th.client.get("/users/me").await.check_ok();
    assert_eq!(
        stored_session(&th).await.last_used_at,
        nine_minutes_ago,
        "nor is a use nine minutes after it"
    );

    store
        .touch_session(&session.id, now - 11 * MINUTE)
        .await
        .expect("the last use is moved back further");
    th.client.get("/users/me").await.check_ok();
    let recorded = stored_session(&th).await.last_used_at;
    assert!(
        recorded >= now,
        "a use eleven minutes after it is: {recorded} against {now}"
    );

    th.teardown().await;
}

/// A password that is right, but stored with other hashing parameters than the current
/// ones, is hashed anew at login: the parameters can be raised without asking anyone to
/// set their password again.
#[tokio::test]
async fn a_login_renews_a_hash_made_with_other_parameters() {
    let th = Setup::native().start().await.init_basic().await;
    let store = th.app.store();
    let user = store
        .get_user_by_username(crate::common::USER1_USERNAME)
        .await
        .expect("the user is stored");

    // A hash as an earlier version with cheaper parameters would have stored it.
    let cheap = argon2_params(8, 1);
    let old_hash = fvoci_server::auth::password::hash_password_with(PASSWORD, cheap)
        .expect("a hash with other parameters");
    assert!(
        old_hash.starts_with("$argon2id$v=19$m=8,t=1,p=1$"),
        "{old_hash}"
    );
    assert!(
        store
            .replace_password_hash(&user.id, &user.password_hash, &old_hash)
            .await
            .expect("the old hash is planted"),
        "planted"
    );

    // A wrong password changes nothing.
    th.anonymous()
        .login(crate::common::USER1_USERNAME, "not the password")
        .await
        .check_unauthorized();
    let stored = store.get_user_by_id(&user.id).await.expect("the user");
    assert_eq!(
        stored.password_hash, old_hash,
        "a failed login renews nothing"
    );

    // The right one signs in and leaves a current hash behind.
    th.login(crate::common::USER1_USERNAME, PASSWORD).await;
    let stored = store.get_user_by_id(&user.id).await.expect("the user");
    assert!(
        stored
            .password_hash
            .starts_with("$argon2id$v=19$m=19456,t=2,p=1$"),
        "the hash was renewed with the current parameters: {}",
        stored.password_hash
    );
    // The session that was open stays: the password did not change.
    th.client.get("/users/me").await.check_ok();
    // And the password still is the password.
    th.login(crate::common::USER1_USERNAME, PASSWORD).await;
    let again = store.get_user_by_id(&user.id).await.expect("the user");
    assert_eq!(
        again.password_hash, stored.password_hash,
        "a current hash is left alone"
    );

    th.teardown().await;
}

/// Argon2 parameters with the given memory (KiB) and iterations.
fn argon2_params(memory_kib: u32, iterations: u32) -> argon2::Params {
    argon2::Params::new(memory_kib, iterations, 1, None).expect("valid parameters")
}

/// A user who changes their password signs every other device out: whoever else knew
/// the old password does not stay signed in. The session the change came through stays.
#[tokio::test]
async fn changing_the_password_ends_the_other_sessions_of_the_user() {
    let th = Setup::native().start().await.init_basic().await;
    let me = th.client.me().await;
    let here = &th.client;
    let phone = th.login(crate::common::USER1_USERNAME, PASSWORD).await;
    let tablet = th.login(crate::common::USER1_USERNAME, PASSWORD).await;
    for client in [here, &phone, &tablet] {
        client.get("/users/me").await.check_ok();
    }

    // A refused change ends nothing.
    here.post(
        &format!("/users/{}/changepassword", me.id),
        &json!({"oldPassword": "not the password", "newPassword": "another password"}),
    )
    .await
    .check_bad_request();
    phone.get("/users/me").await.check_ok();

    here.post(
        &format!("/users/{}/changepassword", me.id),
        &json!({"oldPassword": PASSWORD, "newPassword": "another password"}),
    )
    .await
    .check_ok();

    here.get("/users/me").await.check_ok();
    for (client, device) in [(&phone, "phone"), (&tablet, "tablet")] {
        let response = client.get("/users/me").await;
        assert_eq!(
            response.status,
            StatusCode::UNAUTHORIZED,
            "the session on the {device} ended with the old password"
        );
    }
    // The other user is not signed out by it.
    th.client2.get("/users/me").await.check_ok();
    // The old password opens nothing any more; the new one does.
    th.anonymous()
        .login(crate::common::USER1_USERNAME, PASSWORD)
        .await
        .check_unauthorized();
    th.login(crate::common::USER1_USERNAME, "another password")
        .await;

    th.teardown().await;
}

/// A password is hashed whole, whatever its length up to the limit, and a password
/// longer than the limit is refused where it is set and wrong where it is tried.
#[tokio::test]
async fn passwords_are_bounded_by_characters_below_and_by_bytes_above() {
    let th = Setup::native().start().await.init_basic().await;
    let team = th.app.root_team().await.expect("the root team");
    let register = |username: &str, password: &str| {
        let body = json!({
            "username": username,
            "email": format!("{username}@sample.com"),
            "password": password,
            "token": team.signup_token,
        });
        let anonymous = th.anonymous();
        async move { anonymous.post("/register", &body).await }
    };

    // Longer than the 72 bytes the previous hash looked at; every byte counts.
    let long = "p".repeat(200);
    register("long", &long).await.check_ok();
    th.login("long", &long).await;
    let mut other_end = "p".repeat(199);
    other_end.push('q');
    th.anonymous()
        .login("long", &other_end)
        .await
        .check_unauthorized();

    // The limits, where a password is set.
    let longest = "가".repeat(341);
    register("longest", &longest).await.check_ok();
    th.login("longest", &longest).await;
    for (username, password, message) in [
        (
            "toolong",
            "x".repeat(1025),
            "password must be at most 1024 bytes",
        ),
        (
            "short",
            "가나다라".to_owned(),
            "password must be at least 8 characters",
        ),
    ] {
        let response = register(username, &password).await;
        response.check_error(StatusCode::BAD_REQUEST, message);
    }

    // A password longer than the limit was never set: trying it is a wrong password
    // like any other, with the same answer.
    let response = th.anonymous().login("long", &"x".repeat(10_000)).await;
    response.check_error(StatusCode::UNAUTHORIZED, "incorrect login");
    assert_eq!(response.error_reason(), "invalid_credentials");

    th.teardown().await;
}
