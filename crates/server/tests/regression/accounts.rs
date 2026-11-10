//! Scenarios: system routes, registration and sessions, teams and users.
#![expect(
    clippy::too_many_lines,
    reason = "a scenario is one request sequence; cutting it up would hide the order it pins"
)]

use serde_json::{Value, json};

use crate::common::{ROOT_TEAM_ID, UNKNOWN_ID};
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
    // The prefix of the original server's API names nothing: a client written for it
    // is told so, with or without a session, and is not handed the web app's page.
    s.get("old prefix: client config", "/api/v2/clientConfig")
        .await;
    s.post("old prefix: login", "/api/v2/login")
        .json(json!({"type": "normal", "username": "nobody", "password": "nothing"}))
        .await;
    s.get("old prefix: the prefix itself", "/api/v2").await;
    s.get("the api root", "/api").await;
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
        .session_token();
    s.post("change password: wrong old password", &change_password)
        .json(json!({"oldPassword": "nope", "newPassword": "new-password"}))
        .user(&alice)
        .await;
    s.post("change password: short new password", &change_password)
        .json(json!({"oldPassword": "test-password", "newPassword": "short"}))
        .user(&alice)
        .await;
    let before_the_change = s.session_token(&alice);
    let new_token = s
        .post("change password", &change_password)
        .json(json!({"oldPassword": "test-password", "newPassword": "new-password"}))
        .user(&alice)
        .keep_headers(&["Set-Cookie"])
        .await
        .session_token();
    // The session the password was changed through goes on under a new token.
    s.set_session_token(&alice, new_token);
    s.get(
        "me with the token from before the change",
        &format!("{API}/users/me"),
    )
    .token(&before_the_change)
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

/// Administrators of the instance. The harness's first user, `root`, is the one the
/// instance starts with.
pub(crate) async fn admins(s: &mut Session) {
    let [alice, bob] = s.users("admins", ["alice", "bob"]).await;
    let alice_id = s.user_id(&alice).await;
    let bob_id = s.user_id(&bob).await;
    let root_id = s.user_id("root").await;
    let old_token = s.signup_token("root").await;
    let team = format!("{API}/teams/{ROOT_TEAM_ID}");
    let regenerate = format!("{team}/regenerate_signup_token");
    let admin_of = |user_id: &str| format!("{API}/users/{user_id}/admin");

    s.get("me: the first user", &format!("{API}/users/me"))
        .user("root")
        .await;
    s.get("me: a later user", &format!("{API}/users/me"))
        .user(&alice)
        .await;
    s.get(
        "the first user, seen by another",
        &format!("{API}/users/{root_id}"),
    )
    .user(&alice)
    .await;

    // -- the signup token is for administrators --------------------------------------
    s.get("team: administrator", &team).user("root").await;
    s.get("team: anybody else", &team).user(&alice).await;
    s.get("teams: administrator", &format!("{API}/teams"))
        .user("root")
        .await;
    s.get("teams: anybody else", &format!("{API}/teams"))
        .user(&alice)
        .await;
    s.post("renew the token: not an administrator", &regenerate)
        .user(&alice)
        .await;
    s.post("renew the token: no session", &regenerate).await;
    s.post(
        "renew the token: unknown team",
        &format!("{API}/teams/{UNKNOWN_ID}/regenerate_signup_token"),
    )
    .user("root")
    .await;
    s.post("renew the token", &regenerate).user("root").await;
    s.post("register with the old token", &format!("{API}/register"))
        .json(json!({
            "username": "admins-late", "email": "admins-late@example.com",
            "password": "test-password", "token": old_token,
        }))
        .await;
    s.post("register without a token", &format!("{API}/register"))
        .json(json!({
            "username": "admins-late", "email": "admins-late@example.com",
            "password": "test-password",
        }))
        .await;

    // -- making and unmaking administrators ------------------------------------------
    s.put("make: not an administrator", &admin_of(&bob_id))
        .json(json!({"isAdmin": true}))
        .user(&alice)
        .await;
    s.put("make: oneself, not an administrator", &admin_of(&alice_id))
        .json(json!({"isAdmin": true}))
        .user(&alice)
        .await;
    s.put("make: no session", &admin_of(&bob_id))
        .json(json!({"isAdmin": true}))
        .await;
    s.put("make: no body member", &admin_of(&bob_id))
        .json(json!({}))
        .user("root")
        .await;
    s.put("make: not a boolean", &admin_of(&bob_id))
        .json(json!({"isAdmin": "yes"}))
        .user("root")
        .await;
    s.put("make: unknown user", &admin_of(UNKNOWN_ID))
        .json(json!({"isAdmin": true}))
        .user("root")
        .await;
    s.put("make: no identifier", &admin_of("nobody"))
        .json(json!({"isAdmin": true}))
        .user("root")
        .await;
    s.put("step down: the only administrator", &admin_of(&root_id))
        .json(json!({"isAdmin": false}))
        .user("root")
        .await;
    s.put("make", &admin_of(&bob_id))
        .json(json!({"isAdmin": true}))
        .user("root")
        .await;
    s.put("make: again", &admin_of(&bob_id))
        .json(json!({"isAdmin": true}))
        .user("root")
        .await;
    s.get("team: the new administrator", &team).user(&bob).await;
    s.put("step down", &admin_of(&root_id))
        .json(json!({"isAdmin": false}))
        .user("root")
        .await;
    s.put("make: after stepping down", &admin_of(&root_id))
        .json(json!({"isAdmin": true}))
        .user("root")
        .await;
    s.get("team: after stepping down", &team).user("root").await;
    s.put("unmake: the new only administrator", &admin_of(&bob_id))
        .json(json!({"isAdmin": false}))
        .user(&bob)
        .await;
    s.post("renew the token: the new administrator", &regenerate)
        .user(&bob)
        .await;
}

/// What a user has set about how things are shown to them, and the bag of client state
/// next to it.
pub(crate) async fn settings(s: &mut Session) {
    let [alice, bob] = s.users("settings", ["alice", "bob"]).await;
    let alice_id = s.user_id(&alice).await;
    let path = format!("{API}/users/me/settings");
    let bag = format!("{API}/users/{alice_id}/config");

    s.get("get: defaults", &path).user(&alice).await;
    s.get("get: no session", &path).await;
    s.patch("patch: every setting", &path)
        .json(json!({
            "timeZone": "Asia/Seoul", "weekStart": "monday", "hourCycle": "h23",
            "theme": "dark", "cardOpen": "side", "density": "compact", "haptics": false,
        }))
        .user(&alice)
        .await;
    s.get("get: as set", &path).user(&alice).await;
    s.get("get: another user", &path).user(&bob).await;
    s.patch("patch: one setting, two back to their defaults", &path)
        .json(json!({"cardOpen": "full", "theme": null, "haptics": null}))
        .user(&alice)
        .await;
    s.patch("patch: nothing", &path)
        .json(json!({}))
        .user(&alice)
        .await;
    s.patch("patch: an old name of a time zone", &path)
        .json(json!({"timeZone": "Asia/Calcutta"}))
        .user(&alice)
        .await;

    let refused: [(&str, Value); 12] = [
        (
            "no such time zone",
            json!({"timeZone": "Mars/Olympus_Mons"}),
        ),
        (
            "a time zone in other letters",
            json!({"timeZone": "asia/seoul"}),
        ),
        ("no such week start", json!({"weekStart": "tuesday"})),
        ("no such hour cycle", json!({"hourCycle": "h24"})),
        ("no such theme", json!({"theme": "sepia"})),
        ("no such way to open a card", json!({"cardOpen": "auto"})),
        ("no such density", json!({"density": "cozy"})),
        ("haptics as a word", json!({"haptics": "true"})),
        ("a number", json!({"theme": 3})),
        ("no such setting", json!({"locale": "ko"})),
        (
            "one valid, one not",
            json!({"density": "comfortable", "theme": "sepia"}),
        ),
        ("not an object", json!(["theme"])),
    ];
    for (name, body) in refused {
        s.patch(&format!("patch: {name}"), &path)
            .json(body)
            .user(&alice)
            .await;
    }
    s.patch("patch: no session", &path)
        .json(json!({"theme": "dark"}))
        .await;
    s.get("get: after the refusals", &path).user(&alice).await;

    // The bag of client state: names and values the server does not know, within bounds.
    s.put("client state: set", &bag)
        .json(json!({"updatedFields": {"theme": "of-the-client", "tour.step": "2"}}))
        .user(&alice)
        .await;
    s.get("client state: get", &format!("{API}/users/me/config"))
        .user(&alice)
        .await;
    s.get("get: untouched by the client state", &path)
        .user(&alice)
        .await;
    s.put("client state: a name with a space", &bag)
        .json(json!({"updatedFields": {"with space": "1"}}))
        .user(&alice)
        .await;
    s.put("client state: a name that is too long", &bag)
        .json(json!({"updatedFields": {"n".repeat(65): "1"}}))
        .user(&alice)
        .await;
    s.put("client state: a value that is too long", &bag)
        .json(json!({"updatedFields": {"big": "v".repeat(4097)}}))
        .user(&alice)
        .await;
    let many: serde_json::Map<String, Value> = (0..99)
        .map(|index| (format!("entry-{index:02}"), json!("v")))
        .collect();
    s.put("client state: more entries than the bag holds", &bag)
        .json(json!({"updatedFields": many}))
        .user(&alice)
        .await;
    s.put("client state: remove", &bag)
        .json(json!({"deletedFields": ["theme"]}))
        .user(&alice)
        .await;
}

/// How a session reaches the server and what keeps another site's page from using it:
/// the cookie of a login, the request-forgery header, the origin of a request.
pub(crate) async fn cookies(s: &mut Session) {
    let [alice] = s.users("cookies", ["alice"]).await;
    let login = format!("{API}/login");
    let me = format!("{API}/users/me");
    let boards = format!("{API}/boards");
    let credentials = json!({"type": "normal", "username": alice, "password": "test-password"});
    let board = json!({"teamId": ROOT_TEAM_ID, "type": "P", "title": "made"});
    // The address the test servers take for their own: the default `serverRoot`.
    let own_origin = "http://localhost:8000";
    let foreign_origin = "https://evil.example";

    // -- signing in -----------------------------------------------------------------
    let token = s
        .post("login: the cookie and an empty body", &login)
        .json(credentials.clone())
        .keep_headers(&["Set-Cookie"])
        .await
        .session_token();
    s.post("login: wrong password sets no cookie", &login)
        .json(json!({"type": "normal", "username": alice, "password": "nope"}))
        .keep_headers(&["Set-Cookie"])
        .await;
    s.post("login: without the header", &login)
        .json(credentials.clone())
        .without_csrf_header()
        .await;
    s.post("login: from another origin", &login)
        .json(credentials.clone())
        .header("Origin", foreign_origin)
        .await;
    s.post("login: from the own origin", &login)
        .json(credentials.clone())
        .header("Origin", own_origin)
        .unrecorded()
        .await;

    // -- reading --------------------------------------------------------------------
    s.get("read: cookie, no header", &me)
        .token(&token)
        .in_cookie()
        .without_csrf_header()
        .await;
    s.get("read: bearer, no header", &me)
        .token(&token)
        .without_csrf_header()
        .await;
    s.get("read: cookie, another origin named", &me)
        .token(&token)
        .in_cookie()
        .header("Origin", foreign_origin)
        .await;
    s.get(
        "read: the token in the address",
        &format!("{me}?access_token={token}"),
    )
    .await;
    s.get("read: a cookie that is no session's", &me)
        .token("not-a-session")
        .in_cookie()
        .await;
    s.get("read: the cookie of another name", &me)
        .header("Cookie", &format!("FVOCIAUTHTOKEN={token}"))
        .await;

    // -- changing -------------------------------------------------------------------
    s.post("change: cookie, no header", &boards)
        .json(board.clone())
        .token(&token)
        .in_cookie()
        .without_csrf_header()
        .await;
    s.post("change: cookie and header", &boards)
        .json(board.clone())
        .token(&token)
        .in_cookie()
        .await;
    s.post("change: bearer, no header", &boards)
        .json(board.clone())
        .token(&token)
        .without_csrf_header()
        .await;
    s.post("change: cookie and header, the own origin", &boards)
        .json(board.clone())
        .token(&token)
        .in_cookie()
        .header("Origin", own_origin)
        .await;
    s.post("change: cookie and header, another origin", &boards)
        .json(board.clone())
        .token(&token)
        .in_cookie()
        .header("Origin", foreign_origin)
        .await;
    s.post("change: bearer, another origin", &boards)
        .json(board.clone())
        .token(&token)
        .header("Origin", foreign_origin)
        .await;
    s.post("change: cookie and header, no origin to speak of", &boards)
        .json(board.clone())
        .token(&token)
        .in_cookie()
        .header("Origin", "null")
        .await;
    s.post(
        "change: bearer and cookie, the bearer is no session's",
        &boards,
    )
    .json(board)
    .token("not-a-session")
    .header("Cookie", &format!("fvoci_session={token}"))
    .await;

    // -- signing out ----------------------------------------------------------------
    s.post("logout: cookie, no header", &format!("{API}/logout"))
        .token(&token)
        .in_cookie()
        .without_csrf_header()
        .await;
    s.post("logout: clears the cookie", &format!("{API}/logout"))
        .token(&token)
        .in_cookie()
        .keep_headers(&["Set-Cookie"])
        .await;
    s.get("read: after the logout", &me)
        .token(&token)
        .in_cookie()
        .await;
}

/// One attempt too many: an address that keeps failing at an account is told to wait,
/// with the same words whether the account exists.
pub(crate) async fn login_limits(s: &mut Session) {
    let [alice, bob] = s.users("limits", ["alice", "bob"]).await;
    let login = format!("{API}/login");
    let attempt = |username: &str, password: &str| json!({"type": "normal", "username": username, "password": password});

    for account in [alice.as_str(), "limits-nobody"] {
        // Five failures are free; the sixth is refused like them and starts the wait.
        for number in 1..=6 {
            s.post(&format!("{account}: wrong password {number}"), &login)
                .json(attempt(account, "not-the-password"))
                .await;
        }
        s.post(&format!("{account}: one too many"), &login)
            .json(attempt(account, "not-the-password"))
            .keep_headers(&["Retry-After"])
            .await;
    }
    s.post("the right password waits too", &login)
        .json(attempt(&alice, "test-password"))
        .keep_headers(&["Retry-After"])
        .await;
    s.post("the account in other letters is the same account", &login)
        .json(attempt(&alice.to_uppercase(), "test-password"))
        .await;
    s.post("another account is not held up", &login)
        .json(attempt(&bob, "test-password"))
        .await;
    s.get("a session from before goes on", &format!("{API}/users/me"))
        .user(&alice)
        .await;
}
