//! Subscriptions to blocks.
//! Port of `server/integrationtests/subscriptions_test.go`, plus tests of what
//! `server/api/subscriptions.go` decides and the original leaves untested: the
//! validation with its order and messages, subscribing twice, whose subscriptions a
//! user may read and end, and that nobody is told. The expectations of those were
//! observed on the original, except where a test says that the port differs.
//!
//! The subtests of the original share one server; here every subtest has its own.

use std::time::Duration;

use fvoci_model::{Block, BlockType, Board, BoardMember, BoardType, SubscriberType, Subscription};
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

use crate::board_broadcasts::WorkspaceSocket;
use crate::cards::{Roles, board_with_roles, check_error, insert_block, new_block};
use crate::common::TEST_WS;
use crate::common::{ApiResponse, Client, Setup, TEST_WORKSPACE_ID, UNKNOWN_ID};

/// How long background notifications may take to finish.
const BACKGROUND: Duration = Duration::from_secs(10);

const PATH: &str = "/w/00000000-0000-7000-8000-000000000001/subscriptions";

/// What a block the user cannot read answers, as one that does not exist.
const NO_BLOCK: &str = "block not found";

/// What ending a subscription the user does not have answers.
const NO_SUBSCRIPTION: &str = "subscription not found";

/// The refusal to subscribe somebody else.
const NOT_FOR_OTHERS: &str = "a user subscribes for themselves only";

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
        .created_json()
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
        workspace_id: TEST_WORKSPACE_ID.to_owned(),
        board_type: BoardType::Open,
        create_at: 1,
        update_at: 1,
        ..Board::default()
    };
    let board: Board = client.create_board(&board).await.created_json();

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
        let created: Subscription = client.post(PATH, &subscription).await.created_json();
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

    // Everything it lacks is told at once.
    check_error(
        &response,
        StatusCode::BAD_REQUEST,
        "3 members of the request were refused",
    );
    assert_eq!(
        response.refused_places(),
        ["/blockId", "/blockType", "/subscriberType"]
    );
    th.teardown().await;
}

/// `TestCreateSubscription/"Create subscription for another user"`
#[tokio::test]
async fn create_subscription_create_subscription_for_another_user() {
    let th = Setup::native().start().await.init_basic().await;
    let other = th.client2.me().await;

    let response = th
        .client
        .post(
            PATH,
            &json!({
                "blockType": "",
                "blockId": "",
                "subscriberType": "",
                "subscriberId": other.id,
                "createAt": 0,
                "deleteAt": 0,
            }),
        )
        .await;

    // As in the original, the subscription is refused for what it lacks before the
    // subscriber is compared with the session's user.
    check_error(
        &response,
        StatusCode::BAD_REQUEST,
        "3 members of the request were refused",
    );
    assert_eq!(
        response.refused_places(),
        ["/blockId", "/blockType", "/subscriberType"]
    );
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
    response.check_no_content();

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

    for block_id in [UNKNOWN_ID, "bogus"] {
        let response = unsubscribe(&th.client, block_id, &user.id).await;
        check_error(&response, StatusCode::NOT_FOUND, NO_SUBSCRIPTION);
    }
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
    let board = th.create_board(TEST_WORKSPACE_ID, BoardType::Private).await;
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

    let all_null = json!({
        "blockType": null,
        "blockId": null,
        "subscriberType": null,
        "subscriberId": null,
    });
    let bad_requests = [
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
        // The original also takes `channel`, for the channels of its host application.
        (
            with("subscriberType", json!("channel")),
            "invalid subscriber type",
        ),
        (with("blockId", json!("")), "missing block id"),
        // Nothing at all, also as `null` members: everything missing is reported.
        (json!({}), "4 members of the request were refused"),
        (all_null, "4 members of the request were refused"),
        (
            json!({"subscriberId": other}),
            "3 members of the request were refused",
        ),
    ];
    for (body, message) in &bad_requests {
        let response = th.client.post(PATH, body).await;
        check_error(&response, StatusCode::BAD_REQUEST, message);
    }
    let response = th.client.post(PATH, &json!({})).await;
    assert_eq!(
        response.refused_places(),
        ["/blockId", "/blockType", "/subscriberId", "/subscriberType"],
        "each member that is missing, the type after the others"
    );

    // Complete, but for someone else; whether the block exists is not looked at.
    for block_id in [card.id.as_str(), UNKNOWN_ID, "nope"] {
        let response = th.client.post(PATH, &request(block_id, &other)).await;
        check_error(&response, StatusCode::FORBIDDEN, NOT_FOR_OTHERS);
    }
    // A block that does not exist. Boards are no blocks.
    for block_id in [UNKNOWN_ID, "nope", board.id.as_str()] {
        let response = th.client.post(PATH, &request(block_id, &me)).await;
        check_error(&response, StatusCode::NOT_FOUND, NO_BLOCK);
    }

    // A body that is no JSON object, or whose members have the wrong types, is a bad
    // request. (The original answers 500.)
    let block_id_number = with("blockId", json!(5)).to_string();
    let time_text = with("notifiedAt", json!("now")).to_string();
    for body in [
        "{\"blockId\":",
        "",
        "[]",
        "null",
        "\"card\"",
        &block_id_number,
        &time_text,
    ] {
        let response = th.client.send_raw_body(Method::POST, PATH, body).await;
        response.check_bad_request();
        assert_eq!(response.problem_type(), "/problems/bad-request", "{body}");
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
    let board = th.create_board(TEST_WORKSPACE_ID, BoardType::Private).await;
    let card = insert_card(&th.client, &board.id).await;
    let before = fvoci_server::auth::now_millis();

    // The times are the server's, whatever the request says.
    let mut body = request(&card.id, &me);
    body["notifiedAt"] = json!(5);
    body["createAt"] = json!(6);
    body["deleteAt"] = json!(7);
    let response = th.client.post(PATH, &body).await;
    let first: Subscription = response.created_json();
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

    // Subscribing again answers the subscription there is: it keeps its creation time
    // and is marked as notified now.
    tokio::time::sleep(Duration::from_millis(5)).await;
    let second = subscribe(&th.client, &card.id, &me).await;
    assert_eq!(second.create_at, first.create_at, "the same subscription");
    assert!(second.notified_at > first.notified_at, "notified now");
    assert_eq!(
        subscriptions_of(&th.client, &me).await,
        std::slice::from_ref(&second),
        "what is stored is what was answered"
    );

    // Unsubscribing ends the subscription; subscribing after that starts a new one.
    unsubscribe(&th.client, &card.id, &me)
        .await
        .check_no_content();
    assert_eq!(subscriptions_of(&th.client, &me).await, [], "unsubscribed");
    tokio::time::sleep(Duration::from_millis(5)).await;
    let third = subscribe(&th.client, &card.id, &me).await;
    assert!(third.create_at > first.create_at, "created anew");
    assert_eq!(
        subscriptions_of(&th.client, &me).await,
        std::slice::from_ref(&third),
        "what is stored is what was answered"
    );

    // The block type is whatever the client says, also another one than the block's.
    for (block_type, subscriber_type) in [("text", "user"), ("whatever", "user")] {
        let card = insert_card(&th.client, &board.id).await;
        let mut body = request(&card.id, &me);
        body["blockType"] = json!(block_type);
        body["subscriberType"] = json!(subscriber_type);
        let response = th.client.post(PATH, &body).await;
        response.check_created();
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
    let board = th.create_board(TEST_WORKSPACE_ID, BoardType::Private).await;
    let card = insert_card(&th.client, &board.id).await;
    let subscription = subscribe(&th.client, &card.id, &user1).await;

    let response = get_subscriptions(&th.anonymous(), &user1).await;
    response.check_unauthorized();
    let response = unsubscribe(&th.anonymous(), &card.id, &user1).await;
    response.check_unauthorized();

    // The subscriptions of another user are nobody's business: the answer is the same
    // for a user who has some, a user who has none, and nobody at all.
    for subscriber_id in [user1.as_str(), UNKNOWN_ID, "nobody"] {
        let response = get_subscriptions(&th.client2, subscriber_id).await;
        check_error(&response, StatusCode::NOT_FOUND, "subscriber not found");
        for block_id in [card.id.as_str(), UNKNOWN_ID, "nope"] {
            let response = unsubscribe(&th.client2, block_id, subscriber_id).await;
            check_error(&response, StatusCode::NOT_FOUND, NO_SUBSCRIPTION);
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
    let board = th.create_board(TEST_WORKSPACE_ID, BoardType::Private).await;
    let card = insert_card(&th.client, &board.id).await;
    let response = unsubscribe(&th.client, &card.id, &me).await;
    check_error(&response, StatusCode::NOT_FOUND, NO_SUBSCRIPTION);

    subscribe(&th.client, &card.id, &me).await;
    unsubscribe(&th.client, &card.id, &me)
        .await
        .check_no_content();
    let response = unsubscribe(&th.client, &card.id, &me).await;
    check_error(&response, StatusCode::NOT_FOUND, NO_SUBSCRIPTION);

    th.teardown().await;
}

/// A subscription ends with its block: it is not listed any more and there is nothing
/// left to end. Restoring the block does not bring the subscription back.
#[tokio::test]
async fn a_subscription_ends_with_its_block() {
    let th = Setup::native().start().await.init_basic().await;
    let me = th.client.me().await.id;
    let board = th.create_board(TEST_WORKSPACE_ID, BoardType::Private).await;
    let card = insert_card(&th.client, &board.id).await;
    subscribe(&th.client, &card.id, &me).await;
    let block_path = format!("{TEST_WS}/boards/{}/blocks/{}", board.id, card.id);
    th.client.delete(&block_path).await.check_no_content();

    assert_eq!(
        subscriptions_of(&th.client, &me).await,
        [],
        "the subscription went with the block"
    );
    let response = th.client.post(PATH, &request(&card.id, &me)).await;
    check_error(&response, StatusCode::NOT_FOUND, NO_BLOCK);
    let response = unsubscribe(&th.client, &card.id, &me).await;
    check_error(&response, StatusCode::NOT_FOUND, NO_SUBSCRIPTION);

    th.client
        .post_empty(&format!("{block_path}/undelete"))
        .await
        .check_ok();
    assert_eq!(
        subscriptions_of(&th.client, &me).await,
        [],
        "and does not come back with it"
    );
    th.teardown().await;
}

// -- who is told --------------------------------------------------------------------------

/// The standalone websocket server of the original sends nothing about subscriptions
/// (server/ws/server.go:785-787), and neither does this one.
#[tokio::test]
async fn subscription_changes_are_not_broadcast() {
    let th = Setup::native().start().await.init_basic().await;
    let me = th.client.me().await.id;
    let board = th.create_board(TEST_WORKSPACE_ID, BoardType::Private).await;
    let card = insert_card(&th.client, &board.id).await;
    assert!(
        th.app.finish_background(BACKGROUND).await,
        "the card's broadcast is out before anyone listens"
    );
    let mut socket = WorkspaceSocket::subscribe(&th, &th.client, TEST_WORKSPACE_ID, &me).await;

    subscribe(&th.client, &card.id, &me).await;
    subscribe(&th.client, &card.id, &me).await;
    unsubscribe(&th.client, &card.id, &me)
        .await
        .check_no_content();

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
    let private_board = th.create_board(TEST_WORKSPACE_ID, BoardType::Private).await;
    let open_board = th.create_board(TEST_WORKSPACE_ID, BoardType::Open).await;

    // The request names a block and no board, so a block the user cannot read answers
    // as one that does not exist: nothing is said about the blocks of other boards.
    for board in [&private_board, &open_board] {
        let card = insert_card(&th.client, &board.id).await;
        let response = th.client2.post(PATH, &request(&card.id, &outsider)).await;
        check_error(&response, StatusCode::NOT_FOUND, NO_BLOCK);
    }
    assert_eq!(
        subscriptions_of(&th.client2, &outsider).await,
        [],
        "nothing was stored"
    );

    for block_id in [UNKNOWN_ID, "nope"] {
        let response = th.client2.post(PATH, &request(block_id, &outsider)).await;
        check_error(&response, StatusCode::NOT_FOUND, NO_BLOCK);
    }

    // Once they have joined the open board, they can subscribe to its cards.
    let card = insert_card(&th.client, &open_board.id).await;
    th.client2
        .post_empty(&format!("{TEST_WS}/boards/{}/join", open_board.id))
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
/// role, subscribes to its blocks. A user who is removed from the board cannot subscribe
/// any more, and has no subscription left to end.
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
    let text = insert_block(
        &th.client,
        &new_block(&board.id, &card.id, BlockType::Attachment),
    )
    .await;

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
        .delete(&format!(
            "{TEST_WS}/boards/{}/members/{viewer_id}",
            board.id
        ))
        .await
        .check_no_content();
    let response = viewer.post(PATH, &request(&card.id, &viewer_id)).await;
    check_error(&response, StatusCode::NOT_FOUND, NO_BLOCK);
    assert_eq!(
        subscriptions_of(&viewer, &viewer_id).await,
        [],
        "ended with the membership"
    );
    for block in [&card, &text] {
        unsubscribe(&viewer, &block.id, &viewer_id)
            .await
            .check_not_found();
    }
    th.teardown().await;
}

/// Not in the original, where a subscription outlives the membership it was made with
/// (the user keeps a standing request to be told about a board they can no longer see).
/// A member who leaves or is removed loses the subscriptions to that board's blocks.
/// Subscriptions on other boards, and those of members who stay, are untouched.
#[tokio::test]
async fn leaving_or_being_removed_from_a_board_ends_the_subscriptions_to_its_blocks() {
    let th = Setup::native().start().await.init_basic().await;
    let admin_id = th.client.me().await.id;
    let user2_id = th.client2.me().await.id;
    let mut cards = Vec::new();
    let mut boards = Vec::new();
    for _ in 0..2 {
        let board = th.create_board(TEST_WORKSPACE_ID, BoardType::Private).await;
        let member = BoardMember {
            board_id: board.id.clone(),
            user_id: user2_id.clone(),
            scheme_editor: true,
            ..BoardMember::default()
        };
        th.client.add_member(&member).await.check_ok();
        cards.push(insert_card(&th.client, &board.id).await);
        boards.push(board);
    }
    let on_first = subscribe(&th.client2, &cards[0].id, &user2_id).await;
    let on_second = subscribe(&th.client2, &cards[1].id, &user2_id).await;
    let admins = subscribe(&th.client, &cards[0].id, &admin_id).await;

    // A removal that is refused ends nothing: an editor may not remove a member.
    th.client2
        .delete(&format!(
            "{TEST_WS}/boards/{}/members/{admin_id}",
            boards[0].id
        ))
        .await
        .check_forbidden();
    assert_eq!(
        subscriptions_of(&th.client, &admin_id).await,
        std::slice::from_ref(&admins),
        "the subscription of the member who was not removed"
    );

    // Removed by the admin.
    th.client
        .delete(&format!(
            "{TEST_WS}/boards/{}/members/{user2_id}",
            boards[0].id
        ))
        .await
        .check_no_content();
    assert_eq!(
        subscriptions_of(&th.client2, &user2_id).await,
        [on_second],
        "only the subscription on the other board is left"
    );
    assert_eq!(
        subscriptions_of(&th.client, &admin_id).await,
        [admins],
        "the subscription of a member who stays"
    );
    let _ = on_first;

    // Leaving by oneself.
    th.client2
        .post_empty(&format!("{TEST_WS}/boards/{}/leave", boards[1].id))
        .await
        .check_no_content();
    assert_eq!(
        subscriptions_of(&th.client2, &user2_id).await,
        [],
        "nothing is left after leaving the other board too"
    );

    // A member again, the user subscribes again.
    let member = BoardMember {
        board_id: boards[0].id.clone(),
        user_id: user2_id.clone(),
        scheme_viewer: true,
        ..BoardMember::default()
    };
    th.client.add_member(&member).await.check_ok();
    subscribe(&th.client2, &cards[0].id, &user2_id).await;
    // The revived subscription keeps its first creation time, so compare the blocks.
    let subscribed: Vec<String> = subscriptions_of(&th.client2, &user2_id)
        .await
        .into_iter()
        .map(|subscription| subscription.block_id)
        .collect();
    assert_eq!(
        subscribed,
        [cards[0].id.clone()],
        "subscribed again as a member"
    );
    th.teardown().await;
}
