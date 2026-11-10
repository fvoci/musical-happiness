//! Cards: the card view of blocks of type `card`.
//! Ports of `server/integrationtests/cards_test.go` and `server/app/cards_test.go`,
//! plus tests of what `server/api/cards.go` decides and the original leaves untested:
//! who may do what, the refusals with their order and messages, paging, and who is
//! told about a change. The expectations of those were observed on the original.
//!
//! The subtests of `TestGetCards` share one server; here every subtest is a test of its
//! own that first builds the same cards.
//!
//! The cases of `server/app/cards_test.go` that inject a store failure through a mock
//! are ported with the failures the real store has; each says which.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use fvoci_model::{
    BLOCK_TITLE_MAX_CHARS, Block, BlockType, Board, BoardMember, BoardType, Card, CardPatch,
    JsonMap, is_id, new_id,
};
use fvoci_server::app::ChangeNotification;
use fvoci_server::error::ErrorKind;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

use crate::boards::TeamSocket;
use crate::common::{ApiResponse, Client, Setup, TEST_TEAM_ID, TestServer, UNKNOWN_ID};

/// How long background notifications may take to finish.
const BACKGROUND: Duration = Duration::from_secs(10);

// -- helpers --------------------------------------------------------------------------

/// A private board of the first user (`th.client`, its administrator) with one more
/// member of each role. `th.client2` is no member.
pub(crate) struct Roles {
    pub(crate) board: Board,
    pub(crate) editor: Client,
    pub(crate) commenter: Client,
    pub(crate) viewer: Client,
}

pub(crate) async fn board_with_roles(th: &TestServer) -> Roles {
    let board = th.create_board(TEST_TEAM_ID, BoardType::Private).await;
    let editor = th.add_user("editor").await;
    let commenter = th.add_user("commenter").await;
    let viewer = th.add_user("viewer").await;
    let member = |user_id: String| BoardMember {
        board_id: board.id.clone(),
        user_id,
        ..BoardMember::default()
    };
    let members = [
        BoardMember {
            scheme_editor: true,
            ..member(editor.me().await.id)
        },
        BoardMember {
            scheme_commenter: true,
            ..member(commenter.me().await.id)
        },
        BoardMember {
            scheme_viewer: true,
            ..member(viewer.me().await.id)
        },
    ];
    for member in &members {
        th.client.add_member(member).await.check_ok();
    }
    Roles {
        board,
        editor,
        commenter,
        viewer,
    }
}

/// Checks that the answer is the error with this status and message.
#[track_caller]
pub(crate) fn check_error(response: &ApiResponse, status: StatusCode, message: &str) {
    response.check_error(status, message);
}

/// Checks that the answer is the server error, which never says what went wrong.
#[track_caller]
pub(crate) fn check_internal_error(response: &ApiResponse) {
    response.check_internal_error();
}

fn cards_path(board_id: &str) -> String {
    format!("/boards/{board_id}/cards")
}

fn card_path(card_id: &str) -> String {
    format!("/cards/{card_id}")
}

/// `CreateCard`, expecting success. Returns the card, which is stored as answered.
async fn create_card(client: &Client, board_id: &str, card: &impl serde::Serialize) -> Card {
    let created: Card = client
        .post(&cards_path(board_id), card)
        .await
        .created_json();
    let read: Card = client.get(&card_path(&created.id)).await.ok_json();
    assert_eq!(read, created, "the card is stored as answered");
    read
}

/// `GetCards`, expecting success.
async fn cards_of(client: &Client, board_id: &str, query: &str) -> Vec<Card> {
    client
        .get(&format!("{}{query}", cards_path(board_id)))
        .await
        .ok_json()
}

/// `MakeCardProps`: `count` properties with random names and values.
fn make_card_props(count: usize) -> JsonMap {
    (0..count).map(|_| (new_id(), json!(new_id()))).collect()
}

/// `CreateBoardAndCards`: an open board of the first user with `count` cards.
async fn create_board_and_cards(th: &TestServer, count: usize) -> (Board, Vec<Card>) {
    let board = th.create_board(TEST_TEAM_ID, BoardType::Open).await;
    let mut cards = Vec::with_capacity(count);
    for number in 1..=count {
        let card = Card {
            title: format!("test card {number}"),
            icon: "😱".to_owned(),
            properties: make_card_props(5),
            ..Card::default()
        };
        let created: Card = th
            .client
            .post(
                &format!("{}?disableNotify=true", cards_path(&board.id)),
                &card,
            )
            .await
            .created_json();
        cards.push(created);
    }
    (board, cards)
}

/// A block as the original's tests send it, with both times set to 1.
pub(crate) fn new_block(board_id: &str, parent_id: &str, block_type: BlockType) -> Block {
    Block {
        id: new_id(),
        board_id: board_id.to_owned(),
        parent_id: parent_id.to_owned(),
        create_at: 1,
        update_at: 1,
        block_type,
        ..Block::default()
    }
}

/// Inserts one block through the API and returns it as the server answered.
pub(crate) async fn insert_block(client: &Client, block: &Block) -> Block {
    let mut inserted: Vec<Block> = client
        .insert_blocks(&block.board_id, std::slice::from_ref(block))
        .await
        .created_json();
    inserted.pop().expect("one block was inserted")
}

/// Inserts a card block with the given fields through the blocks route, which checks
/// nothing about them.
async fn insert_card_block(client: &Client, board_id: &str, fields: Value) -> Block {
    let block = Block {
        fields: serde_json::from_value(fields).expect("the fields are a JSON object"),
        ..new_block(board_id, "", BlockType::Card)
    };
    insert_block(client, &block).await
}

fn title_patch(title: &str) -> CardPatch {
    CardPatch {
        title: Some(title.to_owned()),
        ..CardPatch::default()
    }
}

/// The 25 cards of `TestGetCards`, titled `0` to `24`, each with three texts.
const CARD_COUNT: usize = 25;

async fn get_cards_fixture(th: &TestServer) -> Board {
    let board = th.create_board(TEST_TEAM_ID, BoardType::Open).await;
    let user_id = th.client.me().await.id;
    for number in 0..CARD_COUNT {
        let card = Card {
            board_id: board.id.clone(),
            created_by: user_id.clone(),
            modified_by: user_id.clone(),
            title: number.to_string(),
            ..Card::default()
        };
        let path = format!("{}?disableNotify=true", cards_path(&board.id));
        let created: Card = th.client.post(&path, &card).await.created_json();

        let texts: Vec<Block> = (0..3)
            .map(|text| Block {
                created_by: user_id.clone(),
                modified_by: user_id.clone(),
                schema: 1,
                title: format!("text {text} for card {number}"),
                ..new_block(&board.id, &created.id, BlockType::Attachment)
            })
            .collect();
        th.client
            .post(
                &format!("/boards/{}/blocks?disableNotify=true", board.id),
                &texts,
            )
            .await
            .check_created();
    }
    board
}

/// The numbers the cards of `TestGetCards` have as their titles.
fn card_numbers(cards: &[Card]) -> BTreeSet<usize> {
    cards
        .iter()
        .map(|card| card.title.parse().expect("the title is the card's number"))
        .collect()
}

// -- server/integrationtests/cards_test.go ----------------------------------------------

/// `TestCreateCard/"a non authenticated user should be rejected"`
#[tokio::test]
async fn create_card_a_non_authenticated_user_should_be_rejected() {
    let th = Setup::native().start().await.init_basic().await;
    let board = th.create_board(TEST_TEAM_ID, BoardType::Open).await;
    let card = Card {
        title: "basic card".to_owned(),
        ..Card::default()
    };

    let response = th.anonymous().post(&cards_path(&board.id), &card).await;

    response.check_unauthorized();
    th.teardown().await;
}

/// `TestCreateCard/"good"`
#[tokio::test]
async fn create_card_good() {
    let th = Setup::native().start().await.init_basic().await;
    let board = th.create_board(TEST_TEAM_ID, BoardType::Open).await;
    let card = Card {
        title: "test card 1".to_owned(),
        icon: "😱".to_owned(),
        ..Card::default()
    };

    let card_new = create_card(&th.client, &board.id, &card).await;

    assert_eq!(card_new.board_id, board.id, "the card is on the board");
    assert_eq!(card_new.title, "test card 1", "the title");
    assert_eq!(card_new.icon, "😱", "the icon");
    th.teardown().await;
}

/// `TestCreateCard/"invalid card"`
#[tokio::test]
async fn create_card_invalid_card() {
    let th = Setup::native().start().await.init_basic().await;
    let board = th.create_board(TEST_TEAM_ID, BoardType::Open).await;
    let card = Card {
        title: "too many emoji's".to_owned(),
        icon: "😱😱😱😱".to_owned(),
        ..Card::default()
    };

    let response = th.client.post(&cards_path(&board.id), &card).await;

    check_error(
        &response,
        StatusCode::BAD_REQUEST,
        "invalid card, Icon can have only one grapheme",
    );
    assert_eq!(
        cards_of(&th.client, &board.id, "").await,
        [],
        "nothing was created"
    );
    th.teardown().await;
}

/// `TestGetCards/"fetch all cards"`
#[tokio::test]
async fn get_cards_fetch_all_cards() {
    let th = Setup::native().start().await.init_basic().await;
    let board = get_cards_fixture(&th).await;

    let cards = cards_of(&th.client, &board.id, "?page=0&perPage=-1").await;

    assert_eq!(cards.len(), CARD_COUNT, "all cards and none of the texts");
    th.teardown().await;
}

/// `TestGetCards/"fetch with pagination"`
#[tokio::test]
async fn get_cards_fetch_with_pagination() {
    let th = Setup::native().start().await.init_basic().await;
    let board = get_cards_fixture(&th).await;
    let mut card_numbers_seen = BTreeSet::new();

    for (page, expected_count) in [(0, 10), (1, 10), (2, 5)] {
        let query = format!("?page={page}&perPage=10");
        let cards = cards_of(&th.client, &board.id, &query).await;
        assert_eq!(cards.len(), expected_count, "the cards of page {page}");
        card_numbers_seen.extend(card_numbers(&cards));
    }

    assert_eq!(
        card_numbers_seen,
        (0..CARD_COUNT).collect(),
        "the pages together are all cards"
    );
    th.teardown().await;
}

/// `TestGetCards/"a non authenticated user should be rejected"`
#[tokio::test]
async fn get_cards_a_non_authenticated_user_should_be_rejected() {
    let th = Setup::native().start().await.init_basic().await;
    let board = get_cards_fixture(&th).await;

    let response = th
        .anonymous()
        .get(&format!("{}?page=0&perPage=10", cards_path(&board.id)))
        .await;

    response.check_unauthorized();
    th.teardown().await;
}

/// `TestPatchCard/"a non authenticated user should be rejected"`
#[tokio::test]
async fn patch_card_a_non_authenticated_user_should_be_rejected() {
    let th = Setup::native().start().await.init_basic().await;
    let (_, cards) = create_board_and_cards(&th, 1).await;
    let card = &cards[0];

    let response = th
        .anonymous()
        .patch(&card_path(&card.id), &title_patch("another title"))
        .await;

    response.check_unauthorized();
    th.teardown().await;
}

/// `TestPatchCard/"good"`
#[tokio::test]
async fn patch_card_good() {
    let th = Setup::native().start().await.init_basic().await;
    let (board, cards) = create_board_and_cards(&th, 1).await;
    let card = &cards[0];

    let updated_props: JsonMap = card
        .properties
        .keys()
        .map(|id| (id.clone(), json!(new_id())))
        .collect();
    let patch = CardPatch {
        title: Some("another title".to_owned()),
        icon: Some("🐿".to_owned()),
        updated_properties: updated_props.clone(),
        ..CardPatch::default()
    };

    let patched: Card = th
        .client
        .patch(&card_path(&card.id), &patch)
        .await
        .ok_json();

    assert_eq!(patched.board_id, board.id, "the card stays on its board");
    assert_eq!(patched.title, "another title", "the new title");
    assert_eq!(patched.icon, "🐿", "the new icon");
    assert_eq!(patched.properties, updated_props, "the new properties");
    th.teardown().await;
}

/// `TestPatchCard/"invalid card patch"`
///
/// A bad request, as for a new card with such an icon. (The original answers 500.)
#[tokio::test]
async fn patch_card_invalid_card_patch() {
    let th = Setup::native().start().await.init_basic().await;
    let (_, cards) = create_board_and_cards(&th, 1).await;
    let card = &cards[0];
    let patch = CardPatch {
        icon: Some("🐿🐿🐿".to_owned()),
        ..CardPatch::default()
    };

    let response = th.client.patch(&card_path(&card.id), &patch).await;

    check_error(
        &response,
        StatusCode::BAD_REQUEST,
        "invalid card, Icon can have only one grapheme",
    );
    let unchanged: Card = th.client.get(&card_path(&card.id)).await.ok_json();
    assert_eq!(&unchanged, card, "the card is as it was");
    th.teardown().await;
}

/// `TestGetCard/"a non authenticated user should be rejected"`
#[tokio::test]
async fn get_card_a_non_authenticated_user_should_be_rejected() {
    let th = Setup::native().start().await.init_basic().await;
    let (_, cards) = create_board_and_cards(&th, 1).await;

    let response = th.anonymous().get(&card_path(&cards[0].id)).await;

    response.check_unauthorized();
    th.teardown().await;
}

/// `TestGetCard/"good"`
#[tokio::test]
async fn get_card_good() {
    let th = Setup::native().start().await.init_basic().await;
    let (board, cards) = create_board_and_cards(&th, 1).await;
    let card = &cards[0];

    let fetched: Card = th.client.get(&card_path(&card.id)).await.ok_json();

    assert_eq!(fetched.board_id, board.id, "the card's board");
    assert_eq!(fetched.title, card.title, "the title");
    assert_eq!(fetched.icon, card.icon, "the icon");
    assert_eq!(fetched.properties, card.properties, "the properties");
    th.teardown().await;
}

// -- server/app/cards_test.go, through the real store -------------------------------------

/// `TestCreateCard/"success scenario"` (app)
#[tokio::test]
async fn app_create_card_success_scenario() {
    let th = Setup::native().start().await.init_basic().await;
    let board = th.create_board(TEST_TEAM_ID, BoardType::Open).await;
    let user_id = th.stranger().await;
    let card = Card {
        board_id: board.id.clone(),
        created_by: user_id.clone(),
        modified_by: user_id.clone(),
        title: "test card".to_owned(),
        properties: make_card_props(3),
        ..Card::default()
    };

    let new_card = th
        .app
        .create_card(card.clone(), &board.id, &user_id, ChangeNotification::Send)
        .await
        .expect("the card is created");

    assert_eq!(new_card.board_id, card.board_id, "the board");
    assert_eq!(new_card.title, card.title, "the title");
    assert_eq!(new_card.properties, card.properties, "the properties");
    th.teardown().await;
}

/// `TestCreateCard/"error scenario"` (app). The original injects a store failure
/// through a mock; the failures the real store has are a block it refuses and a board
/// that does not exist.
#[tokio::test]
async fn app_create_card_error_scenario() {
    let th = Setup::native().start().await.init_basic().await;
    let board = th.create_board(TEST_TEAM_ID, BoardType::Open).await;
    let user_id = th.stranger().await;

    let too_long = Card {
        title: "a".repeat(BLOCK_TITLE_MAX_CHARS + 1),
        ..Card::default()
    };
    let error = th
        .app
        .create_card(too_long, &board.id, &user_id, ChangeNotification::Send)
        .await
        .expect_err("the title is too long");
    assert_eq!(error.kind(), ErrorKind::BadRequest, "{error:?}");
    assert!(
        error
            .to_string()
            .ends_with(": block title size limit exceeded"),
        "what is wrong with the card, without the app layer's own words: {error}"
    );

    for board_id in [UNKNOWN_ID, "no-such-board"] {
        let error = th
            .app
            .create_card(
                Card::default(),
                board_id,
                &user_id,
                ChangeNotification::Send,
            )
            .await
            .expect_err("the board does not exist");
        assert!(error.is_not_found(), "{board_id}: {error:?}");
    }

    let cards = th
        .app
        .cards_for_board(&board.id, 0, 0)
        .await
        .expect("the cards are read");
    assert_eq!(cards, [], "nothing was created");
    th.teardown().await;
}

/// `TestGetCards/"success scenario"` (app)
#[tokio::test]
async fn app_get_cards_success_scenario() {
    let th = Setup::native().start().await.init_basic().await;
    let board = th.create_board(TEST_TEAM_ID, BoardType::Open).await;
    let mut blocks: Vec<Block> = (0..CARD_COUNT)
        .map(|number| Block {
            schema: 1,
            title: format!("card {number}"),
            ..new_block(&board.id, &board.id, BlockType::Card)
        })
        .collect();
    th.app
        .insert_blocks(
            &mut blocks,
            &th.stranger().await,
            ChangeNotification::Suppress,
        )
        .await
        .expect("the card blocks are stored");

    let cards = th
        .app
        .cards_for_board(&board.id, 0, 0)
        .await
        .expect("the cards are read");

    assert_eq!(cards.len(), CARD_COUNT, "all cards");
    th.teardown().await;
}

/// `TestGetCards/"error scenario"` (app). The original injects a store failure through
/// a mock; the failure the real store allows is a card block that cannot be read as a
/// card.
#[tokio::test]
async fn app_get_cards_error_scenario() {
    let th = Setup::native().start().await.init_basic().await;
    let board = th.create_board(TEST_TEAM_ID, BoardType::Open).await;
    insert_card_block(&th.client, &board.id, json!({"icon": 5})).await;

    let error = th
        .app
        .cards_for_board(&board.id, 0, 0)
        .await
        .expect_err("one of the cards is malformed");

    assert_eq!(error.kind(), ErrorKind::Internal, "{error:?}");
    assert_eq!(
        error.to_string(),
        "Block2Card fail: invalid type for field 'icon'",
        "what the log says"
    );
    th.teardown().await;
}

/// `TestPatchCard/"success scenario"` (app)
#[tokio::test]
async fn app_patch_card_success_scenario() {
    let th = Setup::native().start().await.init_basic().await;
    let board = th.create_board(TEST_TEAM_ID, BoardType::Open).await;
    let user_id = th.stranger().await;
    let props = make_card_props(3);
    let card = Card {
        title: "test card for patch".to_owned(),
        properties: props.clone(),
        ..Card::default()
    };
    let card = th
        .app
        .create_card(card, &board.id, &user_id, ChangeNotification::Send)
        .await
        .expect("the card is created");

    let card_patch = CardPatch {
        title: Some("patched".to_owned()),
        icon: Some("😀".to_owned()),
        updated_properties: props
            .keys()
            .map(|id| (id.clone(), json!(new_id())))
            .collect(),
        ..CardPatch::default()
    };
    let mut expected = card.clone();
    card_patch.apply(&mut expected);

    let patched = th
        .app
        .patch_card(&card_patch, &card.id, &user_id, ChangeNotification::Send)
        .await
        .expect("the card is patched");

    assert_eq!(patched.board_id, board.id, "the board");
    assert_eq!(patched.title, "patched", "the title");
    assert_eq!(patched.icon, "😀", "the icon");
    assert_eq!(patched.properties, expected.properties, "the properties");
    th.teardown().await;
}

/// `TestPatchCard/"error scenario"` (app). The original injects a store failure
/// through a mock; the failures there really are: no such card, a patched title that
/// is too long, and a patch that is not valid.
#[tokio::test]
async fn app_patch_card_error_scenario() {
    let th = Setup::native().start().await.init_basic().await;
    let (_, cards) = create_board_and_cards(&th, 1).await;
    let card = &cards[0];
    let patch_card = async |patch: &CardPatch, card_id: &str| {
        th.app
            .patch_card(
                patch,
                card_id,
                &th.stranger().await,
                ChangeNotification::Send,
            )
            .await
            .expect_err("the patch is refused")
    };

    for card_id in [UNKNOWN_ID, "no-such-card"] {
        let error = patch_card(&title_patch("x"), card_id).await;
        assert!(error.is_not_found(), "{card_id}: {error:?}");
        assert_eq!(
            error.to_string(),
            "not found",
            "nothing but that it is not there"
        );
    }

    let too_long = title_patch(&"a".repeat(BLOCK_TITLE_MAX_CHARS + 1));
    let error = patch_card(&too_long, &card.id).await;
    assert_eq!(error.kind(), ErrorKind::BadRequest, "{error:?}");

    let bad_icon = CardPatch {
        icon: Some("😀😀".to_owned()),
        ..CardPatch::default()
    };
    let error = patch_card(&bad_icon, &card.id).await;
    assert_eq!(error.kind(), ErrorKind::BadRequest, "{error:?}");

    let unchanged = th.app.card(&card.id).await.expect("the card is read");
    assert_eq!(&unchanged, card, "no patch was applied");
    th.teardown().await;
}

/// `TestGetCard/"success scenario"` (app)
#[tokio::test]
async fn app_get_card_success_scenario() {
    let th = Setup::native().start().await.init_basic().await;
    let board = th.create_board(TEST_TEAM_ID, BoardType::Open).await;
    let props = make_card_props(5);
    let content_order = vec![new_id(), new_id()];
    let fields = json!({
        "contentOrder": content_order,
        "properties": props,
        "icon": "😀",
        "isTemplate": true,
    });
    let block = Block {
        title: "test card".to_owned(),
        fields: serde_json::from_value(fields).expect("the fields are a JSON object"),
        ..new_block(&board.id, &board.id, BlockType::Card)
    };
    let block = insert_block(&th.client, &block).await;

    let card = th.app.card(&block.id).await.expect("the card is read");

    assert_eq!(card.board_id, board.id, "the board");
    assert_eq!(card.title, block.title, "the title");
    assert_eq!(card.icon, "😀", "the icon");
    assert!(card.is_template, "a template");
    assert_eq!(card.properties, props, "the properties");
    th.teardown().await;
}

/// `TestGetCard/"not found"` (app)
#[tokio::test]
async fn app_get_card_not_found() {
    let th = Setup::native().start().await.init_basic().await;

    for bogus_id in [UNKNOWN_ID, "nope"] {
        let error = th
            .app
            .card(bogus_id)
            .await
            .expect_err("there is no such block");

        assert!(error.is_not_found(), "{bogus_id}: {error:?}");
    }
    th.teardown().await;
}

/// `TestGetCard/"error scenario"` (app). The original injects a store failure through
/// a mock; the failure there really is: a block that is no card.
#[tokio::test]
async fn app_get_card_error_scenario() {
    let th = Setup::native().start().await.init_basic().await;
    let board = th.create_board(TEST_TEAM_ID, BoardType::Open).await;
    let text = insert_block(&th.client, &new_block(&board.id, "", BlockType::Attachment)).await;

    let error = th.app.card(&text.id).await.expect_err("a text is no card");

    assert_eq!(error.kind(), ErrorKind::Internal, "{error:?}");
    assert_eq!(
        error.to_string(),
        "cannot convert block to card: not a card block",
        "what the handlers pass on"
    );
    th.teardown().await;
}

// -- POST /boards/{board_id}/cards --------------------------------------------------------

/// The server chooses the identifier, the board, the creator and the times of a new
/// card, whatever the request says, and stores the card as a block of the board.
#[tokio::test]
async fn create_card_answer_and_stored_block() {
    let th = Setup::native().start().await.init_basic().await;
    let board = th.create_board(TEST_TEAM_ID, BoardType::Private).await;
    let me = th.client.me().await;
    let before = fvoci_server::auth::now_millis();

    // Everything a card can say about itself.
    let response = th
        .client
        .post(
            &cards_path(&board.id),
            &json!({
                "id": "myid",
                "boardId": board.id,
                "createdBy": "someone",
                "modifiedBy": "else",
                "title": "t",
                "contentOrder": ["a", "b"],
                "icon": "👨‍👩‍👧",
                "isTemplate": true,
                "properties": {"p": "v", "q": ["1", "2"]},
                "createAt": 5,
                "updateAt": 6,
                "deleteAt": 7,
            }),
        )
        .await;
    let card: Card = response.created_json();
    assert!(
        is_id(&card.id),
        "a new identifier, not the client's: {}",
        card.id
    );
    assert!(card.create_at >= before, "created now: {}", card.create_at);
    assert!(
        card.update_at >= card.create_at,
        "written at {}, not before its creation at {}",
        card.update_at,
        card.create_at
    );
    assert_eq!(
        response.value(),
        json!({
            "id": card.id,
            "boardId": board.id,
            "createdBy": me.id,
            "modifiedBy": me.id,
            "title": "t",
            "icon": "👨‍👩‍👧",
            "isTemplate": true,
            "properties": {"p": "v", "q": ["1", "2"]},
            "body": null,
            "hasBody": false,
            "tasksTotal": 0,
            "tasksDone": 0,
            "createAt": card.create_at,
            "updateAt": card.update_at,
            "deleteAt": 0,
        }),
        "the answer is the card with the server's identifier, creator and times"
    );

    let block = th.app.block(&card.id).await.expect("the block is stored");
    assert_eq!(
        serde_json::to_value(&block).expect("a block is JSON"),
        json!({
            "id": card.id,
            "parentId": "",
            "createdBy": me.id,
            "modifiedBy": me.id,
            "schema": 1,
            "type": "card",
            "title": "t",
            // A content order is no member of a card: what the request had is not
            // stored.
            "fields": {
                "icon": "👨‍👩‍👧",
                "isTemplate": true,
                "properties": {"p": "v", "q": ["1", "2"]},
            },
            "body": null,
            "createAt": card.create_at,
            "updateAt": card.update_at,
            "deleteAt": 0,
            "boardId": board.id,
        }),
        "a card block directly on its board, with the times that were answered"
    );

    th.teardown().await;
}

/// A card need not say anything about itself; `null` members count as left out.
#[tokio::test]
async fn create_card_without_members() {
    let th = Setup::native().start().await.init_basic().await;
    let board = th.create_board(TEST_TEAM_ID, BoardType::Private).await;
    let me = th.client.me().await;

    for body in [
        json!({}),
        json!({"title": null, "contentOrder": null, "properties": null, "icon": null}),
    ] {
        let response = th.client.post(&cards_path(&board.id), &body).await;
        let card: Card = response.created_json();
        assert_eq!(
            response.value(),
            json!({
                "id": card.id,
                "boardId": board.id,
                "createdBy": me.id,
                "modifiedBy": me.id,
                "title": "",
                "icon": "",
                "isTemplate": false,
                "properties": {},
                "body": null,
                "hasBody": false,
                "tasksTotal": 0,
                "tasksDone": 0,
                "createAt": card.create_at,
                "updateAt": card.update_at,
                "deleteAt": 0,
            }),
            "an empty card for {body}"
        );
    }

    th.teardown().await;
}

/// What `POST /boards/{id}/cards` refuses, in the order the checks are made: whether
/// the board can be seen, then the permission, then the body, then the card.
#[tokio::test]
async fn create_card_refusals() {
    let th = Setup::native().start().await.init_basic().await;
    let roles = board_with_roles(&th).await;
    let path = cards_path(&roles.board.id);
    let outsider = &th.client2;
    let denied = "access denied to create card";

    // Creating a card takes the permission to manage the board's cards. Who is no
    // member of the private board is told that there is no such board, as for a board
    // that does not exist; the body is not looked at.
    let card = json!({"title": "x"});
    for client in [&roles.commenter, &roles.viewer] {
        for body in [card.to_string().as_str(), "{\"title\":", "null"] {
            let response = client.send_raw_body(Method::POST, &path, body).await;
            check_error(&response, StatusCode::FORBIDDEN, denied);
        }
    }
    for body in [card.to_string().as_str(), "{\"title\":", "null"] {
        let response = outsider.send_raw_body(Method::POST, &path, body).await;
        check_error(&response, StatusCode::NOT_FOUND, "board not found");
    }
    for board_id in [UNKNOWN_ID, "no-such-board"] {
        let response = th.client.post(&cards_path(board_id), &card).await;
        check_error(&response, StatusCode::NOT_FOUND, "board not found");
    }

    // A body that is no card is a bad request.
    for body in [
        "{\"title\":",
        "",
        "[]",
        "null",
        "{\"title\":5}",
        "\"card\"",
        "{\"icon\":5}",
    ] {
        let response = th.client.send_raw_body(Method::POST, &path, body).await;
        response.check_bad_request();
        assert_eq!(response.error_code(), "bad_request", "{body}");
    }

    let by_editor = create_card(&roles.editor, &roles.board.id, &card).await;
    assert_eq!(
        by_editor.created_by,
        roles.editor.me().await.id,
        "an editor creates cards"
    );

    let bad_icon = json!({"icon": "😱😱"});
    for other_board in [UNKNOWN_ID, "other"] {
        let response = th
            .client
            .post(&path, &json!({"boardId": other_board}))
            .await;
        check_error(&response, StatusCode::BAD_REQUEST, "Board IDs do not match");
    }
    let response = th.client.post(&path, &bad_icon).await;
    check_error(
        &response,
        StatusCode::BAD_REQUEST,
        "invalid card, Icon can have only one grapheme",
    );

    // The store's limits are the client's mistake too.
    let too_long = json!({"title": "a".repeat(BLOCK_TITLE_MAX_CHARS + 1)});
    let response = th.client.post(&path, &too_long).await;
    response.check_bad_request();
    let message = response.error_message();
    let block_id = message
        .strip_prefix("error validating block ")
        .and_then(|rest| rest.strip_suffix(": block title size limit exceeded"));
    assert!(
        block_id.is_some_and(is_id),
        "the message names the block by the identifier it would have had: {message}"
    );

    let cards = cards_of(&th.client, &roles.board.id, "").await;
    assert_eq!(cards, [by_editor], "only the editor's card was created");
    th.teardown().await;
}

// -- GET /boards/{board_id}/cards ---------------------------------------------------------

/// Everyone who may view the board gets its cards; a malformed paging parameter is
/// refused.
#[tokio::test]
async fn get_cards_permissions_and_malformed_paging() {
    let th = Setup::native().start().await.init_basic().await;
    let roles = board_with_roles(&th).await;
    let board_id = roles.board.id.as_str();
    let path = cards_path(board_id);
    let outsider = &th.client2;
    let card = create_card(&th.client, board_id, &json!({"title": "card"})).await;

    for client in [&th.client, &roles.editor, &roles.commenter, &roles.viewer] {
        assert_eq!(
            cards_of(client, board_id, "").await,
            std::slice::from_ref(&card),
            "every member reads the cards"
        );
    }
    // The private board answers an outsider as a board that does not exist.
    let response = outsider.get(&path).await;
    check_error(&response, StatusCode::NOT_FOUND, "board not found");
    for board_id in [UNKNOWN_ID, "no-such-board"] {
        let response = th.client.get(&cards_path(board_id)).await;
        check_error(&response, StatusCode::NOT_FOUND, "board not found");
    }

    // The original writes these refusals and then the cards after them; here the
    // refusal is the whole answer, and it names the parameter.
    let invalid = |name: &str, reason: &str| format!("invalid `{name}` parameter: {reason}");
    let cases = [
        ("?page=x", "page", "not a whole number"),
        ("?perPage=x", "perPage", "not a whole number"),
        ("?page=x&perPage=y", "page", "not a whole number"),
        ("?perPage=1.5", "perPage", "not a whole number"),
        ("?perPage=%202", "perPage", "not a whole number"),
        ("?page=99999999999999999999", "page", "out of range"),
    ];
    for (query, name, reason) in cases {
        let response = th.client.get(&format!("{path}{query}")).await;
        check_error(&response, StatusCode::BAD_REQUEST, &invalid(name, reason));
        assert_eq!(response.value()["field"], json!(name), "{query}");
    }
    // Whether the board can be seen comes first.
    let response = outsider.get(&format!("{path}?page=x")).await;
    check_error(&response, StatusCode::NOT_FOUND, "board not found");

    th.teardown().await;
}

/// `page` counts from zero and defaults to it, `perPage` defaults to 100, and zero or
/// less asks for all cards.
#[tokio::test]
async fn get_cards_paging() {
    let th = Setup::native().start().await.init_basic().await;
    let board = th.create_board(TEST_TEAM_ID, BoardType::Open).await;
    let total = 103;
    let mut blocks: Vec<Block> = (0..total)
        .map(|number| Block {
            title: number.to_string(),
            ..new_block(&board.id, &board.id, BlockType::Card)
        })
        .collect();
    th.app
        .insert_blocks(
            &mut blocks,
            &th.stranger().await,
            ChangeNotification::Suppress,
        )
        .await
        .expect("the card blocks are stored");
    let count = async |query: &str| cards_of(&th.client, &board.id, query).await.len();

    assert_eq!(count("").await, 100, "a page has 100 cards by default");
    assert_eq!(count("?page=1").await, 3, "the rest is on the second page");
    assert_eq!(count("?page=2").await, 0, "and nothing on the third");
    assert_eq!(count("?page=&perPage=").await, 100, "empty is absent");
    // The page size has one spelling; another is a parameter the server does not know.
    assert_eq!(count("?per_page=2").await, 100, "no page size");
    assert_eq!(count("?per_page=x").await, 100, "and nothing to refuse");
    assert_eq!(count("?perPage=2").await, 2, "a smaller page");
    assert_eq!(count("?perPage=%2B2").await, 2, "a sign is fine");
    assert_eq!(count("?page=00&perPage=03").await, 3, "so are zeros");
    assert_eq!(count("?page=51&perPage=2").await, 1, "the last card");
    assert_eq!(count("?page=1000&perPage=2").await, 0, "beyond the end");
    assert_eq!(count("?perPage=1000000000000").await, total, "a huge page");
    assert_eq!(count("?perPage=0").await, total, "zero is no limit");
    assert_eq!(count("?perPage=-1").await, total, "nor is less than zero");
    assert_eq!(
        count("?page=0&perPage=-5").await,
        total,
        "on the first page"
    );
    assert_eq!(count("?page=-1").await, 0, "no page before the first");
    assert_eq!(count("?page=-1&perPage=2").await, 0, "whatever its size");
    // The original's SQLite fails these two with a server error, an offset without a
    // limit being no valid query there; here a page size of zero or less is all cards
    // on any page.
    assert_eq!(count("?page=1&perPage=0").await, total, "all cards");
    assert_eq!(count("?page=-1&perPage=-1").await, total, "all cards");
    // The original's offset wraps around here and lands on the first page.
    assert_eq!(
        count("?page=4611686018427387904&perPage=4").await,
        0,
        "a page beyond any card"
    );

    // Pages do not overlap and leave nothing out.
    let mut titles = BTreeSet::new();
    for page in 0..6 {
        let cards = cards_of(&th.client, &board.id, &format!("?page={page}&perPage=20")).await;
        titles.extend(cards.into_iter().map(|card| card.title));
    }
    assert_eq!(titles.len(), total, "every card is on one of the pages");

    th.teardown().await;
}

/// Preserved quirk: one card block that cannot be read as a card fails the board's
/// whole list of cards (server/app/cards.go:54-60), and reading it alone is a bad
/// request that says why. Properties that are no object are the exception, see
/// `cards_whose_properties_are_no_object_are_read_without_properties`.
#[tokio::test]
async fn get_cards_fails_on_a_malformed_card() {
    let th = Setup::native().start().await.init_basic().await;
    let board = th.create_board(TEST_TEAM_ID, BoardType::Open).await;
    let path = cards_path(&board.id);
    create_card(&th.client, &board.id, &json!({"title": "fine"})).await;
    // A stored `contentOrder` is not read, whatever its shape: side-by-side content, as
    // the original's web app stores it, has nested lists.
    insert_card_block(
        &th.client,
        &board.id,
        json!({"contentOrder": ["a", ["b", "c"]]}),
    )
    .await;
    insert_card_block(&th.client, &board.id, json!({"contentOrder": "a"})).await;
    assert_eq!(
        cards_of(&th.client, &board.id, "").await.len(),
        3,
        "three cards"
    );

    let cases = [json!({"icon": 5}), json!({"isTemplate": "yes"})];
    for fields in cases {
        let block = insert_card_block(&th.client, &board.id, fields).await;

        let response = th.client.get(&path).await;
        check_internal_error(&response);
        // What is stored is no card: that is the server's state, not the request's
        // mistake. (The original answers 400 for the card alone.)
        let response = th.client.get(&card_path(&block.id)).await;
        check_internal_error(&response);

        th.client
            .delete(&format!("/boards/{}/blocks/{}", board.id, block.id))
            .await
            .check_no_content();
        assert_eq!(
            cards_of(&th.client, &board.id, "").await.len(),
            3,
            "the list works again without the malformed card"
        );
    }

    th.teardown().await;
}

// -- GET /cards/{card_id} -----------------------------------------------------------------

/// Reading one card takes the permission to view the board, like the board's list of
/// cards. The original asks for the permission to manage cards here
/// (server/api/cards.go:363), so its viewers and commenters get the card in the list but
/// a 403 for the card alone; that is not reproduced. The path names a card and no board:
/// what is no card, and a card on a board the user cannot read, answer alike.
#[tokio::test]
async fn get_card_permissions_and_refusals() {
    let th = Setup::native().start().await.init_basic().await;
    let roles = board_with_roles(&th).await;
    let board_id = roles.board.id.as_str();
    let outsider = &th.client2;
    let card = create_card(&th.client, board_id, &json!({"title": "card"})).await;
    let path = card_path(&card.id);

    for client in [&th.client, &roles.editor, &roles.commenter, &roles.viewer] {
        let fetched: Card = client.get(&path).await.ok_json();
        assert_eq!(fetched, card, "every member of the board reads the card");
    }
    let response = outsider.get(&path).await;
    check_error(&response, StatusCode::NOT_FOUND, "card not found");
    for client in [&roles.commenter, &roles.viewer] {
        assert_eq!(
            cards_of(client, board_id, "").await,
            std::slice::from_ref(&card),
            "the same card as in the list"
        );
    }

    // No card: an identifier nobody has, a block that is no card, a board.
    let text = insert_block(
        &th.client,
        &new_block(board_id, &card.id, BlockType::Attachment),
    )
    .await;
    for card_id in [UNKNOWN_ID, "nope", text.id.as_str(), board_id] {
        for client in [&th.client, outsider] {
            let response = client.get(&card_path(card_id)).await;
            check_error(&response, StatusCode::NOT_FOUND, "card not found");
        }
    }

    // A card block without fields is an empty card.
    let bare = insert_card_block(&th.client, board_id, json!({})).await;
    let response = th.client.get(&card_path(&bare.id)).await;
    response.check_ok();
    assert_eq!(
        response.value(),
        json!({
            "id": bare.id,
            "boardId": board_id,
            "createdBy": bare.created_by,
            "modifiedBy": bare.modified_by,
            "title": "",
            "icon": "",
            "isTemplate": false,
            "properties": {},
            "body": null,
            "hasBody": false,
            "tasksTotal": 0,
            "tasksDone": 0,
            "createAt": response.value()["createAt"],
            "updateAt": response.value()["updateAt"],
            "deleteAt": 0,
        }),
        "the card of a bare card block"
    );

    th.teardown().await;
}

// -- PATCH /cards/{card_id} ---------------------------------------------------------------

/// What `PATCH /cards/{id}` refuses, in the order the checks are made: whether there is
/// such a card for the user, then the permission on its board, then the body.
#[tokio::test]
async fn patch_card_refusals() {
    let th = Setup::native().start().await.init_basic().await;
    let roles = board_with_roles(&th).await;
    let board_id = roles.board.id.as_str();
    let outsider = &th.client2;
    let card = create_card(&th.client, board_id, &json!({"title": "card"})).await;
    let path = card_path(&card.id);
    let text = insert_block(
        &th.client,
        &new_block(board_id, &card.id, BlockType::Attachment),
    )
    .await;
    let malformed = "{\"title\":";

    // No card: not found for everyone, whatever the body. A card on a board the user
    // cannot read answers the same.
    for card_id in [UNKNOWN_ID, "nope", text.id.as_str()] {
        for client in [&th.client, outsider] {
            let response = client
                .send_raw_body(Method::PATCH, &card_path(card_id), malformed)
                .await;
            check_error(&response, StatusCode::NOT_FOUND, "card not found");
        }
    }
    for body in ["{\"title\":\"x\"}", malformed] {
        let response = outsider.send_raw_body(Method::PATCH, &path, body).await;
        check_error(&response, StatusCode::NOT_FOUND, "card not found");
    }

    // No permission to manage the board's cards: refused, whatever the body.
    for client in [&roles.commenter, &roles.viewer] {
        for body in ["{\"title\":\"x\"}", malformed, "{\"icon\":\"🐿🐿🐿\"}"] {
            let response = client.send_raw_body(Method::PATCH, &path, body).await;
            check_error(
                &response,
                StatusCode::FORBIDDEN,
                "access denied to patch card",
            );
        }
    }

    // A body that is no patch is a bad request.
    // So are `null` and an icon of more than one grapheme. (The original answers 500.)
    for body in [
        malformed,
        "",
        "[]",
        "null",
        "{\"title\":5}",
        "{\"icon\":5}",
        "{\"icon\":\"🐿🐿🐿\"}",
    ] {
        let response = th.client.send_raw_body(Method::PATCH, &path, body).await;
        response.check_bad_request();
        assert_eq!(response.error_code(), "bad_request", "{body}");
    }

    let too_long = title_patch(&"a".repeat(BLOCK_TITLE_MAX_CHARS + 1));
    let response = th.client.patch(&path, &too_long).await;
    check_error(
        &response,
        StatusCode::BAD_REQUEST,
        &format!(
            "error validating block {0}: block title size limit exceeded",
            card.id
        ),
    );

    let unchanged: Card = th.client.get(&path).await.ok_json();
    assert_eq!(unchanged, card, "no patch was applied");
    th.teardown().await;
}

/// What a patch changes and what it leaves alone.
#[tokio::test]
async fn patch_card_changes() {
    let th = Setup::native().start().await.init_basic().await;
    let roles = board_with_roles(&th).await;
    let board_id = roles.board.id.as_str();
    let me = th.client.me().await;
    let editor = roles.editor.me().await;
    let card = create_card(
        &th.client,
        board_id,
        &json!({
            "title": "t",
            "contentOrder": ["a", "b"],
            "icon": "😱",
            "isTemplate": true,
            "properties": {"p": "v", "q": ["1", "2"]},
        }),
    )
    .await;
    let path = card_path(&card.id);
    let send = async |client: &Client, path: &str, body: Value| -> Card {
        client.patch(path, &body).await.ok_json()
    };

    // The title alone; the answer is the whole card.
    let patched = send(&th.client, &path, json!({"title": "patched"})).await;
    assert!(patched.update_at >= card.update_at, "modified now");
    let expected = Card {
        title: "patched".to_owned(),
        update_at: patched.update_at,
        ..card.clone()
    };
    assert_eq!(patched, expected, "only the title changed");

    // Patches that change nothing but the modification time.
    for body in [
        json!({}),
        json!({"updatedProperties": {}}),
        json!({"updatedProperties": null}),
        json!({"title": null, "contentOrder": null, "icon": null}),
        json!({"isTemplate": false, "deleteAt": 5, "boardId": "x", "deletedProperties": ["p"]}),
    ] {
        let patched = send(&th.client, &path, body.clone()).await;
        let expected = Card {
            update_at: patched.update_at,
            ..expected.clone()
        };
        assert_eq!(patched, expected, "nothing changed by {body}");
    }

    // Preserved quirk: updated properties replace all properties.
    let patched = send(
        &th.client,
        &path,
        json!({"updatedProperties": {"z": "only"}}),
    )
    .await;
    assert_eq!(
        serde_json::to_value(&patched.properties).expect("properties are JSON"),
        json!({"z": "only"}),
        "the other properties are gone"
    );

    // The icon, by an editor, without notification. A content order is not a member
    // of a card any more and changes nothing.
    let patched = send(
        &roles.editor,
        &format!("{path}?disableNotify=true"),
        json!({"contentOrder": ["x", "y", "z"], "icon": "🐿"}),
    )
    .await;
    assert!(
        serde_json::to_value(&patched).expect("a card is JSON")["contentOrder"].is_null(),
        "no order"
    );
    assert_eq!(patched.icon, "🐿", "the new icon");
    assert_eq!(patched.modified_by, editor.id, "modified by the editor");
    assert_eq!(patched.created_by, me.id, "still created by its creator");

    // The icon can be emptied.
    let patched = send(&th.client, &path, json!({"icon": ""})).await;
    assert_eq!(patched.icon, "", "no icon");
    assert_eq!(patched.modified_by, me.id, "modified by the admin again");

    let block = th.app.block(&card.id).await.expect("the block is stored");
    assert_eq!(block.title, "patched", "the block's title");
    assert_eq!(
        serde_json::to_value(&block.fields).expect("fields are JSON"),
        json!({
            "icon": "",
            "isTemplate": true,
            "properties": {"z": "only"},
        }),
        "the block's fields"
    );

    // A card block without fields can be patched (the original crashes on its
    // properties, as on any block stored without fields).
    let bare = insert_card_block(&th.client, board_id, json!({})).await;
    let patched = send(
        &th.client,
        &card_path(&bare.id),
        json!({"title": "x", "updatedProperties": {"a": "b"}}),
    )
    .await;
    assert_eq!(patched.title, "x", "the title of the bare card");
    assert_eq!(
        serde_json::to_value(&patched.properties).expect("properties are JSON"),
        json!({"a": "b"}),
        "its properties"
    );

    th.teardown().await;
}

// -- who is told about a change -----------------------------------------------------------

/// Creating and patching a card is broadcast as the change of its block to the
/// board's members who listen to its team, with or without `disableNotify`.
#[tokio::test]
async fn card_changes_are_broadcast() {
    let th = Setup::native().start().await.init_basic().await;
    let board = th.create_board(TEST_TEAM_ID, BoardType::Private).await;
    let user1 = th.client.me().await;
    let user2 = th.client2.me().await;
    let mut member = TeamSocket::subscribe(&th, &th.client, TEST_TEAM_ID, &user1.id).await;
    let mut outsider = TeamSocket::subscribe(&th, &th.client2, TEST_TEAM_ID, &user2.id).await;
    let path = cards_path(&board.id);

    for (path, title) in [
        (path.clone(), "card"),
        (format!("{path}?disableNotify=true"), "quiet card"),
    ] {
        let card: Card = th
            .client
            .post(&path, &json!({"title": title}))
            .await
            .created_json();
        let message = member.expect("UPDATE_BLOCK", "/block/id", &card.id).await;
        assert_eq!(message["teamId"], json!(TEST_TEAM_ID), "{message}");
        assert_eq!(message["block"]["type"], json!("card"), "{message}");
        assert_eq!(message["block"]["title"], json!(title), "{message}");
        assert_eq!(message["block"]["boardId"], json!(board.id), "{message}");

        for (query, title) in [("", "patched"), ("?disableNotify=true", "patched quietly")] {
            th.client
                .patch(
                    &format!("{}{query}", card_path(&card.id)),
                    &title_patch(title),
                )
                .await
                .check_ok();
            let message = member.expect("UPDATE_BLOCK", "/block/id", &card.id).await;
            assert_eq!(message["block"]["title"], json!(title), "{message}");
        }
    }

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

/// Where the webhook test's receiver listens. The server's configuration is changed by
/// a plain function, which finds the address here.
static WEBHOOK_URL: OnceLock<String> = OnceLock::new();

/// Starts an HTTP server that keeps the JSON bodies posted to it, and puts its address
/// into [`WEBHOOK_URL`].
async fn start_webhook_receiver() -> (Arc<Mutex<Vec<Value>>>, tokio::task::JoinHandle<()>) {
    let posted: Arc<Mutex<Vec<Value>>> = Arc::default();
    let keep = {
        let posted = Arc::clone(&posted);
        move |axum::Json(block): axum::Json<Value>| async move {
            posted.lock().expect("lock poisoned").push(block);
        }
    };
    let hook = axum::Router::new().route("/hook", axum::routing::post(keep));
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("a local port is free");
    let address = listener.local_addr().expect("the listener has an address");
    WEBHOOK_URL
        .set(format!("http://{address}/hook"))
        .expect("only one test sets the webhook address");
    let serving = tokio::spawn(async move {
        axum::serve(listener, hook)
            .await
            .expect("the webhook receiver serves");
    });
    (posted, serving)
}

/// A created card, a patched card and a card whose content was reordered are posted
/// to the webhook, as their blocks; a subscription is not.
#[tokio::test]
async fn card_changes_call_the_webhook() {
    let (posted, serving) = start_webhook_receiver().await;
    let th = Setup::native()
        .configure(|config| {
            config.webhook_update = WEBHOOK_URL.get().into_iter().cloned().collect();
        })
        .start()
        .await
        .init_basic()
        .await;
    let board = th.create_board(TEST_TEAM_ID, BoardType::Private).await;
    let me = th.client.me().await;
    // What the webhook got since the last call, as (identifier, type, title) triples.
    let take_posted = async || -> Vec<(String, String, String)> {
        assert!(
            th.app.finish_background(BACKGROUND).await,
            "the webhook calls finish"
        );
        std::mem::take(&mut *posted.lock().expect("lock poisoned"))
            .iter()
            .map(|block| {
                let text = |member: &str| block[member].as_str().unwrap_or_default().to_owned();
                (text("id"), text("type"), text("title"))
            })
            .collect()
    };
    let card_triple =
        |card: &Card, title: &str| (card.id.clone(), "card".to_owned(), title.to_owned());
    let nothing: [(String, String, String); 0] = [];

    let card = create_card(&th.client, &board.id, &json!({"title": "card"})).await;
    assert_eq!(
        take_posted().await,
        [card_triple(&card, "card")],
        "a new card"
    );

    th.client
        .patch(
            &format!("{}?disableNotify=true", card_path(&card.id)),
            &title_patch("patched"),
        )
        .await
        .check_ok();
    assert_eq!(
        take_posted().await,
        [card_triple(&card, "patched")],
        "a patched card"
    );

    th.client
        .post(
            "/subscriptions",
            &json!({
                "blockType": "card",
                "blockId": card.id,
                "subscriberType": "user",
                "subscriberId": me.id,
            }),
        )
        .await
        .check_created();
    th.client
        .delete(&format!("/subscriptions/{}/{}", card.id, me.id))
        .await
        .check_no_content();
    assert_eq!(take_posted().await, nothing, "subscriptions are not posted");

    th.teardown().await;
    serving.abort();
}
