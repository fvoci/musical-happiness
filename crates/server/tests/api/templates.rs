//! The built-in template boards.
//!
//! Ports `TestGetTemplates` (`server/integrationtests/board_test.go`),
//! `TestGetTemplatesForWorkTemplate` (`server/integrationtests/work_template_test.go`)
//! and `TestApp_initializeTemplates` (`server/app/templates_test.go`).

use std::time::Instant;

use fvoci_model::{Block, Board, BoardType};
use serde_json::{Value, json};

use crate::common::{ROOT_TEAM_ID, Setup};

/// `TestGetTemplates`: the built-in templates are listed, each with blocks, and each can
/// be duplicated into a non-template board.
#[tokio::test]
async fn get_templates_returns_the_built_in_templates() {
    let ts = Setup::native()
        .with_templates()
        .start()
        .await
        .init_basic()
        .await;
    let boards = ts
        .client
        .get(&format!("/teams/{ROOT_TEAM_ID}/templates"))
        .await
        .ok_json::<Vec<Board>>();

    assert!(
        boards.len() >= 6,
        "at least six templates, got {}",
        boards.len()
    );
    assert!(
        boards.iter().all(|board| board.is_template),
        "all are templates"
    );
    assert!(
        boards
            .iter()
            .all(|board| board.board_type == BoardType::Open),
        "all are open"
    );
    assert!(
        boards.iter().all(|board| board.template_version >= 6),
        "all are at least the current version"
    );
    assert!(
        boards
            .iter()
            .any(|board| board.title == "Welcome to Boards!"),
        "the welcome board is present"
    );

    let first = &boards[0];
    let blocks = ts
        .client
        .get(&format!("/boards/{}/blocks", first.id))
        .await
        .ok_json::<Vec<Block>>();
    assert!(!blocks.is_empty(), "the template has blocks");

    let duplicate = ts
        .client
        .post(
            &format!("/boards/{}/duplicate?toTeam={ROOT_TEAM_ID}", first.id),
            &json!({}),
        )
        .await
        .ok_json::<Value>();
    let copy = &duplicate["boards"][0];
    assert_eq!(
        copy["isTemplate"],
        json!(false),
        "the copy is not a template"
    );
    assert!(
        duplicate["blocks"]
            .as_array()
            .is_some_and(|blocks| !blocks.is_empty()),
        "the copy has blocks"
    );
    ts.teardown().await;
}

/// `TestGetTemplatesForWorkTemplate`: the work-template tracking identifiers are present
/// among the templates.
#[tokio::test]
async fn get_templates_has_the_work_template_tracking_ids() {
    let ts = Setup::native()
        .with_templates()
        .start()
        .await
        .init_basic()
        .await;
    let boards = ts
        .client
        .get(&format!("/teams/{ROOT_TEAM_ID}/templates"))
        .await
        .ok_json::<Vec<Board>>();

    let tracking_ids: Vec<String> = boards
        .iter()
        .filter_map(|board| {
            board
                .properties
                .get("trackingTemplateId")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .collect();
    for known in [
        "7ba22ccfdfac391d63dea5c4b8cde0de", // Company Goals & OKRs
        "ecc250bb7dff0fe02247f1110f097544", // Sales Pipeline CRM
        "e4f03181c4ced8edd4d53d33d569a086", // Team Retrospective
    ] {
        assert!(
            tracking_ids.iter().any(|id| id == known),
            "the tracking id {known} is present"
        );
    }
    ts.teardown().await;
}

/// `GET /teams/0/templates` without a session is a 401 (verified on the oracle).
#[tokio::test]
async fn get_templates_without_a_session_is_401() {
    let ts = Setup::native()
        .with_templates()
        .start()
        .await
        .init_basic()
        .await;
    ts.anonymous()
        .get(&format!("/teams/{ROOT_TEAM_ID}/templates"))
        .await
        .check_unauthorized();
    ts.teardown().await;
}

/// `TestApp_initializeTemplates/"Skip template init"`: a second server on a database that
/// already has current templates does not re-import them, and starts without them when
/// not asked for. Also measures the startup cost of template initialization.
#[tokio::test]
async fn template_initialization_is_idempotent_and_measured() {
    // A server without templates starts quickly.
    let without = Instant::now();
    let plain = Setup::native().start().await;
    let plain_cost = without.elapsed();
    assert_eq!(
        ts_template_count(&plain).await,
        0,
        "no templates without initialization"
    );
    plain.teardown().await;

    // A server with templates imports them.
    let with = Instant::now();
    let ts = Setup::native().with_templates().start().await;
    let with_cost = with.elapsed();
    let first = ts_template_count(&ts).await;
    assert!(first >= 6, "templates were imported, got {first}");

    // Re-running initialization on the same, already-current database is a no-op.
    let done = ts.app.init_templates().await.expect("init is idempotent");
    assert!(!done, "no re-import was needed");
    assert_eq!(
        ts_template_count(&ts).await,
        first,
        "the template count is unchanged"
    );

    println!(
        "TEMPLATE_INIT_COST without={plain_cost:?} with={with_cost:?} delta={:?}",
        with_cost.saturating_sub(plain_cost)
    );
    ts.teardown().await;
}

/// The number of template boards the root team is offered.
async fn ts_template_count(ts: &crate::common::TestServer) -> usize {
    ts.app
        .template_boards(ROOT_TEAM_ID, "")
        .await
        .expect("the template boards")
        .len()
}

/// Built-in templates that were imported before the onboarding mark existed are imported
/// again at the next start, so that the tour finds its board. Once there is a marked
/// one, nothing is imported.
#[tokio::test]
async fn templates_without_the_onboarding_mark_are_imported_again() {
    let ts = Setup::native()
        .with_templates()
        .start()
        .await
        .init_basic()
        .await;
    let marked = |boards: &[Board]| {
        boards
            .iter()
            .filter(|board| board.properties.get("onboarding") == Some(&serde_json::json!(true)))
            .map(|board| board.id.clone())
            .collect::<Vec<_>>()
    };
    let before: Vec<Board> = ts
        .client
        .get(&format!("/teams/{ROOT_TEAM_ID}/templates"))
        .await
        .ok_json();
    let marked_before = marked(&before);
    assert_eq!(
        marked_before.len(),
        1,
        "one marked template after the first start"
    );
    assert!(
        !ts.app.init_templates().await.expect("the check runs"),
        "current templates are not imported again"
    );

    // As a database looked before the mark existed.
    let unmark = fvoci_model::BoardPatch {
        deleted_properties: vec!["onboarding".to_owned()],
        ..fvoci_model::BoardPatch::default()
    };
    ts.app
        .store()
        .patch_board(&marked_before[0], &unmark, "")
        .await
        .expect("the mark is removed");

    assert!(
        ts.app.init_templates().await.expect("the import runs"),
        "templates without a marked one are imported again"
    );
    let after: Vec<Board> = ts
        .client
        .get(&format!("/teams/{ROOT_TEAM_ID}/templates"))
        .await
        .ok_json();
    assert_eq!(after.len(), before.len(), "the same number of templates");
    let marked_after = marked(&after);
    assert_eq!(marked_after.len(), 1, "one marked template again");
    assert_ne!(marked_after, marked_before, "a newly imported board");
    ts.client
        .post_empty(&format!("/teams/{ROOT_TEAM_ID}/onboard"))
        .await
        .check_ok();
    ts.teardown().await;
}

/// Security fix. The built-in templates are open boards of the team every user is in,
/// so in the original anyone could join one, become its editor, and change what every
/// new user's first board is copied from. A template is not joined, and so nobody
/// becomes a member of a built-in one.
#[tokio::test]
async fn a_built_in_template_cannot_be_joined_or_changed() {
    let ts = Setup::native()
        .with_templates()
        .start()
        .await
        .init_basic()
        .await;
    let templates = ts
        .client
        .get(&format!("/teams/{ROOT_TEAM_ID}/templates"))
        .await
        .ok_json::<Vec<Board>>();
    let welcome = templates
        .iter()
        .find(|board| board.title.starts_with("Welcome"))
        .expect("the welcome template is built in");

    let response = ts
        .client
        .post_empty(&format!("/boards/{}/join", welcome.id))
        .await;
    response.check_error(
        reqwest::StatusCode::FORBIDDEN,
        "a template cannot be joined",
    );

    // Without a membership there is no way to write to it.
    ts.client
        .patch(
            &format!("/boards/{}", welcome.id),
            &serde_json::json!({"title": "taken over"}),
        )
        .await
        .check_forbidden();
    ts.client
        .delete(&format!("/boards/{}", welcome.id))
        .await
        .check_forbidden();
    let blocks = ts
        .client
        .get(&format!("/boards/{}/blocks", welcome.id))
        .await
        .ok_json::<Vec<fvoci_model::Block>>();
    let card = blocks
        .iter()
        .find(|block| block.block_type == fvoci_model::BlockType::Card)
        .expect("the template has cards");
    ts.client
        .patch(
            &format!("/boards/{}/blocks/{}", welcome.id, card.id),
            &serde_json::json!({"title": "taken over"}),
        )
        .await
        .check_forbidden();
    ts.client
        .post_empty(&format!("/boards/{}/members", welcome.id))
        .await
        .check_forbidden();

    // Reading and copying it is what it is for.
    ts.client
        .post_empty(&format!("/boards/{}/duplicate", welcome.id))
        .await
        .check_ok();
    ts.teardown().await;
}
