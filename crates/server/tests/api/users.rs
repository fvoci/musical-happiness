//! Ports of `server/integrationtests/user_test.go` and of the user and login cases of
//! `server/app/auth_test.go`, plus tests of what the port changes on purpose.
//!
//! Not ported here: `TestGetUserList` (runs in the host-application mode this server does
//! not have) and `TestTeamUploadFile` (in the `files` module).

use fvoci_model::{ChangePasswordRequest, IdKind, LoginRequest, RegisterRequest, User, new_id};
use reqwest::StatusCode;
use serde_json::json;

use crate::common::{PASSWORD, Setup};

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
    let request = register_request(&new_id(IdKind::None));

    th.client.post("/register", &request).await.check_ok();

    // The same user a second time: the first user exists, so a signup token is needed.
    let again = th.client.post("/register", &request).await;
    again.check_error(StatusCode::FORBIDDEN, "registration needs the signup token");
    assert_eq!(again.error_reason(), "signup_token_required");

    // With the token, the name is found to be taken.
    let mut with_token = register_request(&new_id(IdKind::None));
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
    let request = login_request("nonexistuser", "", &new_id(IdKind::None));

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
    let password = new_id(IdKind::None);
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
        27,
        "the session token is a generated identifier"
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
    assert_eq!(
        user.auth_service, "native",
        "the user belongs to native authentication"
    );
    assert!(
        user.password.starts_with("$2"),
        "the password is stored hashed"
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
    let password = new_id(IdKind::None);
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
        .register_and_login(FAKE_USERNAME, FAKE_EMAIL, &new_id(IdKind::None), "")
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

    let team_users: Vec<User> = th.client.get("/teams/0/users").await.ok_json();
    assert_eq!(team_users.len(), 2, "the team has both users");
    let found: Vec<User> = th.client.get("/teams/0/users?search=user2").await.ok_json();
    assert_eq!(found.len(), 1, "the search narrows the list");
    assert_eq!(found[0].id, user2.id, "to the user that matches");

    let by_id: Vec<User> = th
        .client
        .post("/teams/0/users", &[&user2.id])
        .await
        .ok_json();
    assert_eq!(by_id.len(), 1, "one user was asked for");

    th.teardown().await;
}

/// `TestUserChangePassword`
#[tokio::test]
async fn user_change_password() {
    let th = Setup::native().start().await;
    let password = new_id(IdKind::None);
    let client = th
        .register_and_login(FAKE_USERNAME, FAKE_EMAIL, &password, "")
        .await;
    let me = client.me().await;
    let new_password = new_id(IdKind::None);

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
    let me = th.client.me().await;

    assert!(
        th.app.change_password("", "", "").await.is_err(),
        "fail, missing login information"
    );
    assert!(
        th.app.change_password("badID", "", "").await.is_err(),
        "fail, invalid userId"
    );
    assert!(
        th.app
            .change_password(&me.id, "wrongPassword", "newPassword")
            .await
            .is_err(),
        "fail, invalid password"
    );
    th.app
        .change_password(&me.id, PASSWORD, "newPassword")
        .await
        .expect("success, using username");

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

/// The team routes of a standalone server.
#[tokio::test]
async fn teams() {
    let th = Setup::native().start().await.init_basic().await;

    let teams = th.client.get("/teams").await.value();
    assert_eq!(
        teams.as_array().map(Vec::len),
        Some(1),
        "there is one team: {teams}"
    );
    assert_eq!(teams[0]["id"], json!("0"), "the root team: {teams}");

    // Preserved quirk: any team identifier gets the root team.
    let team = th.client.get("/teams/some-other-team").await.value();
    assert_eq!(team["id"], json!("0"), "{team}");
    let signup_token = team["signupToken"].as_str().unwrap_or_default().to_owned();
    assert!(
        !signup_token.is_empty(),
        "the signup token is part of the team: {team}"
    );

    th.client
        .post_empty("/teams/0/regenerate_signup_token")
        .await
        .check_ok();
    let regenerated = th.client.get("/teams/0").await.value();
    assert_ne!(
        regenerated["signupToken"],
        json!(signup_token),
        "the token changed"
    );

    th.anonymous().get("/teams/0").await.check_unauthorized();
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

    assert_eq!(first.id, "0", "the root team has the global identifier");
    assert!(!first.signup_token.is_empty(), "it has a signup token");
    assert_eq!(first.modified_by, "", "nobody modified it");
    assert_eq!(
        first.signup_token, second.signup_token,
        "reading does not change the token"
    );
    th.teardown().await;
}

/// In single-user mode there are no accounts: the token is the one credential, and the
/// account routes refuse.
#[tokio::test]
async fn single_user_mode() {
    let th = Setup::single_user().start().await;

    let me = th.client.me().await;
    assert_eq!(me.id, "single-user", "the one user");
    assert_eq!(me.username, "single-user", "is named after the mode");

    // The one user has no record in the database; every route that names a user
    // knows that user all the same.
    let users: Vec<User> = th.client.post("/users", &["single-user"]).await.ok_json();
    assert_eq!(users.len(), 1, "the users list knows the one user");
    assert_eq!(users[0].id, "single-user", "by the mode's name");
    let user: User = th.client.get("/users/single-user").await.ok_json();
    assert_eq!(user.id, "single-user", "and so does the profile route");
    let users: Vec<User> = th
        .client
        .post("/teams/0/users", &["single-user"])
        .await
        .ok_json();
    assert_eq!(users.len(), 1, "the team route knows the one user");
    assert_eq!(users[0].id, "single-user", "by the mode's name");
    assert_eq!(
        th.client.get("/teams/0/users").await.value(),
        json!([]),
        "no user is stored"
    );
    assert_eq!(
        th.client.get("/users/me/memberships").await.value(),
        json!([]),
        "no boards yet"
    );
    th.client
        .post_empty("/teams/0/regenerate_signup_token")
        .await
        .check_ok();

    let anonymous = th.anonymous();
    let response = anonymous.get("/users/me").await;
    response.check_unauthorized();
    assert_eq!(
        response.error_message(),
        "invalid single user token",
        "no token"
    );
    th.client_with_token("WRONG")
        .get("/users/me")
        .await
        .check_unauthorized();

    // There are no accounts to sign in to, register, sign out of or change.
    let refused = [
        th.client.post("/login", &json!({})).await,
        th.client.post("/register", &json!({})).await,
        th.client.post_empty("/logout").await,
        th.client
            .post(
                "/users/single-user/changepassword",
                &json!({"oldPassword": "a", "newPassword": "b"}),
            )
            .await,
    ];
    for response in &refused {
        response.check_error(StatusCode::FORBIDDEN, "not permitted in single-user mode");
        assert_eq!(response.error_reason(), "single_user_mode");
    }

    th.teardown().await;
}
