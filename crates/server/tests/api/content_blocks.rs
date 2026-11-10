//! Moving a content block within its card.
//! Ports of `server/integrationtests/content_blocks_test.go` and
//! `server/app/content_blocks_test.go`, plus tests of what the route decides and the
//! original leaves untested: the refusals with their order and messages, what happens
//! to entries of the content order that are no block identifiers, and who is told.
//! The expectations of those were observed on the original.
//!
//! The cases of the original's `TestMoveContentBlock` share one server and build on
//! each other; here every case is a test of its own that first repeats the moves
//! before it.
//!
//! Not ported from `server/app/content_blocks_test.go`:
//! - `TestMoveContentBlock/"valid request but fail on patchparent with content order"`:
//!   a failure injected into the store through a mock, between reading the card and
//!   patching it. The real store has no failure to provoke there.

use std::time::Duration;

use fvoci_model::{Block, BlockType, Board, BoardType};
use fvoci_server::app::MovePosition;
use fvoci_server::error::{AppError, ErrorKind};
use reqwest::StatusCode;
use serde_json::{Value, json};

use crate::boards::TeamSocket;
use crate::cards::{
    Roles, board_with_roles, check_error, check_internal_error, insert_block, new_block,
};
use crate::common::{ApiResponse, Client, Setup, TEST_TEAM_ID, TestServer, UNKNOWN_ID};

/// How long background notifications may take to finish.
const BACKGROUND: Duration = Duration::from_secs(10);

/// The refusal for a user who may not manage the cards of a board.
const DENIED: &str = "access denied to modify board cards";

// -- helpers --------------------------------------------------------------------------

/// `MoveContentBlock`
async fn move_block(client: &Client, block_id: &str, position: &str, dst_id: &str) -> ApiResponse {
    client
        .post_empty(&format!(
            "/content-blocks/{block_id}/moveto/{position}/{dst_id}"
        ))
        .await
}

/// `MoveContentBlock`, expecting success.
async fn move_block_ok(client: &Client, block: &Block, position: &str, dst: &Block) {
    let response = move_block(client, &block.id, position, &dst.id).await;
    response.check_no_content();
}

/// The content order of a card as it is stored.
async fn content_order(th: &TestServer, card_id: &str) -> Value {
    let card = th.app.block(card_id).await.expect("the card is stored");
    card.fields.get("contentOrder").cloned().unwrap_or_default()
}

/// A card block with the given fields.
fn card_block(board_id: &str, fields: Value) -> Block {
    with_fields(new_block(board_id, "", BlockType::Card), fields)
}

fn with_fields(block: Block, fields: Value) -> Block {
    Block {
        fields: serde_json::from_value(fields).expect("the fields are a JSON object"),
        ..block
    }
}

fn text_block(board_id: &str, parent_id: &str) -> Block {
    new_block(board_id, parent_id, BlockType::Text)
}

/// Inserts a card whose content order is `texts` texts of its own, in one request so
/// that the identifiers in the order follow the blocks to their new identifiers.
/// Returns the card and the texts as stored.
async fn card_with_texts(client: &Client, board_id: &str, texts: usize) -> (Block, Vec<Block>) {
    let card = card_block(board_id, json!({}));
    let texts: Vec<Block> = (0..texts).map(|_| text_block(board_id, &card.id)).collect();
    let order: Vec<&str> = texts.iter().map(|block| block.id.as_str()).collect();
    let mut blocks = vec![with_fields(card, json!({"contentOrder": order}))];
    blocks.extend(texts);

    let mut inserted: Vec<Block> = client.insert_blocks(board_id, &blocks).await.created_json();
    let card = inserted.remove(0);
    (card, inserted)
}

fn ids(blocks: &[&Block]) -> Value {
    blocks.iter().map(|block| json!(block.id)).collect()
}

/// The blocks of the original's `TestMoveContentBlock`: two cards with three content
/// blocks each, of one user.
struct TwoCards {
    th: TestServer,
    card1: Block,
    /// The content blocks 1 to 3 of the first card and 4 to 6 of the second.
    content: Vec<Block>,
}

async fn two_cards() -> TwoCards {
    let th = Setup::one_user().start().await;
    let board = th.create_board(TEST_TEAM_ID, BoardType::Open).await;

    let card1 = card_block(&board.id, json!({}));
    let card2 = card_block(&board.id, json!({}));
    // As in the original, the content blocks are of type `card` themselves.
    let content: Vec<Block> = [&card1, &card1, &card1, &card2, &card2, &card2]
        .into_iter()
        .map(|card| new_block(&board.id, &card.id, BlockType::Card))
        .collect();
    let card1 = with_fields(
        card1,
        json!({"contentOrder": [content[0].id, content[1].id, content[2].id]}),
    );
    let card2 = with_fields(
        card2,
        json!({"contentOrder": [content[3].id, content[4].id, content[5].id]}),
    );

    let mut new_blocks = content;
    new_blocks.extend([card1, card2]);
    let mut created: Vec<Block> = th
        .client
        .insert_blocks(&board.id, &new_blocks)
        .await
        .created_json();
    assert_eq!(created.len(), 8, "all blocks were created");
    created.pop().expect("the second card");
    let card1 = created.pop().expect("the first card");
    TwoCards {
        th,
        card1,
        content: created,
    }
}

impl TwoCards {
    /// The moves of the original's cases, in their order: (source, where, destination)
    /// as indices into the content blocks.
    const MOVES: [(usize, &'static str, usize); 4] = [
        (0, "after", 3),
        (1, "after", 0),
        (1, "before", 0),
        (0, "after", 1),
    ];

    /// Runs the first `count` cases of the original and returns the answer to the
    /// last one.
    async fn run_cases(&self, count: usize) -> ApiResponse {
        let mut last = None;
        for (source, position, destination) in &Self::MOVES[..count] {
            let response = move_block(
                &self.th.client,
                &self.content[*source].id,
                position,
                &self.content[*destination].id,
            )
            .await;
            last = Some(response);
        }
        last.expect("at least one case runs")
    }

    /// Checks the content order of the first card, given as indices into the content
    /// blocks.
    async fn check_order(&self, expected: [usize; 3]) {
        let expected: Vec<&Block> = expected.iter().map(|index| &self.content[*index]).collect();
        assert_eq!(
            content_order(&self.th, &self.card1.id).await,
            ids(&expected),
            "the content order of the first card"
        );
    }
}

/// The refusal to move a block next to one that is content of another card.
const NOT_SAME_CARD: &str = "the two blocks are not content of the same card";

// -- server/integrationtests/content_blocks_test.go ---------------------------------------

/// `TestMoveContentBlock/"not matching parents"`
#[tokio::test]
async fn move_content_block_not_matching_parents() {
    let cards = two_cards().await;

    let response = cards.run_cases(1).await;

    check_error(&response, StatusCode::BAD_REQUEST, NOT_SAME_CARD);
    cards.check_order([0, 1, 2]).await;
    cards.th.teardown().await;
}

/// `TestMoveContentBlock/"valid request with not real change"`
#[tokio::test]
async fn move_content_block_valid_request_with_not_real_change() {
    let cards = two_cards().await;

    let response = cards.run_cases(2).await;

    response.check_no_content();
    cards.check_order([0, 1, 2]).await;
    cards.th.teardown().await;
}

/// `TestMoveContentBlock/"valid request changing order with before"`
#[tokio::test]
async fn move_content_block_valid_request_changing_order_with_before() {
    let cards = two_cards().await;

    let response = cards.run_cases(3).await;

    response.check_no_content();
    cards.check_order([1, 0, 2]).await;
    cards.th.teardown().await;
}

/// `TestMoveContentBlock/"valid request changing order with after"`
#[tokio::test]
async fn move_content_block_valid_request_changing_order_with_after() {
    let cards = two_cards().await;

    let response = cards.run_cases(4).await;

    response.check_no_content();
    cards.check_order([1, 0, 2]).await;
    cards.th.teardown().await;
}

// -- server/app/content_blocks_test.go, through the real store ----------------------------

/// A block that exists only as a value: the application function takes the blocks to
/// move as its caller read them. Nothing is stored under its identifier, which only has
/// to be found in the content order of the card.
fn loose_block(id: &str, parent_id: &str) -> Block {
    Block {
        id: id.to_owned(),
        parent_id: parent_id.to_owned(),
        ..Block::default()
    }
}

/// Stores a card with the given fields and moves the loose block `source` next to the
/// loose block `destination`, both children of the card. Returns the outcome and the
/// card's content order afterwards.
async fn app_move(
    th: &TestServer,
    fields: Value,
    (source, position, destination): (&str, MovePosition, &str),
) -> (Result<(), AppError>, Value) {
    let board = th.create_board(TEST_TEAM_ID, BoardType::Open).await;
    let card = insert_block(&th.client, &card_block(&board.id, fields)).await;
    let outcome = th
        .app
        .move_content_block(
            &loose_block(source, &card.id),
            &loose_block(destination, &card.id),
            position,
            &th.stranger().await,
        )
        .await;
    (outcome, content_order(th, &card.id).await)
}

/// Checks that a move was refused as a bad request with this message.
#[track_caller]
fn check_refused(outcome: Result<(), AppError>, message: &str) {
    let error = outcome.expect_err("the move is refused");
    assert_eq!(error.kind(), ErrorKind::BadRequest, "{error:?}");
    assert_eq!(error.to_string(), message, "the refusal");
}

/// `TestMoveContentBlock/"not matching parents"` (app)
#[tokio::test]
async fn app_move_content_block_not_matching_parents() {
    let th = Setup::one_user().start().await;

    let outcome = th
        .app
        .move_content_block(
            &loose_block("test-1", "test-card"),
            &loose_block("test-2", "other-test-card"),
            MovePosition::After,
            &th.stranger().await,
        )
        .await;

    check_refused(outcome, NOT_SAME_CARD);
    th.teardown().await;
}

/// `TestMoveContentBlock/"parent not found"` (app)
#[tokio::test]
async fn app_move_content_block_parent_not_found() {
    let th = Setup::one_user().start().await;

    for card_id in [UNKNOWN_ID, "invalid-card"] {
        let error = th
            .app
            .move_content_block(
                &loose_block("test-1", card_id),
                &loose_block("test-2", card_id),
                MovePosition::After,
                &th.stranger().await,
            )
            .await
            .expect_err("the parent does not exist");

        assert!(error.is_not_found(), "{card_id}: {error:?}");
        assert_eq!(error.to_string(), "block not found", "{card_id}");
    }
    th.teardown().await;
}

/// `TestMoveContentBlock/"valid parent without content order"` (app)
#[tokio::test]
async fn app_move_content_block_valid_parent_without_content_order() {
    let th = Setup::one_user().start().await;

    let (outcome, order) =
        app_move(&th, json!({}), ("test-1", MovePosition::After, "test-2")).await;

    check_refused(
        outcome,
        "block test-1 is not in the content order of its card",
    );
    assert_eq!(order, json!(null), "the card still has no content order");
    th.teardown().await;
}

/// `TestMoveContentBlock/"valid parent with content order but without test-1 in it"`
/// (app)
#[tokio::test]
async fn app_move_content_block_valid_parent_with_content_order_but_without_test_1_in_it() {
    let th = Setup::one_user().start().await;

    let (outcome, order) = app_move(
        &th,
        json!({"contentOrder": ["test-2"]}),
        ("test-1", MovePosition::After, "test-2"),
    )
    .await;

    check_refused(
        outcome,
        "block test-1 is not in the content order of its card",
    );
    assert_eq!(order, json!(["test-2"]), "the order is as it was");
    th.teardown().await;
}

/// `TestMoveContentBlock/"valid parent with content order but without test-2 in it"`
/// (app)
#[tokio::test]
async fn app_move_content_block_valid_parent_with_content_order_but_without_test_2_in_it() {
    let th = Setup::one_user().start().await;

    let (outcome, order) = app_move(
        &th,
        json!({"contentOrder": ["test-1"]}),
        ("test-1", MovePosition::After, "test-2"),
    )
    .await;

    check_refused(
        outcome,
        "block test-2 is not in the content order of its card",
    );
    assert_eq!(order, json!(["test-1"]), "the order is as it was");
    th.teardown().await;
}

/// `TestMoveContentBlock/"valid request with not real change"` (app)
#[tokio::test]
async fn app_move_content_block_valid_request_with_not_real_change() {
    let th = Setup::one_user().start().await;

    let (outcome, order) = app_move(
        &th,
        json!({"contentOrder": ["test-1", "test-2", "test-3"]}),
        ("test-2", MovePosition::After, "test-1"),
    )
    .await;

    outcome.expect("the move is made");
    assert_eq!(order, json!(["test-1", "test-2", "test-3"]), "as before");
    th.teardown().await;
}

/// `TestMoveContentBlock/"valid request changing order with before"` (app)
#[tokio::test]
async fn app_move_content_block_valid_request_changing_order_with_before() {
    let th = Setup::one_user().start().await;

    let (outcome, order) = app_move(
        &th,
        json!({"contentOrder": ["test-1", "test-2", "test-3"]}),
        ("test-2", MovePosition::Before, "test-1"),
    )
    .await;

    outcome.expect("the move is made");
    assert_eq!(order, json!(["test-2", "test-1", "test-3"]), "2 before 1");
    th.teardown().await;
}

/// `TestMoveContentBlock/"valid request changing order with after"` (app)
#[tokio::test]
async fn app_move_content_block_valid_request_changing_order_with_after() {
    let th = Setup::one_user().start().await;

    let (outcome, order) = app_move(
        &th,
        json!({"contentOrder": ["test-1", "test-2", "test-3"]}),
        ("test-1", MovePosition::After, "test-2"),
    )
    .await;

    outcome.expect("the move is made");
    assert_eq!(order, json!(["test-2", "test-1", "test-3"]), "1 after 2");
    th.teardown().await;
}

// -- POST /content-blocks/{block_id}/moveto/{where}/{dst_block_id} ------------------------

/// What the route refuses, in the order the checks are made: both blocks are looked
/// up, then `where` is checked, then the permission, then the move itself.
#[tokio::test]
async fn move_content_block_refusals() {
    let th = Setup::native().start().await.init_basic().await;
    let Roles {
        board,
        editor,
        commenter,
        viewer,
    } = board_with_roles(&th).await;
    let outsider = &th.client2;
    let (card, texts) = card_with_texts(&th.client, &board.id, 3).await;
    let (a, b) = (&texts[0], &texts[1]);
    let (other_card, other_texts) = card_with_texts(&th.client, &board.id, 1).await;
    let original_order = ids(&[a, b, &texts[2]]);

    let response = move_block(&th.anonymous(), &a.id, "after", &b.id).await;
    response.check_unauthorized();

    // A block that does not exist, for everyone and before anything else. For who
    // cannot read the board, its blocks do not exist either, whatever else is asked.
    let missing = "block not found";
    for (block_id, position, dst_id) in [
        (UNKNOWN_ID, "after", b.id.as_str()),
        ("nope", "after", b.id.as_str()),
        (a.id.as_str(), "after", UNKNOWN_ID),
        (a.id.as_str(), "after", "nope"),
        (UNKNOWN_ID, "after", "nope"),
        ("nope", "after", "nope2"),
        ("nope", "sideways", b.id.as_str()),
    ] {
        let response = move_block(&th.client, block_id, position, dst_id).await;
        check_error(&response, StatusCode::NOT_FOUND, missing);
    }
    for (block_id, position, dst_id) in [
        (UNKNOWN_ID, "after", b.id.as_str()),
        ("nope", "after", b.id.as_str()),
        (a.id.as_str(), "after", b.id.as_str()),
        (a.id.as_str(), "after", UNKNOWN_ID),
        (a.id.as_str(), "after", "nope"),
        (a.id.as_str(), "sideways", b.id.as_str()),
        (a.id.as_str(), "after", other_texts[0].id.as_str()),
    ] {
        let response = move_block(outsider, block_id, position, dst_id).await;
        check_error(&response, StatusCode::NOT_FOUND, missing);
    }

    // The permission to manage the cards of the moved block's board, before `where`.
    for client in [&commenter, &viewer] {
        for (position, dst) in [("after", b), ("after", &other_texts[0]), ("sideways", b)] {
            let response = move_block(client, &a.id, position, &dst.id).await;
            check_error(&response, StatusCode::FORBIDDEN, DENIED);
        }
    }

    // `where`.
    for position in ["sideways", "AFTER", "Before", "%20after"] {
        let response = move_block(&th.client, &a.id, position, &b.id).await;
        check_error(
            &response,
            StatusCode::BAD_REQUEST,
            "invalid where parameter, use before or after",
        );
    }

    // The move itself.
    let response = move_block(&editor, &a.id, "after", &other_texts[0].id).await;
    check_error(&response, StatusCode::BAD_REQUEST, NOT_SAME_CARD);
    // A card has no parent, so it is content of no card.
    let response = move_block(&editor, &a.id, "after", &card.id).await;
    check_error(&response, StatusCode::BAD_REQUEST, NOT_SAME_CARD);
    let response = move_block(&editor, &card.id, "after", &other_card.id).await;
    check_error(
        &response,
        StatusCode::BAD_REQUEST,
        "the block is not content of a card",
    );

    // A block is not moved next to itself.
    for position in ["after", "before"] {
        let response = move_block(&editor, &a.id, position, &a.id).await;
        check_error(
            &response,
            StatusCode::BAD_REQUEST,
            "a block cannot be moved next to itself",
        );
    }
    // A child of the card that is not in its content order, as source and as
    // destination.
    let stray = insert_block(&th.client, &text_block(&board.id, &card.id)).await;
    let response = move_block(&editor, &stray.id, "after", &a.id).await;
    check_error(
        &response,
        StatusCode::BAD_REQUEST,
        &format!("block {} is not in the content order of its card", stray.id),
    );
    let response = move_block(&editor, &a.id, "after", &stray.id).await;
    check_error(
        &response,
        StatusCode::BAD_REQUEST,
        &format!("block {} is not in the content order of its card", stray.id),
    );
    assert_eq!(
        content_order(&th, &card.id).await,
        original_order,
        "no refused move changed the order"
    );

    th.teardown().await;
}

/// A card without a content order has no block to move, and one whose content order is
/// no list fails the move (the original crashes on it, which its clients see as the
/// same server error).
#[tokio::test]
async fn move_content_block_on_a_card_without_a_list_of_content() {
    let th = Setup::native().start().await.init_basic().await;
    let board = th.create_board(TEST_TEAM_ID, BoardType::Private).await;

    for (fields, is_server_error) in [(json!({}), false), (json!({"contentOrder": "x"}), true)] {
        let card = insert_block(&th.client, &card_block(&board.id, fields.clone())).await;
        let c = insert_block(&th.client, &text_block(&board.id, &card.id)).await;
        let d = insert_block(&th.client, &text_block(&board.id, &card.id)).await;
        let response = move_block(&th.client, &c.id, "after", &d.id).await;
        if is_server_error {
            check_internal_error(&response);
        } else {
            check_error(
                &response,
                StatusCode::BAD_REQUEST,
                &format!("block {} is not in the content order of its card", c.id),
            );
        }
        assert_eq!(
            th.app.block(&card.id).await.expect("the card").fields,
            serde_json::from_value(fields).expect("the fields are a JSON object"),
            "the card is as it was"
        );
    }

    th.teardown().await;
}

/// What a move does to the content order, and to the card.
#[tokio::test]
async fn move_content_block_changes_the_order_of_the_card() {
    let th = Setup::native().start().await.init_basic().await;
    let Roles { board, editor, .. } = board_with_roles(&th).await;
    let admin_id = th.client.me().await.id;
    let editor_id = editor.me().await.id;
    let (card, texts) = card_with_texts(&th.client, &board.id, 3).await;
    let (a, b, c) = (&texts[0], &texts[1], &texts[2]);
    let stored = async || th.app.block(&card.id).await.expect("the card is stored");
    let before = stored().await;
    assert_eq!(before.modified_by, admin_id, "the card is the admin's");

    move_block_ok(&th.client, a, "after", b).await;
    assert_eq!(content_order(&th, &card.id).await, ids(&[b, a, c]));
    move_block_ok(&th.client, a, "before", b).await;
    assert_eq!(content_order(&th, &card.id).await, ids(&[a, b, c]));
    move_block_ok(&editor, c, "before", a).await;
    assert_eq!(content_order(&th, &card.id).await, ids(&[c, a, b]));
    let moved = stored().await;
    assert_eq!(moved.modified_by, editor_id, "modified by who moved");
    assert!(moved.update_at >= before.update_at, "and modified now");

    // A move that leaves the order as it is patches the card all the same.
    move_block_ok(&th.client, a, "after", c).await;
    assert_eq!(content_order(&th, &card.id).await, ids(&[c, a, b]));
    assert_eq!(stored().await.modified_by, admin_id, "patched by the admin");

    // Entries that are no block identifiers keep their place, blocks inside them are
    // not found, and every entry of the moved block and of the destination counts.
    th.client
        .patch(
            &format!("/boards/{}/blocks/{}", board.id, card.id),
            &json!({"updatedFields": {
                "contentOrder": [a.id, [b.id, "x"], 5, a.id, c.id, null, c.id],
            }}),
        )
        .await
        .check_no_content();
    let response = move_block(&editor, &b.id, "after", &a.id).await;
    check_error(
        &response,
        StatusCode::BAD_REQUEST,
        &format!("block {} is not in the content order of its card", b.id),
    );
    move_block_ok(&editor, a, "after", c).await;
    assert_eq!(
        content_order(&th, &card.id).await,
        json!([[b.id, "x"], 5, c.id, a.id, null, c.id, a.id]),
        "the moved block follows each entry of the destination"
    );
    move_block_ok(&editor, c, "before", a).await;
    assert_eq!(
        content_order(&th, &card.id).await,
        json!([[b.id, "x"], 5, c.id, a.id, null, c.id, a.id]),
        "and precedes each entry of it"
    );

    th.teardown().await;
}

/// A move is broadcast as the change of the card, to the board's members who listen
/// to its team.
#[tokio::test]
async fn move_content_block_is_broadcast() {
    let th = Setup::native().start().await.init_basic().await;
    let board = th.create_board(TEST_TEAM_ID, BoardType::Private).await;
    let (card, texts) = card_with_texts(&th.client, &board.id, 2).await;
    let user1 = th.client.me().await;
    let user2 = th.client2.me().await;
    let mut member = TeamSocket::subscribe(&th, &th.client, TEST_TEAM_ID, &user1.id).await;
    let mut outsider = TeamSocket::subscribe(&th, &th.client2, TEST_TEAM_ID, &user2.id).await;

    move_block_ok(&th.client, &texts[0], "after", &texts[1]).await;

    let message = member.expect("UPDATE_BLOCK", "/block/id", &card.id).await;
    assert_eq!(
        message["block"]["fields"]["contentOrder"],
        ids(&[&texts[1], &texts[0]]),
        "{message}"
    );
    assert!(
        th.app.finish_background(BACKGROUND).await,
        "background work finishes"
    );
    assert_eq!(member.drain().await, [json!(null); 0], "nothing else");
    assert_eq!(
        outsider.drain().await,
        [json!(null); 0],
        "a user who is no member of the board hears nothing"
    );
    th.teardown().await;
}

// -- security -----------------------------------------------------------------------------

/// In the original, a user who may change one block somewhere learns the parent of any
/// block whose identifier they know: they ask to move their block next to it, and the
/// refusal names both parents. Here a destination on a board whose cards the user may
/// not manage is refused like a source on such a board.
#[tokio::test]
async fn move_content_block_next_to_a_block_of_a_board_without_permission_is_refused() {
    let th = Setup::native().start().await.init_basic().await;
    let board = th.create_board(TEST_TEAM_ID, BoardType::Private).await;
    let (foreign_card, foreign_texts) = card_with_texts(&th.client, &board.id, 2).await;
    let intruder = &th.client2;
    let own_board: Board = intruder
        .create_board(&Board {
            team_id: TEST_TEAM_ID.to_owned(),
            board_type: BoardType::Private,
            ..Board::default()
        })
        .await
        .created_json();
    let (_, own_texts) = card_with_texts(intruder, &own_board.id, 1).await;

    // The answer is the one for a block that does not exist.
    for position in ["after", "before"] {
        let response = move_block(intruder, &own_texts[0].id, position, &foreign_texts[0].id).await;
        check_error(&response, StatusCode::NOT_FOUND, "block not found");
        assert!(
            !response.text().contains(&foreign_card.id),
            "the parent of the foreign block is not told"
        );
    }
    // A block of their own cannot claim the foreign card as its parent: a parent is a
    // block of the same board. The refusal is the one for a parent that does not exist.
    for parent_id in [foreign_card.id.as_str(), UNKNOWN_ID] {
        let response = intruder
            .insert_blocks(&own_board.id, &[text_block(&own_board.id, parent_id)])
            .await;
        check_error(
            &response,
            StatusCode::BAD_REQUEST,
            "the parent is not a block of the board",
        );
    }

    assert_eq!(
        content_order(&th, &foreign_card.id).await,
        ids(&[&foreign_texts[0], &foreign_texts[1]]),
        "the foreign card is untouched"
    );
    th.teardown().await;
}

/// The check on the destination's board leaves alone what worked before: moves within
/// a card by an editor, and the refusal of a move between cards of two boards for a
/// user who may manage the cards of both.
#[tokio::test]
async fn move_content_block_within_a_card_and_between_own_boards_is_as_before() {
    let th = Setup::native().start().await.init_basic().await;
    let Roles { board, editor, .. } = board_with_roles(&th).await;
    let (card, texts) = card_with_texts(&th.client, &board.id, 2).await;

    move_block_ok(&editor, &texts[0], "after", &texts[1]).await;
    assert_eq!(
        content_order(&th, &card.id).await,
        ids(&[&texts[1], &texts[0]]),
        "an editor moves content within a card"
    );

    let second_board = th.create_board(TEST_TEAM_ID, BoardType::Private).await;
    let (_, second_texts) = card_with_texts(&th.client, &second_board.id, 1).await;
    let response = move_block(&th.client, &texts[0].id, "after", &second_texts[0].id).await;
    check_error(&response, StatusCode::BAD_REQUEST, NOT_SAME_CARD);
    // The editor of the first board is no member of the second, whose blocks do not
    // exist for them.
    let response = move_block(&editor, &texts[0].id, "after", &second_texts[0].id).await;
    check_error(&response, StatusCode::NOT_FOUND, "block not found");

    th.teardown().await;
}
