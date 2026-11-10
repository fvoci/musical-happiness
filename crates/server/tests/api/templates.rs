//! The built-in template boards.
//!
//! Ports `TestGetTemplates` (`server/integrationtests/board_test.go`),
//! `TestGetTemplatesForWorkTemplate` (`server/integrationtests/work_template_test.go`)
//! and `TestApp_initializeTemplates` (`server/app/templates_test.go`).

use std::time::Instant;

use fvoci_model::{Block, Board, BoardType};
use serde_json::{Value, json};

use crate::common::Setup;

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
        .get("/teams/0/templates")
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
            &format!("/boards/{}/duplicate?toTeam=0", first.id),
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
        .get("/teams/0/templates")
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
        .get("/teams/0/templates")
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

/// The number of template boards team 0 has.
async fn ts_template_count(ts: &crate::common::TestServer) -> usize {
    ts.app
        .template_boards("0", "")
        .await
        .expect("the template boards")
        .len()
}
