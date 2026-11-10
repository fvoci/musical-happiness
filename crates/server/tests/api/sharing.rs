//! Ports of `server/integrationtests/sharing_test.go` and `server/app/sharing_test.go`,
//! plus tests of the sharing routes that the original has none for: who may share, what
//! is taken from the body, and how the body is decoded.
//!
//! The original's `TestSharing` runs its subtests on one server and turns public sharing
//! on halfway. A server's configuration is fixed here, so each subtest has a server of
//! its own with the configuration it needs.
//!
//! Not ported: `TestGetSharing/"should fail to get a sharing"` and
//! `TestUpsertSharing/"should fail to upsert a sharing"` make a mocked store fail and
//! check that the error is passed on. A real store has no failure to provoke.

use fvoci_model::{Board, BoardMember, BoardType, IdKind, Sharing, new_id};
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

use crate::common::{ApiResponse, Client, Setup, TestServer};

/// Puts a board into the store without going through the board routes, with `user_id` as
/// its administrator.
async fn board_with_admin(th: &TestServer, user_id: &str, board_type: BoardType) -> Board {
    let mut board = Board {
        id: new_id(IdKind::Board),
        team_id: "0".to_owned(),
        board_type,
        ..Board::default()
    };
    th.app
        .store()
        .insert_board_with_admin(&mut board, user_id)
        .await
        .expect("the board is inserted");
    board
}

fn sharing_path(board_id: &str) -> String {
    format!("/boards/{board_id}/sharing")
}

/// `PostSharing`
async fn post_sharing(client: &Client, board_id: &str, sharing: &Sharing) -> ApiResponse {
    client.post(&sharing_path(board_id), sharing).await
}

fn sharing_request(board_id: &str, token: &str) -> Sharing {
    Sharing {
        id: board_id.to_owned(),
        enabled: true,
        token: token.to_owned(),
        modified_by: String::new(),
        update_at: 1,
    }
}

#[track_caller]
fn check_error(response: &ApiResponse, status: StatusCode, message: &str) {
    response.check_error(status, message);
}

/// The answer for a board that has no sharing record.
#[track_caller]
fn check_no_sharing(response: &ApiResponse) {
    // A board that was never shared answers as one whose sharing is off.
    response.check_ok();
    let sharing = response.value();
    assert_eq!(sharing["enabled"], json!(false), "{sharing}");
    assert_eq!(sharing["token"], json!(""), "{sharing}");
}

/// A server with public sharing turned on, two users, and a board of the first.
async fn shared_setup() -> (TestServer, Board) {
    let th = Setup::local_mode().start().await.init_basic().await;
    let user = th.client.me().await;
    let board = board_with_admin(&th, &user.id, BoardType::Open).await;
    (th, board)
}

// ---------------------------------------------------------------------------------------
// server/integrationtests/sharing_test.go

/// Returns whether a read token lets a request without a session read the board.
async fn read_token_opens(th: &TestServer, board_id: &str, token: &str) -> bool {
    let path = format!("/boards/{board_id}?read_token={token}");
    th.anonymous().get(&path).await.status == reqwest::StatusCode::OK
}

/// `TestSharing/"an unauthenticated client should not be able to get a sharing"`
#[tokio::test]
async fn sharing_an_unauthenticated_client_should_not_be_able_to_get_a_sharing() {
    let th = Setup::native().start().await.init_basic().await;

    let response = th.anonymous().get(&sharing_path("board-id")).await;

    check_error(&response, StatusCode::UNAUTHORIZED, "no session token");
    th.teardown().await;
}

/// `TestSharing/"Check no initial sharing"`
#[tokio::test]
async fn sharing_check_no_initial_sharing() {
    let th = Setup::native().start().await.init_basic().await;
    let user = th.client.me().await;
    let board = board_with_admin(&th, &user.id, BoardType::Open).await;

    let error = th
        .app
        .sharing(&board.id)
        .await
        .expect_err("a new board has no sharing record");
    assert!(error.is_not_found(), "a not-found error: {error}");

    let response = th.client.get(&sharing_path(&board.id)).await;
    check_no_sharing(&response);

    th.teardown().await;
}

/// `TestSharing/"POST sharing, config = false"` and its `"GET sharing"`
#[tokio::test]
async fn sharing_post_sharing_config_false() {
    let th = Setup::native().start().await.init_basic().await;
    let user = th.client.me().await;
    let board = board_with_admin(&th, &user.id, BoardType::Open).await;
    let token = new_id(IdKind::Token);

    // It fails with the default configuration.
    let response = post_sharing(&th.client, &board.id, &sharing_request(&board.id, &token)).await;
    // The server's configuration refuses it. (The original answers 500.)
    check_error(
        &response,
        StatusCode::FORBIDDEN,
        "public sharing is turned off on this server",
    );
    assert_eq!(response.error_reason(), "sharing_disabled");

    // "GET sharing": nothing was stored.
    let response = th.client.get(&sharing_path(&board.id)).await;
    check_no_sharing(&response);

    th.teardown().await;
}

/// `TestSharing/"POST sharing, config = true"` and its `"GET sharing"`
#[tokio::test]
async fn sharing_post_sharing_config_true() {
    let (th, board) = shared_setup().await;
    let user = th.client.me().await;
    let token = new_id(IdKind::Token);

    // It succeeds with public sharing turned on.
    let response = post_sharing(&th.client, &board.id, &sharing_request(&board.id, &token)).await;
    response.check_ok();
    assert_eq!(response.value(), json!({}), "the answer is an empty object");

    // "GET sharing"
    let response = th.client.get(&sharing_path(&board.id)).await;
    let sharing: Sharing = response.ok_json();
    assert_eq!(sharing.id, board.id, "the board");
    assert!(sharing.enabled, "sharing is on");
    assert_eq!(sharing.token, token, "the token");
    // The exact wire format, as the original answers it.
    assert_eq!(
        response.value(),
        json!({
            "id": board.id,
            "enabled": true,
            "token": token,
            "modifiedBy": user.id,
            "update_at": sharing.update_at,
        }),
        "the sharing record"
    );
    assert!(
        sharing.update_at > 1,
        "the time is the server's, not the request's"
    );

    th.teardown().await;
}

// ---------------------------------------------------------------------------------------
// server/app/sharing_test.go

/// `TestGetSharing/"should get a sharing successfully"`
#[tokio::test]
async fn get_sharing_should_get_a_sharing_successfully() {
    let th = Setup::native().start().await;
    let want = Sharing {
        id: new_id(IdKind::Block),
        enabled: true,
        token: "token".to_owned(),
        modified_by: "otherid".to_owned(),
        update_at: 0,
    };
    th.app
        .upsert_sharing(&want)
        .await
        .expect("the sharing is stored");

    let got = th.app.sharing(&want.id).await.expect("the sharing is read");

    assert!(got.update_at > 0, "the store set the time");
    assert_eq!(
        got,
        Sharing {
            update_at: got.update_at,
            ..want
        },
        "what was stored"
    );
    th.teardown().await;
}

/// `TestGetSharing/"should return a not found error"`
#[tokio::test]
async fn get_sharing_should_return_a_not_found_error() {
    let th = Setup::native().start().await;

    let error = th
        .app
        .sharing("test-id")
        .await
        .expect_err("there is no such sharing");

    assert!(error.is_not_found(), "a not-found error: {error}");
    assert_eq!(error.status(), StatusCode::NOT_FOUND, "{error}");
    assert_eq!(error.to_string(), "sharing not found", "the message");
    th.teardown().await;
}

/// `TestUpsertSharing/"should success to upsert sharing"`
#[tokio::test]
async fn upsert_sharing_should_success_to_upsert_sharing() {
    let th = Setup::native().start().await;
    let mut sharing = Sharing {
        id: new_id(IdKind::Block),
        enabled: true,
        token: "token".to_owned(),
        modified_by: "otherid".to_owned(),
        update_at: 0,
    };

    th.app
        .upsert_sharing(&sharing)
        .await
        .expect("the sharing is stored");
    // A second time replaces the first.
    sharing.enabled = false;
    sharing.token = "token2".to_owned();
    th.app
        .upsert_sharing(&sharing)
        .await
        .expect("the sharing is replaced");

    let got = th
        .app
        .sharing(&sharing.id)
        .await
        .expect("the sharing is read");
    assert!(!got.enabled, "the later state");
    assert_eq!(got.token, "token2", "the later token");
    th.teardown().await;
}

// ---------------------------------------------------------------------------------------
// Not in the original: the routes' answers, checked against the original server.

/// Only an administrator of the board may read or change its sharing. The answer is the
/// same for a board that does not exist.
#[tokio::test]
async fn sharing_needs_the_permission_to_share_the_board() {
    let (th, board) = shared_setup().await;
    let owner = th.client.me().await;
    let other = th.client2.me().await;
    let private = board_with_admin(&th, &owner.id, BoardType::Private).await;
    let request = sharing_request(&board.id, "token");
    let refusal = "access denied to sharing the board";

    // Not a member of an open board: the board can be seen, its sharing not touched.
    let response = th.client2.get(&sharing_path(&board.id)).await;
    check_error(&response, StatusCode::FORBIDDEN, refusal);
    let response = post_sharing(&th.client2, &board.id, &request).await;
    check_error(&response, StatusCode::FORBIDDEN, refusal);
    // Not a member of a private board: it answers as a board that does not exist.
    for board_id in [private.id.as_str(), "no-such-board"] {
        let response = th.client2.get(&sharing_path(board_id)).await;
        check_error(&response, StatusCode::NOT_FOUND, "board not found");
        let response = post_sharing(&th.client2, board_id, &request).await;
        check_error(&response, StatusCode::NOT_FOUND, "board not found");
    }
    // The administrator gets the same answer for a board that does not exist.
    let response = th.client.get(&sharing_path("no-such-board")).await;
    check_error(&response, StatusCode::NOT_FOUND, "board not found");

    // A member who is an editor, not an administrator.
    let editor = BoardMember {
        board_id: board.id.clone(),
        user_id: other.id.clone(),
        scheme_editor: true,
        ..BoardMember::default()
    };
    th.app
        .store()
        .save_member(&editor)
        .await
        .expect("the member is saved");
    let response = th.client2.get(&sharing_path(&board.id)).await;
    check_error(&response, StatusCode::FORBIDDEN, refusal);
    let response = post_sharing(&th.client2, &board.id, &request).await;
    check_error(&response, StatusCode::FORBIDDEN, refusal);
    // The refusal comes before the body is looked at.
    let response = th
        .client2
        .send_raw_body(Method::POST, &sharing_path(&board.id), "{")
        .await;
    check_error(&response, StatusCode::FORBIDDEN, refusal);

    // Nothing was stored by any of these.
    check_no_sharing(&th.client.get(&sharing_path(&board.id)).await);

    // The sharing routes have no anonymous access, with or without a read token.
    post_sharing(&th.client, &board.id, &request)
        .await
        .check_ok();
    let anonymous = th.anonymous();
    let response = anonymous
        .get(&format!("{}?read_token=token", sharing_path(&board.id)))
        .await;
    check_error(&response, StatusCode::UNAUTHORIZED, "no session token");
    let response = post_sharing(&anonymous, &board.id, &request).await;
    check_error(&response, StatusCode::UNAUTHORIZED, "no session token");

    th.teardown().await;
}

/// The board is the one of the path and the modifier the one of the session, whatever
/// the body says about either.
#[tokio::test]
async fn post_sharing_takes_the_board_and_the_modifier_from_the_request() {
    let (th, board) = shared_setup().await;
    let user = th.client.me().await;
    let other_board = board_with_admin(&th, &user.id, BoardType::Open).await;

    let response = th
        .client
        .post(
            &sharing_path(&board.id),
            &json!({
                "id": other_board.id,
                "enabled": true,
                "token": "token",
                "modifiedBy": "someone-else",
                "update_at": 1,
            }),
        )
        .await;
    response.check_ok();

    let sharing: Sharing = th.client.get(&sharing_path(&board.id)).await.ok_json();
    assert_eq!(sharing.id, board.id, "the board of the path");
    assert_eq!(sharing.modified_by, user.id, "the user of the session");
    assert!(sharing.update_at > 1, "the time of the server");
    check_no_sharing(&th.client.get(&sharing_path(&other_board.id)).await);

    th.teardown().await;
}

/// The read token is whatever the client sends: the original stores the body's `token`
/// as it is, and so does the port. Listed as an open question in the porting notes; this
/// test pins what is preserved.
#[tokio::test]
async fn post_sharing_stores_the_token_the_client_chose() {
    let (th, board) = shared_setup().await;
    let user = th.client.me().await;
    let second = board_with_admin(&th, &user.id, BoardType::Private).await;
    // A read token is valid if it lets a request without a session read the board.
    let read_token = async |board_id: &str, token: &str| {
        let path = format!("/boards/{board_id}?read_token={token}");
        th.anonymous().get(&path).await.status == reqwest::StatusCode::OK
    };

    // A short, guessable token, and the same one on two boards.
    for board_id in [&board.id, &second.id] {
        post_sharing(&th.client, board_id, &sharing_request(board_id, "x"))
            .await
            .check_ok();
        let sharing: Sharing = th.client.get(&sharing_path(board_id)).await.ok_json();
        assert_eq!(sharing.token, "x", "stored as sent");
        assert!(read_token(board_id, "x").await, "and it opens the board");
        assert!(!read_token(board_id, "y").await, "another token does not");
    }

    // An empty token is stored too, but no request can present it: a request without a
    // read token, or with an empty one, is not looked up.
    post_sharing(&th.client, &board.id, &sharing_request(&board.id, ""))
        .await
        .check_ok();
    let sharing: Sharing = th.client.get(&sharing_path(&board.id)).await.ok_json();
    assert!(sharing.enabled, "sharing is on");
    assert_eq!(sharing.token, "", "with an empty token");
    assert!(!read_token(&board.id, "").await, "which opens nothing");
    assert_ne!(
        th.anonymous()
            .get(&format!("/boards/{}", board.id))
            .await
            .status,
        reqwest::StatusCode::OK,
        "and neither does leaving the token out"
    );
    assert!(!read_token(&board.id, "x").await, "the old token is gone");

    th.teardown().await;
}

/// Turning sharing off keeps the record and closes the board to its token.
#[tokio::test]
async fn post_sharing_turns_sharing_off() {
    let (th, board) = shared_setup().await;
    post_sharing(&th.client, &board.id, &sharing_request(&board.id, "token"))
        .await
        .check_ok();
    assert!(
        read_token_opens(&th, &board.id, "token").await,
        "the token opens the board"
    );

    let off = Sharing {
        enabled: false,
        ..sharing_request(&board.id, "token")
    };
    post_sharing(&th.client, &board.id, &off).await.check_ok();

    let sharing: Sharing = th.client.get(&sharing_path(&board.id)).await.ok_json();
    assert!(!sharing.enabled, "sharing is off");
    assert_eq!(sharing.token, "token", "the token is kept");
    assert!(
        !read_token_opens(&th, &board.id, "token").await,
        "the token no longer opens the board"
    );

    th.teardown().await;
}

/// How the body is decoded: missing and `null` members are defaults, and everything that
/// is no JSON object with members of the right types is a bad request.
#[tokio::test]
async fn post_sharing_decodes_missing_members_as_defaults_and_refuses_the_rest() {
    let (th, board) = shared_setup().await;
    let path = sharing_path(&board.id);
    post_sharing(&th.client, &board.id, &sharing_request(&board.id, "token"))
        .await
        .check_ok();

    // (The original answers these with 500, and reads `null` as an empty object.)
    for malformed in [
        "{",
        "",
        "[]",
        "null",
        r#"{"enabled": "yes"}"#,
        r#"{"token": 5}"#,
        "5",
    ] {
        let response = th
            .client
            .send_raw_body(Method::POST, &path, malformed)
            .await;
        response.check_bad_request();
        assert_eq!(response.error_code(), "bad_request", "{malformed}");
    }
    let sharing: Sharing = th.client.get(&path).await.ok_json();
    assert!(sharing.enabled, "none of these changed the record");
    assert_eq!(sharing.token, "token", "nor its token");

    // An empty object and `null` members are both "sharing off, no token".
    for defaults in ["{}", r#"{"enabled": null, "token": null}"#] {
        post_sharing(&th.client, &board.id, &sharing_request(&board.id, "token"))
            .await
            .check_ok();
        th.client
            .send_raw_body(Method::POST, &path, defaults)
            .await
            .check_ok();
        let sharing: Value = th.client.get(&path).await.ok_json();
        assert_eq!(sharing["enabled"], json!(false), "after {defaults}");
        assert_eq!(sharing["token"], json!(""), "after {defaults}");
    }

    th.teardown().await;
}

/// In single-user mode there is no user to name as the modifier.
#[tokio::test]
async fn post_sharing_in_single_user_mode_records_no_modifier() {
    let th = Setup::single_user()
        .configure(|config| config.enable_public_shared_boards = true)
        .start()
        .await;
    let board = board_with_admin(&th, fvoci_model::SINGLE_USER_ID, BoardType::Open).await;

    post_sharing(&th.client, &board.id, &sharing_request(&board.id, "token"))
        .await
        .check_ok();

    let response = th.client.get(&sharing_path(&board.id)).await;
    let sharing: Sharing = response.ok_json();
    assert_eq!(sharing.modified_by, "", "no modifier");
    assert_eq!(response.value()["modifiedBy"], json!(""), "on the wire too");

    th.teardown().await;
}

/// Reading the sharing record does not depend on public sharing being turned on: the
/// original's handler does not look at the configuration (`server/api/sharing.go:24-87`).
/// The original server at hand runs with sharing turned on, so this is taken from its
/// code, not from its answers.
#[tokio::test]
async fn get_sharing_does_not_depend_on_the_configuration() {
    let th = Setup::native().start().await.init_basic().await;
    let user = th.client.me().await;
    let board = board_with_admin(&th, &user.id, BoardType::Open).await;
    // A record from a time when sharing was turned on.
    th.app
        .upsert_sharing(&Sharing {
            modified_by: user.id.clone(),
            ..sharing_request(&board.id, "token")
        })
        .await
        .expect("the sharing is stored");

    let sharing: Sharing = th.client.get(&sharing_path(&board.id)).await.ok_json();

    assert!(sharing.enabled, "the record as stored");
    assert_eq!(sharing.token, "token", "with its token");
    assert!(
        !read_token_opens(&th, &board.id, "token").await,
        "but the token opens nothing while sharing is turned off"
    );

    th.teardown().await;
}
