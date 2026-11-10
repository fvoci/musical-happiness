//! The onboarding tour.
//!
//! Ports `TestPrepareOnboardingTour` and `TestCreateWelcomeBoard`
//! (`server/app/onboarding_test.go`).

use fvoci_model::{Board, BoardType};
use serde_json::Value;

use crate::common::Setup;

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

    let response = ts.client.post_empty("/teams/0/onboard").await;
    let value = response.ok_json::<Value>();
    assert_eq!(value["teamID"], Value::String("0".to_owned()), "the team");
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
        .post_empty("/teams/my-team/onboard")
        .await
        .ok_json::<Value>();
    assert_eq!(
        value["teamID"],
        Value::String("my-team".to_owned()),
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
        .post_empty("/teams/0/onboard")
        .await
        .check_unauthorized();
    ts.teardown().await;
}
