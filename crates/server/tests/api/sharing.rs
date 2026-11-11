//! Sharing a board through a read token: who may share, what the server answers, and
//! that the token is the server's to make and to forget.
//!
//! The tests whose names begin with `sharing_` began as ports of
//! `server/integrationtests/sharing_test.go`, whose `TestSharing` runs its subtests on
//! one server and turns public sharing on halfway. A server's configuration is fixed
//! here, so each has a server of its own with the configuration it needs.

use fvoci_model::{Board, BoardMember, BoardType, Sharing, new_id};
use fvoci_server::auth::secret::SECRET_LENGTH;
use fvoci_store::SharingChange;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

use crate::common::{ApiResponse, Client, ROOT_TEAM_ID, Setup, TestServer, UNKNOWN_ID};

/// Puts a board into the store without going through the board routes, with `user_id` as
/// its administrator.
async fn board_with_admin(th: &TestServer, user_id: &str, board_type: BoardType) -> Board {
    let mut board = Board {
        id: new_id(),
        team_id: ROOT_TEAM_ID.to_owned(),
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

/// `POST /boards/{id}/sharing` with a body.
async fn post_sharing(client: &Client, board_id: &str, body: Value) -> ApiResponse {
    client.post(&sharing_path(board_id), &body).await
}

/// Shares the board through the route and returns the answer, which is the sharing
/// state.
async fn share(client: &Client, board_id: &str) -> Sharing {
    post_sharing(client, board_id, json!({"enabled": true}))
        .await
        .ok_json()
}

async fn sharing_of(client: &Client, board_id: &str) -> Sharing {
    client.get(&sharing_path(board_id)).await.ok_json()
}

#[track_caller]
fn check_error(response: &ApiResponse, status: StatusCode, message: &str) {
    response.check_error(status, message);
}

/// The answer for a board that was never shared: not shared, and nothing else.
#[track_caller]
fn check_never_shared(response: &ApiResponse) {
    response.check_ok();
    assert_eq!(response.value(), json!({"enabled": false}), "never shared");
}

/// A server with public sharing turned on, two users, and a board of the first.
async fn shared_setup() -> (TestServer, Board) {
    let th = Setup::local_mode().start().await.init_basic().await;
    let user = th.client.me().await;
    let board = board_with_admin(&th, &user.id, BoardType::Open).await;
    (th, board)
}

/// Returns whether a read token lets a request without a session read the board.
async fn read_token_opens(th: &TestServer, board_id: &str, token: &str) -> bool {
    let path = format!("/boards/{board_id}?readToken={token}");
    th.anonymous().get(&path).await.status == StatusCode::OK
}

// ---------------------------------------------------------------------------------------
// what the routes answer

/// `TestSharing/"an unauthenticated client should not be able to get a sharing"`
#[tokio::test]
async fn sharing_an_unauthenticated_client_should_not_be_able_to_get_a_sharing() {
    let th = Setup::native().start().await.init_basic().await;

    let response = th.anonymous().get(&sharing_path(UNKNOWN_ID)).await;

    check_error(&response, StatusCode::UNAUTHORIZED, "no session token");
    th.teardown().await;
}

/// `TestSharing/"Check no initial sharing"`: a board that was never shared is not
/// shared, which is an answer and no error.
#[tokio::test]
async fn sharing_check_no_initial_sharing() {
    let th = Setup::native().start().await.init_basic().await;
    let user = th.client.me().await;
    let board = board_with_admin(&th, &user.id, BoardType::Open).await;

    let sharing = th
        .app
        .sharing(&board.id)
        .await
        .expect("a board that was never shared has a sharing state too");
    assert!(!sharing.enabled, "not shared");
    assert_eq!(sharing.token, "", "and without a token");

    check_never_shared(&th.client.get(&sharing_path(&board.id)).await);

    th.teardown().await;
}

/// `TestSharing/"POST sharing, config = false"` and its `"GET sharing"`: with public
/// sharing turned off in the configuration nothing can be shared, or unshared.
#[tokio::test]
async fn sharing_post_sharing_config_false() {
    let th = Setup::native().start().await.init_basic().await;
    let user = th.client.me().await;
    let board = board_with_admin(&th, &user.id, BoardType::Open).await;

    for body in [json!({"enabled": true}), json!({"enabled": false})] {
        let response = post_sharing(&th.client, &board.id, body).await;
        check_error(
            &response,
            StatusCode::FORBIDDEN,
            "public sharing is turned off on this server",
        );
        assert_eq!(response.error_reason(), "sharing_disabled");
    }

    // "GET sharing": nothing was stored.
    check_never_shared(&th.client.get(&sharing_path(&board.id)).await);

    th.teardown().await;
}

/// `TestSharing/"POST sharing, config = true"` and its `"GET sharing"`: sharing a board
/// answers the sharing state with the token the server made, and that is what is read
/// afterwards.
#[tokio::test]
async fn sharing_post_sharing_config_true() {
    let (th, board) = shared_setup().await;

    let response = post_sharing(&th.client, &board.id, json!({"enabled": true})).await;
    let shared: Sharing = response.ok_json();
    assert!(shared.enabled, "sharing is on");
    assert_eq!(shared.token.len(), SECRET_LENGTH, "a secret of the server");
    assert!(shared.update_at > 0, "changed now");
    // The exact wire format: these members and no others.
    assert_eq!(
        response.value(),
        json!({"enabled": true, "token": shared.token, "updateAt": shared.update_at}),
        "the sharing state"
    );

    // "GET sharing"
    let response = th.client.get(&sharing_path(&board.id)).await;
    assert_eq!(
        response.ok_json::<Sharing>(),
        shared,
        "what is read is what was answered"
    );
    assert!(
        read_token_opens(&th, &board.id, &shared.token).await,
        "and the token opens the board"
    );

    th.teardown().await;
}

// ---------------------------------------------------------------------------------------
// the token is the server's

/// A request cannot choose the read token. A body that carries one is refused, whatever
/// the token, and changes nothing. (A member that is `null` is a member that is not
/// there, here as everywhere in this API.)
#[tokio::test]
async fn a_request_cannot_set_the_read_token() {
    let (th, board) = shared_setup().await;
    let shared = share(&th.client, &board.id).await;

    for token in [
        json!("x"),
        json!(""),
        json!(shared.token),
        json!(5),
        Value::Null,
    ] {
        let response = post_sharing(
            &th.client,
            &board.id,
            json!({"enabled": true, "token": token}),
        )
        .await;
        check_error(
            &response,
            StatusCode::BAD_REQUEST,
            "the server makes the read token; a request cannot set it",
        );
        assert_eq!(response.value()["field"], json!("token"), "the member");
    }
    assert_eq!(
        sharing_of(&th.client, &board.id).await,
        shared,
        "nothing changed"
    );
    assert!(
        !read_token_opens(&th, &board.id, "x").await,
        "no such token"
    );

    th.teardown().await;
}

/// Every shared board has a token of its own, which opens that board and no other.
/// Sharing a board that is shared keeps its token; asking for a new one replaces it,
/// and the old link is dead.
#[tokio::test]
async fn a_shared_board_keeps_its_token_until_a_new_one_is_asked_for() {
    let (th, board) = shared_setup().await;
    let user = th.client.me().await;
    let second = board_with_admin(&th, &user.id, BoardType::Private).await;

    let first = share(&th.client, &board.id).await;
    let other = share(&th.client, &second.id).await;
    assert_ne!(first.token, other.token, "a token per board");
    assert!(
        read_token_opens(&th, &board.id, &first.token).await,
        "a token opens its board"
    );
    assert!(
        !read_token_opens(&th, &second.id, &first.token).await,
        "and not another board"
    );
    assert!(
        !read_token_opens(&th, &board.id, "").await,
        "no token opens nothing"
    );

    // Sharing what is shared: the same token, also with `regenerate` said to be false.
    for body in [
        json!({"enabled": true}),
        json!({"enabled": true, "regenerate": false}),
        json!({"enabled": true, "regenerate": null}),
    ] {
        let again: Sharing = post_sharing(&th.client, &board.id, body).await.ok_json();
        assert_eq!(again.token, first.token, "the token stays");
    }

    let renewed: Sharing = post_sharing(
        &th.client,
        &board.id,
        json!({"enabled": true, "regenerate": true}),
    )
    .await
    .ok_json();
    assert!(renewed.enabled, "still shared");
    assert_eq!(renewed.token.len(), SECRET_LENGTH, "under a secret");
    assert_ne!(renewed.token, first.token, "a new one");
    assert!(
        read_token_opens(&th, &board.id, &renewed.token).await,
        "the new token opens the board"
    );
    assert!(
        !read_token_opens(&th, &board.id, &first.token).await,
        "the old one no longer does"
    );
    assert_eq!(
        sharing_of(&th.client, &second.id).await,
        other,
        "the other board's sharing is its own"
    );

    th.teardown().await;
}

/// Turning sharing off forgets the token: the link is dead, and sharing the board again
/// makes a new link and does not revive the old one.
#[tokio::test]
async fn turning_sharing_off_forgets_the_token() {
    let (th, board) = shared_setup().await;
    let shared = share(&th.client, &board.id).await;
    assert!(
        read_token_opens(&th, &board.id, &shared.token).await,
        "the token opens the board"
    );

    // `regenerate` has nothing to renew when sharing is turned off.
    for body in [
        json!({"enabled": false}),
        json!({"enabled": false, "regenerate": true}),
    ] {
        let response = post_sharing(&th.client, &board.id, body).await;
        let off: Sharing = response.ok_json();
        assert!(!off.enabled, "sharing is off");
        assert_eq!(
            response.value(),
            json!({"enabled": false, "updateAt": off.update_at}),
            "and there is no token"
        );
        assert!(off.update_at >= shared.update_at, "changed now");
    }
    assert_eq!(
        th.client.get(&sharing_path(&board.id)).await.value()["token"],
        Value::Null,
        "none is read either"
    );
    assert!(
        !read_token_opens(&th, &board.id, &shared.token).await,
        "the token no longer opens the board"
    );

    let again = share(&th.client, &board.id).await;
    assert_ne!(again.token, shared.token, "shared again: a new token");
    assert!(
        !read_token_opens(&th, &board.id, &shared.token).await,
        "the old link stays dead"
    );
    assert!(
        read_token_opens(&th, &board.id, &again.token).await,
        "the new one works"
    );

    th.teardown().await;
}

// ---------------------------------------------------------------------------------------
// who may share, and how the body is read

/// Only an administrator of the board may read or change its sharing. The answer is the
/// same for a board that does not exist.
#[tokio::test]
async fn sharing_needs_the_permission_to_share_the_board() {
    let (th, board) = shared_setup().await;
    let owner = th.client.me().await;
    let other = th.client2.me().await;
    let private = board_with_admin(&th, &owner.id, BoardType::Private).await;
    let request = || json!({"enabled": true});
    let refusal = "access denied to sharing the board";

    // Not a member of an open board: the board can be seen, its sharing not touched.
    let response = th.client2.get(&sharing_path(&board.id)).await;
    check_error(&response, StatusCode::FORBIDDEN, refusal);
    let response = post_sharing(&th.client2, &board.id, request()).await;
    check_error(&response, StatusCode::FORBIDDEN, refusal);
    // Not a member of a private board: it answers as a board that does not exist.
    for board_id in [private.id.as_str(), UNKNOWN_ID, "no-such-board"] {
        let response = th.client2.get(&sharing_path(board_id)).await;
        check_error(&response, StatusCode::NOT_FOUND, "board not found");
        let response = post_sharing(&th.client2, board_id, request()).await;
        check_error(&response, StatusCode::NOT_FOUND, "board not found");
    }
    // The administrator gets the same answer for a board that does not exist.
    for board_id in [UNKNOWN_ID, "no-such-board"] {
        let response = th.client.get(&sharing_path(board_id)).await;
        check_error(&response, StatusCode::NOT_FOUND, "board not found");
        let response = post_sharing(&th.client, board_id, request()).await;
        check_error(&response, StatusCode::NOT_FOUND, "board not found");
    }

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
    let response = post_sharing(&th.client2, &board.id, request()).await;
    check_error(&response, StatusCode::FORBIDDEN, refusal);
    // The refusal comes before the body is looked at.
    let response = th
        .client2
        .send_raw_body(Method::POST, &sharing_path(&board.id), "{")
        .await;
    check_error(&response, StatusCode::FORBIDDEN, refusal);

    // Nothing was stored by any of these.
    check_never_shared(&th.client.get(&sharing_path(&board.id)).await);

    // The sharing routes have no anonymous access, with or without a read token.
    let shared = share(&th.client, &board.id).await;
    let anonymous = th.anonymous();
    let response = anonymous
        .get(&format!(
            "{}?readToken={}",
            sharing_path(&board.id),
            shared.token
        ))
        .await;
    check_error(&response, StatusCode::UNAUTHORIZED, "no session token");
    let response = post_sharing(&anonymous, &board.id, request()).await;
    check_error(&response, StatusCode::UNAUTHORIZED, "no session token");

    th.teardown().await;
}

/// The board is the one of the path and the modifier the one of the session. Members
/// of the body that say otherwise, and any others the route does not know, are not
/// looked at.
#[tokio::test]
async fn post_sharing_takes_the_board_and_the_modifier_from_the_request() {
    let (th, board) = shared_setup().await;
    let user = th.client.me().await;
    let other_board = board_with_admin(&th, &user.id, BoardType::Open).await;

    let shared: Sharing = post_sharing(
        &th.client,
        &board.id,
        json!({
            "id": other_board.id,
            "boardId": other_board.id,
            "enabled": true,
            "modifiedBy": UNKNOWN_ID,
            "updateAt": 1,
        }),
    )
    .await
    .ok_json();

    assert!(shared.update_at > 1, "the time of the server");
    let stored = th
        .app
        .store()
        .get_sharing(&board.id)
        .await
        .expect("the board of the path is shared");
    assert_eq!(stored.modified_by, user.id, "by the user of the session");
    assert_eq!(
        stored.token, shared.token,
        "under the token that was answered"
    );
    check_never_shared(&th.client.get(&sharing_path(&other_board.id)).await);

    th.teardown().await;
}

/// How the body is read: `enabled` must be there and be a boolean, `regenerate` a
/// boolean if it is there; anything else is a bad request that changes nothing.
#[tokio::test]
async fn post_sharing_wants_a_boolean_and_refuses_the_rest() {
    let (th, board) = shared_setup().await;
    let path = sharing_path(&board.id);
    let shared = share(&th.client, &board.id).await;

    for (malformed, field) in [
        ("{", None),
        ("", None),
        ("[]", None),
        ("null", None),
        ("5", None),
        ("{}", Some("enabled")),
        (r#"{"enabled": null}"#, Some("enabled")),
        (r#"{"enabled": "yes"}"#, Some("enabled")),
        (r#"{"enabled": 1}"#, Some("enabled")),
        (r#"{"regenerate": true}"#, Some("enabled")),
        (
            r#"{"enabled": true, "regenerate": "yes"}"#,
            Some("regenerate"),
        ),
        (r#"{"enabled": false, "regenerate": 0}"#, Some("regenerate")),
    ] {
        let response = th
            .client
            .send_raw_body(Method::POST, &path, malformed)
            .await;
        response.check_bad_request();
        assert_eq!(response.error_code(), "bad_request", "{malformed}");
        assert_eq!(
            response.value().get("field").and_then(Value::as_str),
            field,
            "the member that is wrong in {malformed}"
        );
    }
    assert_eq!(
        sharing_of(&th.client, &board.id).await,
        shared,
        "none of these changed the sharing"
    );

    th.teardown().await;
}

/// Reading the sharing state does not depend on public sharing being turned on in the
/// configuration, but the token of a board that was shared while it was on opens
/// nothing while it is off.
#[tokio::test]
async fn get_sharing_does_not_depend_on_the_configuration() {
    let th = Setup::native().start().await.init_basic().await;
    let user = th.client.me().await;
    let board = board_with_admin(&th, &user.id, BoardType::Open).await;
    // A board shared at a time when sharing was turned on.
    let token = th.share(&board.id).await;

    let sharing = sharing_of(&th.client, &board.id).await;

    assert!(sharing.enabled, "the state as stored");
    assert_eq!(sharing.token, token, "with its token");
    assert!(
        !read_token_opens(&th, &board.id, &token).await,
        "but the token opens nothing while sharing is turned off"
    );

    th.teardown().await;
}

// ---------------------------------------------------------------------------------------
// the application layer

/// `TestGetSharing` and `TestUpsertSharing` of `server/app/sharing_test.go`, for the
/// functions there are now: the state that is set is the state that is read, and a
/// board that does not exist cannot be shared.
#[tokio::test]
async fn app_sharing_is_set_and_read() {
    let th = Setup::native().start().await;
    let user_id = th.stranger().await;
    let board = board_with_admin(&th, &user_id, BoardType::Open).await;

    let set = th
        .app
        .set_sharing(&board.id, SharingChange::On, &user_id)
        .await
        .expect("the board is shared");
    let got = th.app.sharing(&board.id).await.expect("the state is read");
    assert_eq!(got, set, "what was stored");
    assert_eq!(got.board_id, board.id, "of the board");
    assert_eq!(got.modified_by, user_id, "by the user");
    assert!(got.update_at > 0, "the store set the time");

    let off = th
        .app
        .set_sharing(&board.id, SharingChange::Off, &user_id)
        .await
        .expect("the board is unshared");
    assert!(!off.enabled, "the later state");
    assert_eq!(off.token, "", "has no token");

    for board_id in [UNKNOWN_ID, "test-id"] {
        let error = th
            .app
            .set_sharing(board_id, SharingChange::On, &user_id)
            .await
            .expect_err("there is no such board");
        assert_eq!(error.status(), StatusCode::NOT_FOUND, "{error}");
        assert_eq!(error.to_string(), "board not found", "the message");
        let never = th
            .app
            .sharing(board_id)
            .await
            .expect("what is not there is not shared");
        assert!(!never.enabled, "{board_id}");
    }
    th.teardown().await;
}
