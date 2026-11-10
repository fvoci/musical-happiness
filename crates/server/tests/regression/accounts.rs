//! Scenarios: system routes, registration and sessions, teams and users.
#![expect(
    clippy::too_many_lines,
    reason = "a scenario is one request sequence; cutting it up would hide the order it pins"
)]

use serde_json::{Value, json};

use crate::common::ROOT_TEAM_ID;
use crate::harness::{API, Session};

pub(crate) async fn system(s: &mut Session) {
    s.get("hello", "/hello").without_csrf_header().await;
    let ping = s
        .get("ping", "/ping")
        .without_csrf_header()
        .unrecorded()
        .await;
    let ping = ping.json();
    let mut members: Vec<&String> = ping
        .as_object()
        .expect("ping answers an object")
        .keys()
        .collect();
    members.sort();
    s.note("ping members", &json!(members));
    let db_type = ping["db_type"].as_str().unwrap_or_default();
    assert!(
        ["sqlite3", "postgres"].contains(&db_type),
        "ping names the database kind, got {db_type:?}"
    );
    // The database kind is the one thing that differs between the two test runs.
    s.note(
        "ping version and sku",
        &json!([ping["version"], ping["sku"], "<db>"]),
    );
    s.get("client config", &format!("{API}/clientConfig")).await;
    s.get(
        "client config without csrf header",
        &format!("{API}/clientConfig"),
    )
    .without_csrf_header()
    .await;
    s.get("unknown api path", &format!("{API}/no-such-route"))
        .await;
    s.delete(
        "wrong method on a known path",
        &format!("{API}/clientConfig"),
    )
    .await;
    s.get("index for an app route", "/board/abc")
        .without_csrf_header()
        .keep_headers(&["Content-Type"])
        .await;
    s.get("static without slash", "/static")
        .without_csrf_header()
        .await;
    s.get("missing static file", "/static/nope.js")
        .without_csrf_header()
        .await;
}

pub(crate) async fn auth(s: &mut Session) {
    let [alice] = s.users("auth", ["alice"]).await;
    let token = s.signup_token("root").await;
    let register = format!("{API}/register");
    let login = format!("{API}/login");

    s.post("register without token", &register)
        .json(json!({"username": "auth-x", "email": "auth-x@example.com", "password": "test-password"}))
        .await;
    s.post("register with a wrong token", &register)
        .json(json!({
            "username": "auth-x", "email": "auth-x@example.com", "password": "test-password", "token": "nope",
        }))
        .await;
    let refused: [(&str, Value); 8] = [
        (
            "no username",
            json!({"username": "", "email": "a@example.com", "password": "test-password"}),
        ),
        (
            "no email",
            json!({"username": "auth-y", "email": "", "password": "test-password"}),
        ),
        (
            "bad email",
            json!({"username": "auth-y", "email": "not-an-email", "password": "test-password"}),
        ),
        (
            "no password",
            json!({"username": "auth-y", "email": "auth-y@example.com", "password": ""}),
        ),
        (
            "short password",
            json!({"username": "auth-y", "email": "auth-y@example.com", "password": "1234567"}),
        ),
        (
            "long password",
            json!({"username": "auth-y", "email": "auth-y@example.com", "password": "x".repeat(1025)}),
        ),
        (
            "taken username",
            json!({"username": alice, "email": "other@example.com", "password": "test-password"}),
        ),
        (
            "taken email",
            json!({"username": "auth-z", "email": format!("{alice}@example.com"), "password": "test-password"}),
        ),
    ];
    for (name, mut body) in refused {
        body["token"] = json!(token);
        s.post(&format!("register: {name}"), &register)
            .json(body)
            .await;
    }

    s.post("login", &login)
        .json(json!({"type": "normal", "username": alice, "password": "test-password"}))
        .await;
    s.post("login by email", &login)
        .json(json!({"type": "normal", "email": format!("{alice}@example.com"), "password": "test-password"}))
        .await;
    s.post("login: wrong password", &login)
        .json(json!({"type": "normal", "username": alice, "password": "nope"}))
        .await;
    s.post("login: unknown user", &login)
        .json(json!({"type": "normal", "username": "nobody", "password": "x"}))
        .await;
    s.post("login: wrong type", &login)
        .json(json!({"type": "sso", "username": alice, "password": "test-password"}))
        .await;
    s.post("login: empty body", &login).json(json!({})).await;

    s.get("me", &format!("{API}/users/me")).user(&alice).await;
    s.get("me without session", &format!("{API}/users/me"))
        .await;
    s.get("me with a bad token", &format!("{API}/users/me"))
        .token("not-a-token")
        .await;
    s.get("my memberships", &format!("{API}/users/me/memberships"))
        .user(&alice)
        .await;
    s.get("my config", &format!("{API}/users/me/config"))
        .user(&alice)
        .await;
    let alice_id = s.user_id(&alice).await;
    s.get("user by id", &format!("{API}/users/{alice_id}"))
        .user(&alice)
        .await;
    s.get("unknown user", &format!("{API}/users/unknown-user"))
        .user(&alice)
        .await;
    s.post("users by id", &format!("{API}/users"))
        .json(json!([alice_id]))
        .user(&alice)
        .await;
    s.post("users by id: empty list", &format!("{API}/users"))
        .json(json!([]))
        .user(&alice)
        .await;
    s.put(
        "update my config",
        &format!("{API}/users/{alice_id}/config"),
    )
    .json(
        json!({"updatedFields": {"welcomePageViewed": "1", "theme": "dark"}, "deletedFields": []}),
    )
    .user(&alice)
    .await;
    s.put(
        "delete a config entry",
        &format!("{API}/users/{alice_id}/config"),
    )
    .json(json!({"updatedFields": {}, "deletedFields": ["theme"]}))
    .user(&alice)
    .await;
    s.get("my config afterwards", &format!("{API}/users/me/config"))
        .user(&alice)
        .await;
    let root_id = s.user_id("root").await;
    s.put(
        "update another user's config",
        &format!("{API}/users/{root_id}/config"),
    )
    .json(json!({"updatedFields": {"x": "1"}, "deletedFields": []}))
    .user(&alice)
    .await;

    let change_password = format!("{API}/users/{alice_id}/changepassword");
    // A second session of the same user, as on another device, opened before the change.
    let other_device = s
        .post("login on another device", &login)
        .json(json!({"type": "normal", "username": alice, "password": "test-password"}))
        .await
        .string("token");
    s.post("change password: wrong old password", &change_password)
        .json(json!({"oldPassword": "nope", "newPassword": "new-password"}))
        .user(&alice)
        .await;
    s.post("change password: short new password", &change_password)
        .json(json!({"oldPassword": "test-password", "newPassword": "short"}))
        .user(&alice)
        .await;
    s.post("change password", &change_password)
        .json(json!({"oldPassword": "test-password", "newPassword": "new-password"}))
        .user(&alice)
        .await;
    s.get(
        "me on the other device after the change",
        &format!("{API}/users/me"),
    )
    .token(&other_device)
    .await;
    s.get(
        "me with the session that changed the password",
        &format!("{API}/users/me"),
    )
    .user(&alice)
    .await;
    s.post("login with the new password", &login)
        .json(json!({"type": "normal", "username": alice, "password": "new-password"}))
        .await;
    s.post("login with the old password", &login)
        .json(json!({"type": "normal", "username": alice, "password": "test-password"}))
        .await;

    s.post("logout", &format!("{API}/logout"))
        .user(&alice)
        .await;
    s.get("me after logout", &format!("{API}/users/me"))
        .user(&alice)
        .await;
}

pub(crate) async fn teams(s: &mut Session) {
    let [alice] = s.users("teams", ["alice"]).await;
    s.get("teams", &format!("{API}/teams")).user(&alice).await;
    s.get("team 0", &format!("{API}/teams/{ROOT_TEAM_ID}"))
        .user(&alice)
        .await;
    s.get("some other team id", &format!("{API}/teams/anything"))
        .user(&alice)
        .await;
    s.get(
        "team without session",
        &format!("{API}/teams/{ROOT_TEAM_ID}"),
    )
    .await;
    let users = s
        .get("team users", &format!("{API}/teams/{ROOT_TEAM_ID}/users"))
        .user(&alice)
        .unrecorded()
        .await;
    let includes_me = users
        .json()
        .as_array()
        .expect("the team users are a list")
        .iter()
        .any(|user| user["username"] == alice.as_str());
    s.note("team users include me", &json!(includes_me));
    s.get(
        "team users search",
        &format!("{API}/teams/{ROOT_TEAM_ID}/users?search=teams-ali"),
    )
    .user(&alice)
    .await;
    s.get(
        "team users search without match",
        &format!("{API}/teams/{ROOT_TEAM_ID}/users?search=zzzz-no-such"),
    )
    .user(&alice)
    .await;
    let alice_id = s.user_id(&alice).await;
    s.post(
        "team users by id",
        &format!("{API}/teams/{ROOT_TEAM_ID}/users"),
    )
    .json(json!([alice_id]))
    .user(&alice)
    .await;
    s.post(
        "team users by id: empty",
        &format!("{API}/teams/{ROOT_TEAM_ID}/users"),
    )
    .json(json!([]))
    .user(&alice)
    .await;
}
