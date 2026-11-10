//! Subscriptions to blocks.
//! Port of `server/integrationtests/subscriptions_test.go`, plus tests of what
//! `server/api/subscriptions.go` decides and the original leaves untested: the
//! validation with its order and messages, subscribing twice, whose subscriptions a
//! user may read and end, and that nobody is told. The expectations of those were
//! observed on the original, except where a test says that the port differs.
//!
//! The subtests of the original share one server; here every subtest has its own.

use std::time::Duration;

use fvoci_model::{
    Block, BlockType, Board, BoardType, IdKind, SINGLE_USER_ID, SubscriberType, Subscription,
    new_id,
};
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

use crate::boards::TeamSocket;
use crate::cards::{
    Roles, board_with_roles, check_error, check_internal_error, insert_block, new_block,
};
use crate::common::{ApiResponse, Client, Setup, TEST_TEAM_ID};

/// How long background notifications may take to finish.
const BACKGROUND: Duration = Duration::from_secs(10);

const PATH: &str = "/subscriptions";

/// The refusal of this area, whatever is refused.
const DENIED: &str = "access denied";

// -- helpers --------------------------------------------------------------------------

/// The body that subscribes a user to a card.
fn request(block_id: &str, subscriber_id: &str) -> Value {
    json!({
        "blockType": "card",
        "blockId": block_id,
        "subscriberType": "user",
        "subscriberId": subscriber_id,
    })
}

/// `CreateSubscription`, expecting success.
async fn subscribe(client: &Client, block_id: &str, subscriber_id: &str) -> Subscription {
    client
        .post(PATH, &request(block_id, subscriber_id))
        .await
        .ok_json()
}

/// `GetSubscriptions`
async fn get_subscriptions(client: &Client, subscriber_id: &str) -> ApiResponse {
    client.get(&format!("{PATH}/{subscriber_id}")).await
}

/// `GetSubscriptions`, expecting success; sorted by block, because the list has no
/// defined order.
async fn subscriptions_of(client: &Client, subscriber_id: &str) -> Vec<Subscription> {
    let mut subscriptions: Vec<Subscription> =
        get_subscriptions(client, subscriber_id).await.ok_json();
    subscriptions.sort_by(|a, b| a.block_id.cmp(&b.block_id));
    subscriptions
}

/// `DeleteSubscription`
async fn unsubscribe(client: &Client, block_id: &str, subscriber_id: &str) -> ApiResponse {
    client
        .delete(&format!("{PATH}/{block_id}/{subscriber_id}"))
        .await
}

fn sorted(mut subscriptions: Vec<Subscription>) -> Vec<Subscription> {
    subscriptions.sort_by(|a, b| a.block_id.cmp(&b.block_id));
    subscriptions
}

/// Inserts a card block on the board through the blocks route.
async fn insert_card(client: &Client, board_id: &str) -> Block {
    insert_block(client, &new_block(board_id, "", BlockType::Card)).await
}

/// `createTestSubscriptions`: a board of the client's user with `count` cards, and the
/// user's subscription to each. Returns the subscriptions as the server answered and
/// the user's identifier.
async fn create_test_subscriptions(client: &Client, count: usize) -> (Vec<Subscription>, String) {
    let user = client.me().await;
    let board = Board {
        team_id: "0".to_owned(),
        board_type: BoardType::Open,
        create_at: 1,
        update_at: 1,
        ..Board::default()
    };
    let board: Board = client.create_board(&board).await.ok_json();

    let mut subscriptions = Vec::with_capacity(count);
    for _ in 0..count {
        let card = insert_card(client, &board.id).await;
        let subscription = Subscription {
            block_type: card.block_type.clone(),
            block_id: card.id.clone(),
            subscriber_type: SubscriberType::User,
            subscriber_id: user.id.clone(),
            notified_at: 0,
            create_at: 0,
            delete_at: 0,
        };
        let created: Subscription = client.post(PATH, &subscription).await.ok_json();
        subscriptions.push(created);
    }
    (subscriptions, user.id)
}

// -- server/integrationtests/subscriptions_test.go ----------------------------------------

/// `TestCreateSubscription/"Create valid subscription"`
#[tokio::test]
async fn create_subscription_create_valid_subscription() {
    let th = Setup::native().start().await.init_basic().await;

    let (subscriptions, user_id) = create_test_subscriptions(&th.client, 5).await;
    assert_eq!(subscriptions.len(), 5, "five subscriptions were created");

    // Fetch the newly created subscriptions and compare.
    let found = subscriptions_of(&th.client, &user_id).await;
    assert_eq!(found, sorted(subscriptions), "as they were answered");
    th.teardown().await;
}

/// `TestCreateSubscription/"Create invalid subscription"`
#[tokio::test]
async fn create_subscription_create_invalid_subscription() {
    let th = Setup::native().start().await.init_basic().await;
    let user = th.client.me().await;

    // The original's client sends the empty subscriber type along.
    let response = th
        .client
        .post(
            PATH,
            &json!({
                "blockType": "",
                "blockId": "",
                "subscriberType": "",
                "subscriberId": user.id,
                "createAt": 0,
                "deleteAt": 0,
            }),
        )
        .await;

    check_error(&response, StatusCode::BAD_REQUEST, "missing block id");
    th.teardown().await;
}

/// `TestCreateSubscription/"Create subscription for another user"`
#[tokio::test]
async fn create_subscription_create_subscription_for_another_user() {
    let th = Setup::native().start().await.init_basic().await;

    let response = th
        .client
        .post(
            PATH,
            &json!({
                "blockType": "",
                "blockId": "",
                "subscriberType": "",
                "subscriberId": new_id(IdKind::User),
                "createAt": 0,
                "deleteAt": 0,
            }),
        )
        .await;

    // As in the original, the subscription is refused for what it lacks before the
    // subscriber is compared with the session's user.
    check_error(&response, StatusCode::BAD_REQUEST, "missing block id");
    th.teardown().await;
}

/// `TestGetSubscriptions/"Get subscriptions for user"`
#[tokio::test]
async fn get_subscriptions_get_subscriptions_for_user() {
    let th = Setup::native().start().await.init_basic().await;

    let (my_subscriptions, user1_id) = create_test_subscriptions(&th.client, 5).await;
    assert_eq!(my_subscriptions.len(), 5, "five of the first user");
    // Create more subscriptions with a different user.
    let (other_subscriptions, _) = create_test_subscriptions(&th.client2, 10).await;
    assert_eq!(other_subscriptions.len(), 10, "ten of the second user");

    // Only the ones created for the current user are returned.
    let found = subscriptions_of(&th.client, &user1_id).await;
    assert_eq!(found, sorted(my_subscriptions), "the first user's own");
    th.teardown().await;
}

/// `TestDeleteSubscription/"Delete valid subscription"`
#[tokio::test]
async fn delete_subscription_delete_valid_subscription() {
    let th = Setup::native().start().await.init_basic().await;
    let (subscriptions, user_id) = create_test_subscriptions(&th.client, 3).await;
    assert_eq!(subscriptions.len(), 3, "three subscriptions were created");

    let response = unsubscribe(&th.client, &subscriptions[1].block_id, &user_id).await;
    response.check_ok();
    assert_eq!(response.text(), "{}", "the empty object");

    // Fetch the subscriptions and ensure the list is correct.
    let found = subscriptions_of(&th.client, &user_id).await;
    assert_eq!(found.len(), 2, "two are left");
    assert!(found.contains(&subscriptions[0]), "the first is left");
    assert!(found.contains(&subscriptions[2]), "the third is left");
    assert!(!found.contains(&subscriptions[1]), "the second is gone");
    th.teardown().await;
}

/// `TestDeleteSubscription/"Delete invalid subscription"`
#[tokio::test]
async fn delete_subscription_delete_invalid_subscription() {
    let th = Setup::native().start().await.init_basic().await;
    let user = th.client.me().await;

    let response = unsubscribe(&th.client, "bogus", &user.id).await;

    check_error(
        &response,
        StatusCode::NOT_FOUND,
        &format!(
            "{{subscription BlockID=bogus SubscriberID={}}} not found",
            user.id
        ),
    );
    th.teardown().await;
}

// -- POST /subscriptions ------------------------------------------------------------------

/// What a subscription must say, in the order it is checked, and how a body that is no
/// subscription is answered.
#[tokio::test]
async fn create_subscription_validation() {
    let th = Setup::native().start().await.init_basic().await;
    let me = th.client.me().await.id;
    let other = th.client2.me().await.id;
    let board = th.create_board(TEST_TEAM_ID, BoardType::Private).await;
    let card = insert_card(&th.client, &board.id).await;
    let without = |member: &str| {
        let mut body = request(&card.id, &me);
        body.as_object_mut()
            .expect("the request is an object")
            .remove(member);
        body
    };
    let with = |member: &str, value: Value| {
        let mut body = request(&card.id, &me);
        body[member] = value;
        body
    };

    let response = th.anonymous().post(PATH, &request(&card.id, &me)).await;
    response.check_unauthorized();

    let no_board_block = format!("invalid blockID: {{block ID={}}} not found", board.id);
    let all_null = json!({
        "blockType": null,
        "blockId": null,
        "subscriberType": null,
        "subscriberId": null,
    });
    let cases = [
        (without("blockId"), "missing block id"),
        (without("blockType"), "missing block type"),
        (without("subscriberId"), "missing subscriber id"),
        (without("subscriberType"), "invalid subscriber type"),
        (
            with("subscriberType", json!("robot")),
            "invalid subscriber type",
        ),
        (
            with("subscriberType", json!("User")),
            "invalid subscriber type",
        ),
        (with("blockId", json!("")), "missing block id"),
        // Nothing at all, also as `null`: the first thing missing is reported.
        (json!({}), "missing block id"),
        (json!(null), "missing block id"),
        (all_null, "missing block id"),
        (json!({"subscriberId": other}), "missing block id"),
        // Complete, but for someone else; whether the block exists is not looked at.
        (
            request(&card.id, &other),
            "userID and subscriberID mismatch",
        ),
        (request("nope", &other), "userID and subscriberID mismatch"),
        // A block that does not exist. Boards are no blocks.
        (
            request("nope", &me),
            "invalid blockID: {block ID=nope} not found",
        ),
        (with("blockId", json!(board.id)), no_board_block.as_str()),
    ];
    for (body, message) in &cases {
        let response = th.client.post(PATH, body).await;
        check_error(&response, StatusCode::BAD_REQUEST, message);
    }

    // Preserved quirk: a body that cannot be decoded is a server error
    // (server/api/subscriptions.go:60-63).
    let block_id_number = with("blockId", json!(5)).to_string();
    let time_text = with("notifiedAt", json!("now")).to_string();
    for body in [
        "{\"blockId\":",
        "",
        "[]",
        "\"card\"",
        &block_id_number,
        &time_text,
    ] {
        let response = th.client.send_raw_body(Method::POST, PATH, body).await;
        check_internal_error(&response);
    }

    assert_eq!(
        subscriptions_of(&th.client, &me).await,
        [],
        "nothing was stored"
    );
    th.teardown().await;
}

/// What a new subscription looks like, and what subscribing again does.
#[tokio::test]
async fn create_subscription_answer_and_subscribing_again() {
    let th = Setup::native().start().await.init_basic().await;
    let me = th.client.me().await.id;
    let board = th.create_board(TEST_TEAM_ID, BoardType::Private).await;
    let card = insert_card(&th.client, &board.id).await;
    let before = fvoci_server::auth::now_millis();

    // The times are the server's, whatever the request says.
    let mut body = request(&card.id, &me);
    body["notifiedAt"] = json!(5);
    body["createAt"] = json!(6);
    body["deleteAt"] = json!(7);
    let response = th.client.post(PATH, &body).await;
    let first: Subscription = response.ok_json();
    assert!(
        first.create_at >= before,
        "created now: {}",
        first.create_at
    );
    assert_eq!(
        response.value(),
        json!({
            "blockType": "card",
            "blockId": card.id,
            "subscriberType": "user",
            "subscriberId": me,
            "notifiedAt": first.create_at,
            "createAt": first.create_at,
            "deleteAt": 0,
        }),
        "the answer"
    );
    assert_eq!(
        subscriptions_of(&th.client, &me).await,
        std::slice::from_ref(&first)
    );

    // Subscribing again is answered like a new subscription, but the stored one keeps
    // its creation time and is only marked as notified now.
    tokio::time::sleep(Duration::from_millis(5)).await;
    let second = subscribe(&th.client, &card.id, &me).await;
    assert!(second.create_at > first.create_at, "answered as new");
    assert_eq!(second.notified_at, second.create_at, "notified now");
    let stored = Subscription {
        notified_at: second.notified_at,
        ..first.clone()
    };
    assert_eq!(subscriptions_of(&th.client, &me).await, [stored]);

    // The same after unsubscribing.
    unsubscribe(&th.client, &card.id, &me).await.check_ok();
    assert_eq!(subscriptions_of(&th.client, &me).await, [], "unsubscribed");
    tokio::time::sleep(Duration::from_millis(5)).await;
    let third = subscribe(&th.client, &card.id, &me).await;
    let stored = Subscription {
        notified_at: third.notified_at,
        ..first
    };
    assert_eq!(subscriptions_of(&th.client, &me).await, [stored]);

    // The block type is whatever the client says, also another one than the block's,
    // and a user may subscribe as a channel.
    for (block_type, subscriber_type) in
        [("text", "user"), ("whatever", "user"), ("card", "channel")]
    {
        let card = insert_card(&th.client, &board.id).await;
        let mut body = request(&card.id, &me);
        body["blockType"] = json!(block_type);
        body["subscriberType"] = json!(subscriber_type);
        let response = th.client.post(PATH, &body).await;
        response.check_ok();
        let answer = response.value();
        assert_eq!(answer["blockType"], json!(block_type), "{answer}");
        assert_eq!(answer["subscriberType"], json!(subscriber_type), "{answer}");
        let listed = get_subscriptions(&th.client, &me).await.value();
        let listed = listed.as_array().expect("a list");
        assert!(listed.contains(&answer), "{answer} is among {listed:?}");
    }

    th.teardown().await;
}

// -- GET /subscriptions/{subscriber_id}, DELETE /subscriptions/{block_id}/{subscriber_id} --

/// A user reads and ends their own subscriptions only.
#[tokio::test]
async fn subscriptions_of_another_user_are_refused() {
    let th = Setup::native().start().await.init_basic().await;
    let user1 = th.client.me().await.id;
    let user2 = th.client2.me().await.id;
    let board = th.create_board(TEST_TEAM_ID, BoardType::Private).await;
    let card = insert_card(&th.client, &board.id).await;
    let subscription = subscribe(&th.client, &card.id, &user1).await;

    let response = get_subscriptions(&th.anonymous(), &user1).await;
    response.check_unauthorized();
    let response = unsubscribe(&th.anonymous(), &card.id, &user1).await;
    response.check_unauthorized();

    for subscriber_id in [user1.as_str(), "nobody"] {
        let response = get_subscriptions(&th.client2, subscriber_id).await;
        check_error(&response, StatusCode::FORBIDDEN, DENIED);
        for block_id in [card.id.as_str(), "nope"] {
            let response = unsubscribe(&th.client2, block_id, subscriber_id).await;
            check_error(&response, StatusCode::FORBIDDEN, DENIED);
        }
    }

    let response = get_subscriptions(&th.client2, &user2).await;
    response.check_ok();
    assert_eq!(response.text(), "[]", "no subscriptions are the empty list");
    assert_eq!(
        subscriptions_of(&th.client, &user1).await,
        [subscription],
        "the first user's subscription is still there"
    );
    th.teardown().await;
}

/// A subscription can be ended once.
#[tokio::test]
async fn delete_subscription_twice() {
    let th = Setup::native().start().await.init_basic().await;
    let me = th.client.me().await.id;
    let board = th.create_board(TEST_TEAM_ID, BoardType::Private).await;
    let card = insert_card(&th.client, &board.id).await;
    let not_found = format!(
        "{{subscription BlockID={} SubscriberID={me}}} not found",
        card.id
    );

    let response = unsubscribe(&th.client, &card.id, &me).await;
    check_error(&response, StatusCode::NOT_FOUND, &not_found);

    subscribe(&th.client, &card.id, &me).await;
    unsubscribe(&th.client, &card.id, &me).await.check_ok();
    let response = unsubscribe(&th.client, &card.id, &me).await;
    check_error(&response, StatusCode::NOT_FOUND, &not_found);

    th.teardown().await;
}

/// A subscription outlives its block and can still be ended then. The original ends
/// it too, but crashes afterwards, looking for the block's board to name a team for a
/// broadcast it does not send, and answers with a server error
/// (server/app/subscriptions.go:43-51).
#[tokio::test]
async fn delete_subscription_to_a_deleted_block() {
    let th = Setup::native().start().await.init_basic().await;
    let me = th.client.me().await.id;
    let board = th.create_board(TEST_TEAM_ID, BoardType::Private).await;
    let card = insert_card(&th.client, &board.id).await;
    let subscription = subscribe(&th.client, &card.id, &me).await;
    th.client
        .delete(&format!("/boards/{}/blocks/{}", board.id, card.id))
        .await
        .check_ok();

    assert_eq!(
        subscriptions_of(&th.client, &me).await,
        [subscription],
        "the subscription is still listed"
    );
    let response = th.client.post(PATH, &request(&card.id, &me)).await;
    check_error(
        &response,
        StatusCode::BAD_REQUEST,
        &format!("invalid blockID: {{block ID={}}} not found", card.id),
    );

    let response = unsubscribe(&th.client, &card.id, &me).await;
    response.check_ok();
    assert_eq!(response.text(), "{}", "the empty object");
    assert_eq!(subscriptions_of(&th.client, &me).await, [], "it is gone");
    th.teardown().await;
}

/// Single-user mode: the one user subscribes under its fixed identifier.
#[tokio::test]
async fn subscriptions_in_single_user_mode() {
    let th = Setup::single_user().start().await;
    let board = th.create_board(TEST_TEAM_ID, BoardType::Open).await;
    let card = insert_card(&th.client, &board.id).await;

    let subscription = subscribe(&th.client, &card.id, SINGLE_USER_ID).await;
    assert_eq!(subscription.subscriber_id, SINGLE_USER_ID, "the one user");
    let response = th.client.post(PATH, &request(&card.id, "someone")).await;
    check_error(
        &response,
        StatusCode::BAD_REQUEST,
        "userID and subscriberID mismatch",
    );
    assert_eq!(
        subscriptions_of(&th.client, SINGLE_USER_ID).await,
        [subscription]
    );
    let response = get_subscriptions(&th.client, "someone").await;
    check_error(&response, StatusCode::FORBIDDEN, DENIED);
    let response = get_subscriptions(&th.client_with_token("wrong"), SINGLE_USER_ID).await;
    check_error(
        &response,
        StatusCode::UNAUTHORIZED,
        "invalid single user token",
    );

    unsubscribe(&th.client, &card.id, SINGLE_USER_ID)
        .await
        .check_ok();
    assert_eq!(subscriptions_of(&th.client, SINGLE_USER_ID).await, []);
    th.teardown().await;
}

// -- who is told --------------------------------------------------------------------------

/// The standalone websocket server of the original sends nothing about subscriptions
/// (server/ws/server.go:785-787), and neither does this one.
#[tokio::test]
async fn subscription_changes_are_not_broadcast() {
    let th = Setup::native().start().await.init_basic().await;
    let me = th.client.me().await.id;
    let board = th.create_board(TEST_TEAM_ID, BoardType::Private).await;
    let card = insert_card(&th.client, &board.id).await;
    assert!(
        th.app.finish_background(BACKGROUND).await,
        "the card's broadcast is out before anyone listens"
    );
    let mut socket = TeamSocket::subscribe(&th, &th.client, TEST_TEAM_ID, &me).await;

    subscribe(&th.client, &card.id, &me).await;
    subscribe(&th.client, &card.id, &me).await;
    unsubscribe(&th.client, &card.id, &me).await.check_ok();

    assert!(
        th.app.finish_background(BACKGROUND).await,
        "background work finishes"
    );
    assert_eq!(socket.drain().await, [json!(null); 0], "nothing is said");
    th.teardown().await;
}

// -- security -----------------------------------------------------------------------------

/// In the original, every signed-in user can subscribe to any block whose identifier
/// they know, also on a board they cannot see. Here that takes the permission to view
/// the block's board.
#[tokio::test]
async fn subscription_to_a_block_of_a_board_the_user_cannot_view_is_refused() {
    let th = Setup::native().start().await.init_basic().await;
    let outsider = th.client2.me().await.id;
    let private_board = th.create_board(TEST_TEAM_ID, BoardType::Private).await;
    let open_board = th.create_board(TEST_TEAM_ID, BoardType::Open).await;

    for board in [&private_board, &open_board] {
        let card = insert_card(&th.client, &board.id).await;
        let response = th.client2.post(PATH, &request(&card.id, &outsider)).await;
        check_error(&response, StatusCode::FORBIDDEN, DENIED);
    }
    assert_eq!(
        subscriptions_of(&th.client2, &outsider).await,
        [],
        "nothing was stored"
    );

    // Whether a block exists is answered before, as in the original.
    let response = th.client2.post(PATH, &request("nope", &outsider)).await;
    check_error(
        &response,
        StatusCode::BAD_REQUEST,
        "invalid blockID: {block ID=nope} not found",
    );

    // Once they have joined the open board, they can subscribe to its cards.
    let card = insert_card(&th.client, &open_board.id).await;
    th.client2
        .post_empty(&format!("/boards/{}/join", open_board.id))
        .await
        .check_ok();
    let subscription = subscribe(&th.client2, &card.id, &outsider).await;
    assert_eq!(
        subscriptions_of(&th.client2, &outsider).await,
        [subscription]
    );
    th.teardown().await;
}

/// The check leaves alone what worked before: every member of a board, whatever their
/// role, subscribes to its blocks, and a user who is no member any more can still end
/// the subscription they have.
#[tokio::test]
async fn subscription_by_every_member_of_the_board_still_works() {
    let th = Setup::native().start().await.init_basic().await;
    let Roles {
        board,
        editor,
        commenter,
        viewer,
    } = board_with_roles(&th).await;
    let card = insert_card(&th.client, &board.id).await;
    let text = insert_block(&th.client, &new_block(&board.id, &card.id, BlockType::Text)).await;

    for client in [&th.client, &editor, &commenter, &viewer] {
        let user_id = client.me().await.id;
        let on_card = subscribe(client, &card.id, &user_id).await;
        let on_text = subscribe(client, &text.id, &user_id).await;
        assert_eq!(
            subscriptions_of(client, &user_id).await,
            sorted(vec![on_card, on_text]),
            "the member's subscriptions"
        );
    }

    let viewer_id = viewer.me().await.id;
    th.client
        .delete(&format!("/boards/{}/members/{viewer_id}", board.id))
        .await
        .check_ok();
    let response = viewer.post(PATH, &request(&card.id, &viewer_id)).await;
    check_error(&response, StatusCode::FORBIDDEN, DENIED);
    unsubscribe(&viewer, &card.id, &viewer_id).await.check_ok();
    unsubscribe(&viewer, &text.id, &viewer_id).await.check_ok();
    assert_eq!(subscriptions_of(&viewer, &viewer_id).await, [], "all ended");
    th.teardown().await;
}
