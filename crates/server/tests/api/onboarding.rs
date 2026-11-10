//! The onboarding tour.
//!
//! Ports `TestPrepareOnboardingTour` and `TestCreateWelcomeBoard`
//! (`server/app/onboarding_test.go`).

use fvoci_model::{Block, BlockType, Board, BoardType, new_id};
use serde_json::{Value, json};

use crate::common::{Client, ROOT_TEAM_ID, Setup, TEST_TEAM_ID, TestServer};

/// The title of the built-in welcome board.
const WELCOME_TITLE: &str = "Welcome to Boards!";

/// Makes an open template in the global team, as any signed-in user can, that looks as
/// much like the built-in welcome board as a client can make it: the same title and the
/// marker property. It holds one card, titled `hijacked`. Returns the template.
async fn impostor_template(client: &Client) -> Board {
    let board: Board = client
        .post(
            "/boards",
            &json!({
                "teamId": ROOT_TEAM_ID, "type": "O", "title": WELCOME_TITLE, "isTemplate": true,
                "properties": {"onboarding": true}, "createdBy": "system",
            }),
        )
        .await
        .ok_json();
    let card = Block {
        id: new_id(),
        board_id: board.id.clone(),
        parent_id: board.id.clone(),
        block_type: BlockType::Card,
        title: "hijacked".to_owned(),
        create_at: 1,
        update_at: 1,
        ..Block::default()
    };
    client.insert_blocks(&board.id, &[card]).await.check_ok();
    board
}

/// The titles of the cards of a board.
async fn card_titles(ts: &TestServer, client: &Client, board_id: &str) -> Vec<String> {
    let _ = ts;
    let blocks: Vec<Block> = client
        .get(&format!("/boards/{board_id}/blocks?type=card"))
        .await
        .ok_json();
    blocks.into_iter().map(|block| block.title).collect()
}

/// The built-in template that carries the onboarding mark.
async fn marked_template(client: &Client) -> Board {
    let templates: Vec<Board> = client
        .get(&format!("/teams/{ROOT_TEAM_ID}/templates"))
        .await
        .ok_json();
    let marked: Vec<Board> = templates
        .into_iter()
        .filter(|board| {
            board.is_builtin && board.properties.get("onboarding") == Some(&json!(true))
        })
        .collect();
    assert_eq!(marked.len(), 1, "one built-in template is marked");
    marked.into_iter().next().expect("checked just above")
}

/// Not in the original, which takes the first open template of the global team that has
/// the welcome board's title, whoever made it. On a server without the built-in
/// templates that handed a user's template to every new user (this test, before the
/// change, got the card `hijacked`). The tour copies the template the system made and
/// marked, and nothing else: here there is none, so onboarding fails.
#[tokio::test]
async fn onboarding_without_the_built_in_templates_does_not_copy_a_user_template() {
    let ts = Setup::native().start().await.init_basic().await;
    let impostor = impostor_template(&ts.client).await;
    assert_ne!(
        impostor.created_by, "system",
        "a client does not choose who created its board"
    );

    let response = ts
        .client2
        .post_empty(&format!("/teams/{ROOT_TEAM_ID}/onboard"))
        .await;

    response.check_status(reqwest::StatusCode::INTERNAL_SERVER_ERROR);
    let boards: Vec<Board> = ts
        .client2
        .get(&format!("/teams/{ROOT_TEAM_ID}/boards"))
        .await
        .ok_json();
    assert_eq!(
        boards
            .iter()
            .filter(|board| !board.is_template)
            .map(|board| board.title.as_str())
            .collect::<Vec<_>>(),
        Vec::<&str>::new(),
        "no board was made for the user"
    );
    ts.teardown().await;
}

/// With the built-in templates, the tour copies the marked one however many templates
/// users made to look like it: the same title, the mark as a property, `createdBy` in
/// the request.
#[tokio::test]
async fn onboarding_copies_the_built_in_board_whatever_users_made() {
    let ts = Setup::native()
        .with_templates()
        .start()
        .await
        .init_basic()
        .await;
    for _ in 0..5 {
        impostor_template(&ts.client).await;
    }
    let built_in = marked_template(&ts.client2).await;
    let mut expected = card_titles(&ts, &ts.client2, &built_in.id).await;
    expected.sort();
    assert!(!expected.is_empty(), "the built-in board has cards");

    let value = ts
        .client2
        .post_empty(&format!("/teams/{ROOT_TEAM_ID}/onboard"))
        .await
        .ok_json::<Value>();
    let board_id = value["boardID"].as_str().expect("a board id").to_owned();

    let mut copied = card_titles(&ts, &ts.client2, &board_id).await;
    copied.sort();
    assert_eq!(copied, expected, "the cards of the built-in board");
    let copy: Board = ts
        .client2
        .get(&format!("/boards/{board_id}"))
        .await
        .ok_json();
    assert_eq!(
        copy.properties.get("onboarding"),
        None,
        "the copy is a user's board and does not carry the mark"
    );
    assert_eq!(copy.board_type, BoardType::Private, "and it is private");
    ts.teardown().await;
}

/// The mark, not the title, is what the tour goes by: the built-in board is still found
/// after its title changed.
#[tokio::test]
async fn onboarding_finds_the_built_in_board_by_its_mark_not_by_its_title() {
    let ts = Setup::native()
        .with_templates()
        .start()
        .await
        .init_basic()
        .await;
    let built_in = marked_template(&ts.client).await;
    assert_eq!(
        built_in.title, WELCOME_TITLE,
        "as it comes out of the archive"
    );
    let rename = fvoci_model::BoardPatch {
        title: Some("Bienvenue".to_owned()),
        ..fvoci_model::BoardPatch::default()
    };
    ts.app
        .store()
        .patch_board(&built_in.id, &rename, "")
        .await
        .expect("the built-in board is renamed");

    let value = ts
        .client
        .post_empty(&format!("/teams/{ROOT_TEAM_ID}/onboard"))
        .await
        .ok_json::<Value>();
    let board_id = value["boardID"].as_str().expect("a board id").to_owned();

    let copy: Board = ts
        .client
        .get(&format!("/boards/{board_id}"))
        .await
        .ok_json();
    assert_eq!(
        copy.title, "Bienvenue",
        "a copy of the renamed built-in board"
    );
    ts.teardown().await;
}

/// `TestPrepareOnboardingTour/"base case"`: onboarding answers the team and a new board,
/// makes the board private, and records the tour preferences.
#[tokio::test]
async fn onboard_duplicates_the_welcome_board_and_records_preferences() {
    let ts = Setup::native()
        .with_templates()
        .start()
        .await
        .init_basic()
        .await;

    let response = ts
        .client
        .post_empty(&format!("/teams/{ROOT_TEAM_ID}/onboard"))
        .await;
    let value = response.ok_json::<Value>();
    assert_eq!(
        value["teamID"],
        Value::String(ROOT_TEAM_ID.to_owned()),
        "the team"
    );
    let board_id = value["boardID"].as_str().expect("a board id").to_owned();
    assert!(!board_id.is_empty(), "a board id is returned");

    let board = ts
        .client
        .get(&format!("/boards/{board_id}"))
        .await
        .ok_json::<Board>();
    assert_eq!(
        board.board_type,
        BoardType::Private,
        "the new board is private"
    );
    assert!(
        board.title.contains("Welcome to Boards"),
        "it is the welcome board: {}",
        board.title
    );

    let user_id = ts.client.me().await.id;
    let preferences = ts
        .app
        .user_preferences(&user_id)
        .await
        .expect("the preferences");
    let find = |name: &str| {
        preferences
            .iter()
            .find(|preference| preference.name == name)
            .map(|preference| preference.value.clone())
    };
    assert_eq!(
        find("onboardingTourStarted"),
        Some("1".to_owned()),
        "tour started"
    );
    assert_eq!(
        find("onboardingTourStep"),
        Some("0".to_owned()),
        "the first step"
    );
    assert_eq!(
        find("tourCategory"),
        Some("onboarding".to_owned()),
        "the tour category"
    );
    ts.teardown().await;
}

/// Onboarding answers for any team identifier, echoing the team back (verified on the
/// oracle).
#[tokio::test]
async fn onboard_echoes_the_team_identifier() {
    let ts = Setup::native()
        .with_templates()
        .start()
        .await
        .init_basic()
        .await;
    let value = ts
        .client
        .post_empty(&format!("/teams/{TEST_TEAM_ID}/onboard"))
        .await
        .ok_json::<Value>();
    assert_eq!(
        value["teamID"],
        Value::String(TEST_TEAM_ID.to_owned()),
        "the team is echoed"
    );
    assert!(
        value["boardID"].as_str().is_some_and(|id| !id.is_empty()),
        "a board id"
    );
    ts.teardown().await;
}

/// `POST /teams/0/onboard` without a session is refused.
#[tokio::test]
async fn onboard_without_a_session_is_refused() {
    let ts = Setup::native()
        .with_templates()
        .start()
        .await
        .init_basic()
        .await;
    ts.anonymous()
        .post_empty(&format!("/teams/{ROOT_TEAM_ID}/onboard"))
        .await
        .check_unauthorized();
    ts.teardown().await;
}
