//! A card's body written whole while its document holds what editors typed: the
//! editors' updates are never written over, nor is a body written past the document.

use fvoci_collab::testing::Editor;
use fvoci_server::collab::{DocChange, DocEvent};
use serde_json::{Value, json};
use tokio::sync::broadcast::Receiver;

use crate::collab::{body_of, card, editing};
use crate::common::Setup;

fn paragraphs(texts: &[&str]) -> Value {
    let content: Vec<Value> = texts
        .iter()
        .map(|text| json!({"type": "paragraph", "content": [{"type": "text", "text": text}]}))
        .collect();
    json!({"type": "doc", "content": content})
}

/// Whether the room was told `change` about the card, passing over the rest.
fn told(events: &mut Receiver<DocEvent>, card_id: &str, change: DocChange) -> bool {
    while let Ok(event) = events.try_recv() {
        if event.card_id == card_id && event.change == change {
            return true;
        }
    }
    false
}

/// A title written to the card between the read of the card and the write of the
/// document's body: the body is written all the same, with what the editors typed, and
/// the title stays.
#[tokio::test]
async fn a_title_written_while_a_body_is_written_keeps_what_the_editors_typed() {
    let th = Setup::native().start().await.init_basic().await;
    let e = editing(&th, &paragraphs(&["first"])).await;
    let collab = e.app.collab().clone();
    let mut events = collab.subscribe();
    let editor = Editor::join(101, &collab.open(&e.app, &e.card_id).await.expect("opened"));
    collab
        .take(
            &e.app,
            &e.card_id,
            &e.user1,
            &editor.append_paragraph("typed"),
        )
        .await
        .expect("taken");
    let (waiting, go) = collab.pause_next_projection();
    let projection = tokio::spawn({
        let (collab, app, card_id) = (collab.clone(), e.app.clone(), e.card_id.clone());
        async move { collab.write_body(&app, &card_id).await }
    });
    waiting.notified().await;
    th.client
        .patch(
            &format!("/cards/{}", e.card_id),
            &json!({"title": "renamed"}),
        )
        .await
        .check_ok();
    go.notify_one();
    projection.await.expect("the task ends").expect("written");
    let read = card(&th.client, &e.card_id).await;
    assert_eq!(read.title, "renamed");
    assert_eq!(
        body_of(&read),
        editor.body().map(|body| body.document().clone()),
        "what the editors typed"
    );
    assert!(!told(&mut events, &e.card_id, DocChange::Replaced));
    th.teardown().await;
}

/// A body written to the card between the read of the card and the write of the
/// document's body, by a writer that does not go through the document (the store
/// itself): it is not written over, and the document is made again from it.
#[tokio::test]
async fn a_body_written_while_a_body_is_written_is_not_written_over() {
    let th = Setup::native().start().await.init_basic().await;
    let e = editing(&th, &paragraphs(&["first"])).await;
    let collab = e.app.collab().clone();
    let mut events = collab.subscribe();
    let editor = Editor::join(101, &collab.open(&e.app, &e.card_id).await.expect("opened"));
    collab
        .take(
            &e.app,
            &e.card_id,
            &e.user1,
            &editor.append_paragraph("typed"),
        )
        .await
        .expect("taken");
    let (waiting, go) = collab.pause_next_projection();
    let projection = tokio::spawn({
        let (collab, app, card_id) = (collab.clone(), e.app.clone(), e.card_id.clone());
        async move { collab.write_body(&app, &card_id).await }
    });
    waiting.notified().await;
    let past = paragraphs(&["written past"]);
    let read = card(&th.client, &e.card_id).await;
    e.app
        .store()
        .patch_block(
            &e.card_id,
            &fvoci_model::BlockPatch {
                body: fvoci_model::BodyChange::Set(
                    fvoci_model::Body::parse(past.clone())
                        .expect("a body")
                        .expect("not empty"),
                ),
                read_at: Some(read.update_at),
                ..fvoci_model::BlockPatch::default()
            },
            &e.user2,
        )
        .await
        .expect("written past the document");
    go.notify_one();
    projection.await.expect("the task ends").expect("written");
    assert_eq!(
        body_of(&card(&th.client, &e.card_id).await),
        Some(past.clone()),
        "not written over"
    );
    assert!(told(&mut events, &e.card_id, DocChange::Replaced));
    let again = Editor::join(102, &collab.open(&e.app, &e.card_id).await.expect("opened"));
    assert_eq!(again.body().map(|body| body.document().clone()), Some(past));
    th.teardown().await;
}

/// A whole body written over the routes to a card whose document is closed with
/// updates in its log that the card does not hold (a server that ended before it wrote
/// them): they are written first, and a writer who read the card before is refused.
#[tokio::test]
async fn a_body_written_whole_over_a_log_not_written_yet_is_refused_until_read_again() {
    let th = Setup::native().start().await.init_basic().await;
    let e = editing(&th, &paragraphs(&["first"])).await;
    let collab = e.app.collab();
    let editor = Editor::join(101, &collab.open(&e.app, &e.card_id).await.expect("opened"));
    collab
        .take(
            &e.app,
            &e.card_id,
            &e.user1,
            &editor.append_paragraph("typed"),
        )
        .await
        .expect("taken");
    collab.abandon().await;
    let read_before = card(&th.client, &e.card_id).await;
    assert_ne!(
        body_of(&read_before),
        editor.body().map(|body| body.document().clone()),
        "not written yet"
    );
    let path = format!("/cards/{}", e.card_id);
    let whole = paragraphs(&["written whole"]);
    th.client
        .patch(
            &path,
            &json!({"body": whole, "readAt": read_before.update_at}),
        )
        .await
        .check_status(reqwest::StatusCode::CONFLICT);
    let read = card(&th.client, &e.card_id).await;
    assert_eq!(
        body_of(&read),
        editor.body().map(|body| body.document().clone()),
        "the log was written first"
    );
    th.client
        .patch(&path, &json!({"body": whole, "readAt": read.update_at}))
        .await
        .check_ok();
    let again = Editor::join(102, &collab.open(&e.app, &e.card_id).await.expect("opened"));
    assert_eq!(
        again.body().map(|body| body.document().clone()),
        Some(whole)
    );
    th.teardown().await;
}

/// A batch of patches that writes a card's body whole meets its open document as one
/// patch does: refused for a read before the editors' update, and the document that
/// body after.
#[tokio::test]
async fn a_batch_that_writes_a_body_whole_meets_the_open_document() {
    let th = Setup::native().start().await.init_basic().await;
    let e = editing(&th, &paragraphs(&["first"])).await;
    let collab = e.app.collab();
    let mut events = collab.subscribe();
    let editor = Editor::join(101, &collab.open(&e.app, &e.card_id).await.expect("opened"));
    let read_before = card(&th.client, &e.card_id).await;
    collab
        .take(
            &e.app,
            &e.card_id,
            &e.user1,
            &editor.append_paragraph("typed"),
        )
        .await
        .expect("taken");
    let whole = paragraphs(&["written in a batch"]);
    let batch = |read_at: i64| {
        json!({
            "blockIds": [e.card_id],
            "blockPatches": [{"body": whole, "readAt": read_at}],
        })
    };
    let path = format!("/boards/{}/blocks", read_before.board_id);
    th.client
        .patch(&path, &batch(read_before.update_at))
        .await
        .check_status(reqwest::StatusCode::CONFLICT);
    let read = card(&th.client, &e.card_id).await;
    th.client
        .patch(&path, &batch(read.update_at))
        .await
        .check_no_content();
    assert!(told(&mut events, &e.card_id, DocChange::Replaced));
    let again = Editor::join(102, &collab.open(&e.app, &e.card_id).await.expect("opened"));
    assert_eq!(
        again.body().map(|body| body.document().clone()),
        Some(whole)
    );
    th.teardown().await;
}

/// A batch whose identifiers are not in the order the cards were made in writes the
/// body of the card its patch is for around that card's document.
#[tokio::test]
async fn a_batch_writes_the_body_of_the_card_its_patch_is_for() {
    let th = Setup::native().start().await.init_basic().await;
    let older = editing(&th, &paragraphs(&["older"])).await;
    let collab = older.app.collab();
    let board_id = card(&th.client, &older.card_id).await.board_id;
    // Made after the first, on its board.
    let newer: fvoci_model::Card = th
        .client
        .post(
            &format!("/boards/{board_id}/cards"),
            &json!({"title": "newer", "body": paragraphs(&["newer"])}),
        )
        .await
        .created_json();
    let editor = Editor::join(
        101,
        &collab
            .open(&older.app, &older.card_id)
            .await
            .expect("opened"),
    );
    let read_before = card(&th.client, &older.card_id).await;
    collab
        .take(
            &older.app,
            &older.card_id,
            &older.user1,
            &editor.append_paragraph("typed"),
        )
        .await
        .expect("taken");
    let whole = paragraphs(&["written in a batch"]);
    let path = format!("/boards/{}/blocks", read_before.board_id);
    th.client
        .patch(
            &path,
            &json!({
                "blockIds": [newer.id, older.card_id],
                "blockPatches": [{"title": "x"}, {"body": whole, "readAt": read_before.update_at}],
            }),
        )
        .await
        .check_status(reqwest::StatusCode::CONFLICT);
    assert_eq!(
        body_of(&card(&th.client, &older.card_id).await),
        editor.body().map(|body| body.document().clone()),
        "the editors' update was written first"
    );
    th.teardown().await;
}

/// A patch of boards and blocks that writes a card's body whole meets its document as a
/// patch of the card does.
#[tokio::test]
async fn a_patch_of_boards_and_blocks_meets_the_open_document() {
    let th = Setup::native().start().await.init_basic().await;
    let e = editing(&th, &paragraphs(&["first"])).await;
    let collab = e.app.collab();
    let mut events = collab.subscribe();
    let editor = Editor::join(101, &collab.open(&e.app, &e.card_id).await.expect("opened"));
    let read_before = card(&th.client, &e.card_id).await;
    collab
        .take(
            &e.app,
            &e.card_id,
            &e.user1,
            &editor.append_paragraph("typed"),
        )
        .await
        .expect("taken");
    let whole = paragraphs(&["written with the boards"]);
    let patch = |read_at: i64| {
        json!({
            "boardIds": [read_before.board_id],
            "boardPatches": [{"title": "the board"}],
            "blockIds": [e.card_id],
            "blockPatches": [{"body": whole, "readAt": read_at}],
        })
    };
    th.client
        .patch("/boards-and-blocks", &patch(read_before.update_at))
        .await
        .check_status(reqwest::StatusCode::CONFLICT);
    let read = card(&th.client, &e.card_id).await;
    th.client
        .patch("/boards-and-blocks", &patch(read.update_at))
        .await
        .check_ok();
    assert!(told(&mut events, &e.card_id, DocChange::Replaced));
    let again = Editor::join(102, &collab.open(&e.app, &e.card_id).await.expect("opened"));
    assert_eq!(
        again.body().map(|body| body.document().clone()),
        Some(whole)
    );
    th.teardown().await;
}

/// A document whose log holds an update the limits refuse now (they were lowered while
/// the server was down) and that the card does not hold is made again from the card:
/// it opens, and the card's body can be written whole, instead of being refused for
/// good. The event names the card's organization.
#[tokio::test]
async fn a_log_the_lowered_limits_refuse_gives_way_to_the_card() {
    let th = Setup::native().start().await.init_basic().await;
    let e = editing(&th, &paragraphs(&["first"])).await;
    let collab = e.app.collab();
    let editor = Editor::join(101, &collab.open(&e.app, &e.card_id).await.expect("opened"));
    collab
        .take(
            &e.app,
            &e.card_id,
            &e.user1,
            &editor.append_paragraph(&"l".repeat(40 * 1024)),
        )
        .await
        .expect("taken");
    let card_id = e.card_id.clone();
    let held = body_of(&card(&th.client, &card_id).await);
    let mut th = th;
    th.config.collab.collab_update_bytes = 32 * 1024;
    let th = th
        .killed_and_started_again(async |app| app.collab().abandon().await)
        .await;
    let e = crate::collab::Editing {
        card_id,
        ..crate::collab::editing_app(&th).await
    };
    let collab = e.app.collab();
    let mut events = collab.subscribe();
    let again = Editor::join(102, &collab.open(&e.app, &e.card_id).await.expect("opened"));
    assert_eq!(
        again.body().map(|body| body.document().clone()),
        held,
        "the card's body"
    );
    let read = card(&th.client, &e.card_id).await;
    let whole = paragraphs(&["written whole"]);
    th.client
        .patch(
            &format!("/cards/{}", e.card_id),
            &json!({"body": whole, "readAt": read.update_at}),
        )
        .await
        .check_ok();
    // The start may have made the document again from the card as well (Remade).
    let replaced = std::iter::from_fn(|| events.try_recv().ok())
        .find(|event| event.change == DocChange::Replaced)
        .expect("the room is told");
    assert_eq!(
        (replaced.tenant, replaced.card_id),
        (e.app.store().tenant(), e.card_id.clone())
    );
    th.teardown().await;
}
