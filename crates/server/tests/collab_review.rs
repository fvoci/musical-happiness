//! Independent regression cases for the collaboration foundation.

#[path = "api/common.rs"]
mod common;

use std::time::Duration;

use fvoci_collab::BodyDoc;
use fvoci_collab::testing::Editor;
use fvoci_model::{BoardType, Body, Card, FileInfo, Session, new_id};
use fvoci_server::app::App;
use fvoci_server::collab::Collab;
use fvoci_server::error::{AppError, Reason};
use serde_json::{Value, json};

use common::TEST_WS;
use common::{Setup, TEST_WORKSPACE_ID, TestServer};

fn paragraph(text: &str) -> Value {
    json!({"type": "doc", "content": [
        {"type": "paragraph", "content": [{"type": "text", "text": text}]}
    ]})
}

/// An editor that loads a card's document, and which making of the document it loaded.
async fn joined(collab: &Collab, app: &App, card_id: &str, client: u64) -> (Editor, i64) {
    let loaded = collab.open(app, card_id).await.expect("opened");
    (Editor::join(client, &loaded.state), loaded.epoch)
}

async fn editing(th: &TestServer, body: Value) -> (App, Card, String, String) {
    let board = th.create_board(TEST_WORKSPACE_ID, BoardType::Open).await;
    let card = th
        .client
        .post(
            &format!("{TEST_WS}/boards/{}/cards", board.id),
            &json!({"title": "Review", "body": body}),
        )
        .await
        .created_json();
    let user1 = th.client.me().await.id;
    let user2 = th.client2.me().await.id;
    let entered = th
        .app
        .enter_workspace(
            &Session {
                user_id: user1.clone(),
                ..Session::default()
            },
            TEST_WORKSPACE_ID,
        )
        .await
        .expect("membership read")
        .expect("member");
    (entered.app, card, user1, user2)
}

/// A request made against the empty document before an HTTP replacement must not
/// enter the replacement, even though it has no dependency on the old state's IDs.
#[tokio::test]
async fn an_update_from_an_empty_replaced_document_is_out_of_step() {
    let th = Setup::native().start().await.init_basic().await;
    let (app, card, user1, _) = editing(&th, Value::Null).await;
    let collab = app.collab();
    let (editor, editor_epoch) = joined(collab, &app, &card.id, 101).await;
    let delayed = editor.append_paragraph("before the replacement");
    th.client
        .patch(
            &format!("{TEST_WS}/cards/{}", card.id),
            &json!({"body": paragraph("replacement"), "readAt": card.update_at}),
        )
        .await
        .check_ok();
    let taken = collab
        .take(&app, &card.id, editor_epoch, &user1, &delayed)
        .await;
    let logged = app
        .store()
        .get_body_doc(&card.id)
        .await
        .expect("read")
        .expect("doc");
    th.teardown().await;
    assert_eq!(
        taken.as_ref().err().and_then(AppError::reason),
        Some(Reason::UpdateOutOfStep),
        "an update made before replacement must be refused: {taken:?}; log end {}",
        logged.log_end,
    );
}

/// Folding away the update log must preserve who last edited a body that the card
/// does not yet hold; crash recovery must not attribute it to the card's old writer.
#[tokio::test]
async fn recovery_after_folding_preserves_the_last_writer() {
    let th = Setup::native().start().await.init_basic().await;
    let (app, card, user1, user2) = editing(&th, paragraph("first")).await;
    let collab = app.collab();
    let (editor, editor_epoch) = joined(collab, &app, &card.id, 102).await;
    for index in 0..200 {
        let writer = if index == 199 { &user2 } else { &user1 };
        collab
            .take(
                &app,
                &card.id,
                editor_epoch,
                writer,
                &editor.append_paragraph("kept"),
            )
            .await
            .expect("taken");
    }
    let stored = app
        .store()
        .get_body_doc(&card.id)
        .await
        .expect("read")
        .expect("doc");
    assert_eq!(stored.state_seq, 200, "the log was folded");
    assert!(
        stored.updates.is_empty(),
        "all writer-bearing entries were folded"
    );
    assert!(stored.projected_seq < 200, "crash before final projection");
    let th = th
        .killed_and_started_again(async |app| app.collab().abandon().await)
        .await;
    let wanted = editor.body().map(|body| body.document().clone());
    let after = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let read: Card = th
                .client
                .get(&format!("{TEST_WS}/cards/{}", card.id))
                .await
                .ok_json();
            if read.body.as_ref().map(|body| body.document().clone()) == wanted {
                break read;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await;
    th.teardown().await;
    let after = after.expect("the body was recovered before inspecting its author");
    assert_eq!(
        after.modified_by, user2,
        "the last editor survives folding and restart"
    );
}

/// Reducing the admission limit for new updates must not erase an update that was
/// already acknowledged under the previous limit and still needs projection.
#[tokio::test]
async fn lowering_the_update_limit_does_not_erase_acknowledged_edits() {
    let mut th = Setup::native().start().await.init_basic().await;
    let (app, card, user1, _) = editing(&th, paragraph("first")).await;
    let collab = app.collab();
    let (editor, editor_epoch) = joined(collab, &app, &card.id, 101).await;
    let update = editor.append_paragraph(&"kept".repeat(10 * 1024));
    collab
        .take(&app, &card.id, editor_epoch, &user1, &update)
        .await
        .expect("acknowledged");
    let stored = app
        .store()
        .get_body_doc(&card.id)
        .await
        .expect("read")
        .expect("doc");
    assert_eq!(
        (stored.projected_seq, stored.log_end),
        (0, 1),
        "not projected yet"
    );
    th.config.collab.collab_update_bytes = 32 * 1024;
    let th = th
        .killed_and_started_again(async |app| app.collab().abandon().await)
        .await;
    let entered = th
        .app
        .enter_workspace(
            &Session {
                user_id: user1.clone(),
                ..Session::default()
            },
            TEST_WORKSPACE_ID,
        )
        .await
        .expect("membership read")
        .expect("member");
    let app = entered.app;
    let (reopened, _) = joined(app.collab(), &app, &card.id, 102).await;
    let wanted = editor.body();
    let actual = reopened.body();
    let stored = app
        .store()
        .get_body_doc(&card.id)
        .await
        .expect("read")
        .expect("doc");
    th.teardown().await;
    assert!(
        actual == wanted,
        "acknowledged content disappeared after lowering the limit; state_seq {}, projected_seq {}, log_end {}, remaining updates {}",
        stored.state_seq,
        stored.projected_seq,
        stored.log_end,
        stored.updates.len()
    );
}

/// An accepted image update must keep the file alive while its body waits to be
/// projected. Otherwise the collector can make a committed update unprojectable.
#[tokio::test]
async fn a_logged_image_keeps_its_file_before_projection() {
    let th = Setup::native().start().await.init_basic().await;
    let (app, card, user1, _) = editing(&th, Value::Null).await;
    let file = FileInfo {
        id: new_id(),
        board_id: card.board_id.clone(),
        stored_name: "7review.png".to_owned(),
        create_at: 1,
        path: format!("boards/{}/7review.png", card.board_id),
        name: "review.png".to_owned(),
        extension: ".png".to_owned(),
        size: 7,
    };
    app.store()
        .save_file_info(&file)
        .await
        .expect("an old unused file");
    let body = Body::parse(json!({"type": "doc", "content": [
        {"type": "image", "attrs": {"fileId": file.stored_name}}
    ]}))
    .expect("valid body")
    .expect("an image");
    let collab = app.collab();
    let epoch = collab.open(&app, &card.id).await.expect("opened").epoch;
    let update = BodyDoc::from_body(Some(&body)).state();
    collab
        .take(&app, &card.id, epoch, &user1, &update)
        .await
        .expect("image accepted and logged");
    let collected = app
        .store()
        .collect_unreferenced_files(2)
        .await
        .expect("collected");
    let projected = collab.write_body(&app, &card.id).await;
    let replacement = th
        .client
        .patch(
            &format!("{TEST_WS}/cards/{}", card.id),
            &json!({"body": paragraph("replacement"), "readAt": card.update_at}),
        )
        .await;
    th.teardown().await;
    assert!(
        collected.is_empty(),
        "a logged image still needs its file: {collected:?}; projection: {projected:?}; HTTP replacement: {}",
        replacement.status,
    );
    assert!(
        projected.is_ok(),
        "an accepted update must be projectable: {projected:?}"
    );
}

/// The document shutdown's five-second budget must also bound the wait for a worker
/// with a request in flight; queuing End behind that request defeats the deadline.
#[cfg(unix)]
#[tokio::test]
async fn document_shutdown_does_not_wait_for_the_worker_request_timeout() {
    let th = Setup::native()
        .configure(|config| config.collab.collab_worker_timeout_ms = 30_000)
        .start()
        .await
        .init_basic()
        .await;
    let (app, card, _, _) = editing(&th, paragraph("first")).await;
    let collab = app.collab().clone();
    collab.open(&app, &card.id).await.expect("opened");
    let pid = collab.worker_pids().await[0].expect("our worker runs");
    let stalled = tokio::spawn({
        let (app, collab, card_id) = (app.clone(), collab.clone(), card.id.clone());
        async move { collab.stall_document(&app, &card_id, 60_000).await }
    });
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(!stalled.is_finished(), "the worker request is in flight");
    let ended = tokio::time::timeout(Duration::from_secs(8), collab.end(&app)).await;
    // Stop only the worker this test started, then drain its request and queued End.
    if ended.is_err() {
        let killed = std::process::Command::new("kill")
            .args(["-KILL", &pid.to_string()])
            .status()
            .expect("kill runs");
        assert!(killed.success(), "our stalled worker was killed");
    }
    let _ = stalled.await.expect("the request task did not panic");
    th.teardown().await;
    assert!(
        ended.is_ok(),
        "document shutdown exceeded even an eight-second allowance"
    );
}
