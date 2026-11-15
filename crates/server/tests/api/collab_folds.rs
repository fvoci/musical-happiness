//! What is stored of a document after its room is done with it: the log folded into
//! the state as the document closes, also when the server that held it was killed
//! and the next one starts, and never longer than the fold's bound in a room that
//! stays open.

use std::time::{Duration, Instant};

use fvoci_collab::testing::Editor;
use fvoci_server::collab::FOLD_AFTER;
use serde_json::{Value, json};

use crate::collab::{Editing, body_of, card, editing, editing_app, joined};
use crate::common::{Setup, TestServer};

fn paragraphs(texts: &[&str]) -> Value {
    let content: Vec<Value> = texts
        .iter()
        .map(|text| json!({"type": "paragraph", "content": [{"type": "text", "text": text}]}))
        .collect();
    json!({"type": "doc", "content": content})
}

/// The state's last update, the log's last, the last one the card holds, and how many
/// rows the log has.
async fn stored(e: &Editing) -> (i64, i64, i64, usize) {
    let stored = e
        .app
        .store()
        .get_body_doc(&e.card_id)
        .await
        .expect("read")
        .expect("a document");
    (
        stored.state_seq,
        stored.log_end,
        stored.projected_seq,
        stored.updates.len(),
    )
}

/// Writes `lines` into the card's document as its first user, each an update.
async fn typed(e: &Editing, lines: &[String]) -> Editor {
    let collab = e.app.collab();
    let (editor, epoch) = joined(collab, &e.app, &e.card_id, 101).await;
    for line in lines {
        collab
            .take(
                &e.app,
                &e.card_id,
                epoch,
                &e.user1,
                &editor.append_paragraph(line),
            )
            .await
            .expect("taken");
    }
    editor
}

fn lines(count: usize) -> Vec<String> {
    (0..count).map(|n| format!("line {n}")).collect()
}

/// The document of the card as a server opens it now, and the body.
async fn reopened(th: &TestServer, card_id: &str) -> Option<Value> {
    let e = Editing {
        card_id: card_id.to_owned(),
        ..editing_app(th).await
    };
    let (again, _) = joined(e.app.collab(), &e.app, &e.card_id, 109).await;
    again.body().map(|body| body.document().clone())
}

/// A document closed with updates its state does not hold (fewer than a fold's worth)
/// is stored as its state alone: the log is empty, and the document opened again is
/// the same.
#[tokio::test]
async fn a_document_that_closes_folds_its_log() {
    let th = Setup::native().start().await.init_basic().await;
    let e = editing(&th, &paragraphs(&["first"])).await;
    let editor = typed(&e, &lines(30)).await;
    let wanted = editor.body().map(|body| body.document().clone());
    let (_, log_end, _, rows) = stored(&e).await;
    assert_eq!(rows, 30, "logged, not folded yet");

    e.app
        .collab()
        .close(&e.app, &e.card_id)
        .await
        .expect("closed");
    assert_eq!(
        stored(&e).await,
        (log_end, log_end, log_end, 0),
        "folded and written"
    );
    assert_eq!(body_of(&card(&th.client, &e.card_id).await), wanted);
    assert_eq!(reopened(&th, &e.card_id).await, wanted, "the same document");
    th.teardown().await;
}

/// What a server that was killed left in the log, written to the card or not yet, is
/// folded by the next server as it starts: the log is empty and the body is the same.
#[tokio::test]
async fn the_log_a_killed_server_left_is_folded_by_the_next() {
    let th = Setup::native().start().await.init_basic().await;
    let written = editing(&th, &paragraphs(&["written"])).await;
    let unwritten = editing(&th, &paragraphs(&["unwritten"])).await;
    let wanted_written = typed(&written, &lines(40)).await;
    // Its body is written to the card, two seconds after the last update: the log
    // stays, and nothing else would have folded it.
    written
        .app
        .collab()
        .write_body(&written.app, &written.card_id)
        .await
        .expect("written");
    let (_, written_end, projected, rows) = stored(&written).await;
    assert_eq!((projected, rows), (written_end, 40), "written, not folded");
    let wanted_unwritten = typed(&unwritten, &lines(25)).await;
    let wanted = [
        (
            written.card_id.clone(),
            wanted_written.body().map(|body| body.document().clone()),
        ),
        (
            unwritten.card_id.clone(),
            wanted_unwritten.body().map(|body| body.document().clone()),
        ),
    ];

    let th = th
        .killed_and_started_again(async |app| app.collab().abandon().await)
        .await;
    for (card_id, body) in &wanted {
        let e = Editing {
            card_id: card_id.clone(),
            ..editing_app(&th).await
        };
        let began = Instant::now();
        loop {
            let (state_seq, log_end, projected, rows) = stored(&e).await;
            if rows == 0 {
                assert_eq!((state_seq, projected), (log_end, log_end), "{card_id}");
                break;
            }
            assert!(
                began.elapsed() < Duration::from_secs(20),
                "folded as the server started: {rows} rows left of {card_id}"
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert_eq!(
            &body_of(&card(&th.client, card_id).await),
            body,
            "{card_id}"
        );
        assert_eq!(&reopened(&th, card_id).await, body, "{card_id}");
    }
    th.teardown().await;
}

/// In a room that stays open, the log never holds a fold's worth of updates: after
/// each, it is folded once it would.
#[tokio::test]
async fn the_log_of_a_room_that_stays_open_does_not_grow() {
    let th = Setup::native().start().await.init_basic().await;
    let e = editing(&th, &paragraphs(&["first"])).await;
    let collab = e.app.collab();
    let (editor, epoch) = joined(collab, &e.app, &e.card_id, 101).await;
    let bound = usize::try_from(FOLD_AFTER).expect("a count");
    for n in 0..(3 * bound + 17) {
        let update = if n % 4 == 3 {
            editor.delete_last()
        } else {
            editor.append_paragraph(&format!("line {n}"))
        };
        collab
            .take(&e.app, &e.card_id, epoch, &e.user1, &update)
            .await
            .expect("taken");
        let (_, _, _, rows) = stored(&e).await;
        assert!(rows < bound, "after update {n}: {rows} rows");
    }
    th.teardown().await;
}
