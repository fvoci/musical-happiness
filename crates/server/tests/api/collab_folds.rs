//! What is stored of a document after its room is done with it: the log folded into
//! the state as the document closes (its room empties, or the server stops), also
//! when the server that held it was killed and the next one starts, and never longer
//! than the fold's bound in a room that stays open. The log's rows are counted in the
//! table, whether or not the state holds them.

use std::time::{Duration, Instant};

use fvoci_collab::testing::Editor;
use fvoci_server::collab::FOLD_AFTER;
use serde_json::Value;

use crate::collab::{Editing, body_of, card, editing, editing_app, joined};
use crate::common::{Setup, TEST_WORKSPACE_ID, TestServer};
use crate::rooms::{Peer, paragraphs, table};

/// The state's last update, the log's last, the last one the card holds, and how many
/// rows the log has.
async fn stored(e: &Editing) -> (i64, i64, i64, usize) {
    let store = e.app.store();
    let stored = store
        .get_body_doc(&e.card_id)
        .await
        .expect("read")
        .expect("a document");
    let rows = store
        .logged_body_updates(&e.card_id)
        .await
        .expect("counted");
    (
        stored.state_seq,
        stored.log_end,
        stored.projected_seq,
        usize::try_from(rows).expect("a count"),
    )
}

/// Waits until what is stored of the document has no log, and says so of it.
async fn folded(e: &Editing, why: &str) {
    let began = Instant::now();
    loop {
        let (state_seq, log_end, projected, rows) = stored(e).await;
        if rows == 0 {
            assert_eq!((state_seq, projected), (log_end, log_end), "{why}");
            return;
        }
        assert!(
            began.elapsed() < Duration::from_secs(20),
            "{why}: {rows} rows left"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
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

/// The room of a card that empties, its last editor gone after 30 updates (fewer
/// than a fold's worth), stores its document as its state alone: the log is empty,
/// and the document opened again is the same.
#[tokio::test]
async fn a_room_that_empties_folds_its_log() {
    let th = Setup::native().start().await.init_basic().await;
    let t = table(&th, &paragraphs(&["first"])).await;
    let mut peer = Peer::connect(&th.client, 101).await;
    peer.joined(TEST_WORKSPACE_ID, &t.card_id).await;
    for line in lines(30) {
        peer.write_acked(&line).await;
    }
    let wanted = peer.body();
    let e = Editing {
        card_id: t.card_id.clone(),
        ..editing_app(&th).await
    };
    let (_, _, _, rows) = stored(&e).await;
    assert_eq!(rows, 30, "logged, not folded yet");

    peer.leave().await;
    folded(&e, "the room emptied").await;
    assert_eq!(body_of(&card(&th.client, &e.card_id).await), wanted);
    assert_eq!(reopened(&th, &e.card_id).await, wanted, "the same document");
    th.teardown().await;
}

/// A document closed through the place of the documents, and the documents of a
/// server that stops, are stored as their state alone.
#[tokio::test]
async fn a_document_closed_or_stopped_folds_its_log() {
    let th = Setup::native().start().await.init_basic().await;
    let closed = editing(&th, &paragraphs(&["closed"])).await;
    let stopped = editing(&th, &paragraphs(&["stopped"])).await;
    let wanted_closed = typed(&closed, &lines(30))
        .await
        .body()
        .map(|body| body.document().clone());
    typed(&stopped, &lines(20)).await;
    closed
        .app
        .collab()
        .close(&closed.app, &closed.card_id)
        .await
        .expect("closed");
    folded(&closed, "closed").await;
    assert_eq!(
        body_of(&card(&th.client, &closed.card_id).await),
        wanted_closed
    );
    assert_eq!(reopened(&th, &closed.card_id).await, wanted_closed);

    stopped.app.collab().end(&stopped.app).await;
    folded(&stopped, "the server stopped").await;
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
    // Its body is written to the card and kept as a version (its time has come): it
    // is finished, and only its log is left, which nothing else would have folded.
    written
        .app
        .collab()
        .pass_version_time(Duration::from_hours(1));
    written
        .app
        .collab()
        .write_body(&written.app, &written.card_id)
        .await
        .expect("written");
    let (_, written_end, projected, rows) = stored(&written).await;
    assert_eq!((projected, rows), (written_end, 40), "written, not folded");
    let unfinished = written
        .app
        .store()
        .unfinished_body_docs()
        .await
        .expect("read");
    assert!(
        unfinished
            .iter()
            .all(|(card_id, _)| *card_id != written.card_id),
        "written and kept: {unfinished:?}"
    );
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
        folded(&e, &format!("as the server started, {card_id}")).await;
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
    let wanted = editor.body().map(|body| body.document().clone());
    collab.abandon().await;
    assert_eq!(
        reopened(&th, &e.card_id).await,
        wanted,
        "the folded state and the log after it"
    );
    th.teardown().await;
}
