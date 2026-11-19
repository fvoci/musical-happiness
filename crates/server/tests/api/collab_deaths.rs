//! Editing a card's body together when the worker process dies or does not answer, and
//! what is past the limits of a document.

use fvoci_collab::testing::Editor;
use fvoci_server::collab::DocChange;
use fvoci_server::error::{AppError, Reason};
use serde_json::{Value, json};

use crate::collab::{Editing, body_of, card, editing, joined};
use crate::common::{Setup, TestServer};

fn paragraphs(texts: &[&str]) -> Value {
    let content: Vec<Value> = texts
        .iter()
        .map(|text| json!({"type": "paragraph", "content": [{"type": "text", "text": text}]}))
        .collect();
    json!({"type": "doc", "content": content})
}

fn reason_of(error: &AppError) -> Option<&'static str> {
    error.reason().map(Reason::as_str)
}

async fn worker_pid(e: &Editing) -> u32 {
    let pids = e.app.collab().worker_pids().await;
    pids[0].expect("a worker runs")
}

#[cfg(unix)]
pub(crate) fn kill(pid: u32) {
    let status = std::process::Command::new("kill")
        .args(["-KILL", &pid.to_string()])
        .status()
        .expect("kill runs");
    assert!(status.success(), "kill -KILL {pid}");
}

/// A server whose workers give up on a request after 5 s.
async fn impatient() -> TestServer {
    Setup::native()
        .configure(|config| config.collab.collab_worker_timeout_ms = 5000)
        .start()
        .await
        .init_basic()
        .await
}

/// A worker killed with a document open: the server goes on answering, the next
/// request about the document starts another worker and opens the document again from
/// what is stored, with every update it took, and the room is told.
#[cfg(unix)]
#[tokio::test]
async fn a_killed_worker_leaves_the_server_answering_and_the_document_opens_again() {
    let th = Setup::native().start().await.init_basic().await;
    let e = editing(&th, &paragraphs(&["first"])).await;
    let collab = e.app.collab();
    let mut events = collab.subscribe();
    let (editor, editor_epoch) = joined(collab, &e.app, &e.card_id, 101).await;
    for line in ["one", "two"] {
        collab
            .take(
                &e.app,
                &e.card_id,
                editor_epoch,
                &e.user1,
                &editor.append_paragraph(line),
            )
            .await
            .expect("taken");
    }
    let wanted = editor.body().map(|body| body.document().clone());
    let killed = worker_pid(&e).await;
    kill(killed);
    // The server finds the worker gone (here: before the next request is asked of it,
    // which is not counted against its document).
    while e.app.collab().worker_pids().await[0].is_some() {
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }

    let read = card(&th.client, &e.card_id).await;
    assert_eq!(read.id, e.card_id, "the server answers");
    let (again, again_epoch) = joined(collab, &e.app, &e.card_id, 102).await;
    assert_eq!(again.body().map(|body| body.document().clone()), wanted);
    assert_ne!(worker_pid(&e).await, killed, "another worker");
    let event = events.try_recv().expect("an event");
    assert_eq!(
        (event.card_id, event.change),
        (e.card_id.clone(), DocChange::Reopened)
    );
    collab
        .take(
            &e.app,
            &e.card_id,
            again_epoch,
            &e.user1,
            &again.append_paragraph("three"),
        )
        .await
        .expect("taken by the new worker");
    collab.close(&e.app, &e.card_id).await.expect("closed");
    assert_eq!(
        body_of(&card(&th.client, &e.card_id).await),
        again.body().map(|body| body.document().clone())
    );
    th.teardown().await;
}

/// A request that the worker does not answer in time ends the worker and is refused;
/// the server answers meanwhile, and the document opens again from what is stored. A
/// document whose requests end the worker twice, a little apart, is closed for a while,
/// even if it was closed between them.
#[tokio::test]
async fn a_request_past_the_time_limit_ends_the_worker_and_is_refused() {
    let th = impatient().await;
    let e = editing(&th, &paragraphs(&["first"])).await;
    let other = editing(&th, &paragraphs(&["elsewhere"])).await;
    let collab = e.app.collab();
    let (editor, editor_epoch) = joined(collab, &e.app, &e.card_id, 101).await;
    collab
        .take(
            &e.app,
            &e.card_id,
            editor_epoch,
            &e.user1,
            &editor.append_paragraph("one"),
        )
        .await
        .expect("taken");
    let wanted = editor.body().map(|body| body.document().clone());
    let before = worker_pid(&e).await;

    let stalled = collab
        .stall_document(&e.app, &e.card_id, 30_000)
        .await
        .expect_err("past the time limit");
    assert_eq!(reason_of(&stalled), Some("document_unavailable"));
    let read = card(&th.client, &e.card_id).await;
    assert_eq!(read.id, e.card_id, "the server answers");
    let (again, _) = joined(collab, &e.app, &e.card_id, 102).await;
    assert_eq!(again.body().map(|body| body.document().clone()), wanted);
    assert_ne!(worker_pid(&e).await, before, "another worker");
    // Closed and opened again, it remembers the death.
    collab.close(&e.app, &e.card_id).await.expect("closed");

    let stalled = collab
        .stall_document(&e.app, &e.card_id, 30_000)
        .await
        .expect_err("past the time limit");
    assert_eq!(reason_of(&stalled), Some("document_unavailable"));
    let closed = collab
        .open(&e.app, &e.card_id)
        .await
        .expect_err("closed for a while");
    assert_eq!(reason_of(&closed), Some("document_unavailable"));
    collab
        .open(&other.app, &other.card_id)
        .await
        .expect("another card's document opens");
    th.teardown().await;
}

/// A worker holds no more documents than it may; an update larger than the limit is
/// refused, and so is one whose body would be larger than a body may be. What was
/// refused is in neither the document nor its log.
#[tokio::test]
async fn what_is_past_the_limits_of_a_document_is_refused() {
    let th = Setup::native()
        .configure(|config| config.collab.collab_documents_per_worker = 2)
        .start()
        .await
        .init_basic()
        .await;
    let first = editing(&th, &paragraphs(&["first"])).await;
    let second = editing(&th, &paragraphs(&["second"])).await;
    let third = editing(&th, &paragraphs(&["third"])).await;
    let collab = first.app.collab();
    let loaded = collab
        .open(&first.app, &first.card_id)
        .await
        .expect("opened");
    collab
        .open(&second.app, &second.card_id)
        .await
        .expect("opened");
    let refused = collab
        .open(&third.app, &third.card_id)
        .await
        .expect_err("a third document");
    assert_eq!(reason_of(&refused), Some("too_many_documents"));
    collab
        .close(&second.app, &second.card_id)
        .await
        .expect("closed");
    collab
        .open(&third.app, &third.card_id)
        .await
        .expect("room for it once another is closed");

    let editor = Editor::join(101, &loaded.state);
    let large = editor.append_paragraph(&"x".repeat(65 * 1024));
    let refused = collab
        .take(
            &first.app,
            &first.card_id,
            loaded.epoch,
            &first.user1,
            &large,
        )
        .await
        .expect_err("larger than an update may be");
    assert_eq!(reason_of(&refused), Some("update_too_large"));

    let editor = Editor::join(102, &loaded.state);
    let mut refused = None;
    let mut taken = 0;
    // As many as a body has room for, and one more.
    for _ in 0..=fvoci_model::BODY_MAX_BYTES / (60 * 1024) {
        let update = editor.append_paragraph(&"y".repeat(60 * 1024));
        match collab
            .take(
                &first.app,
                &first.card_id,
                loaded.epoch,
                &first.user1,
                &update,
            )
            .await
        {
            Ok(_) => taken += 1,
            Err(error) => {
                refused = Some(error);
                break;
            }
        }
    }
    let refused = refused.expect("a body larger than a body may be is refused");
    assert_eq!(reason_of(&refused), Some("document_too_large"));
    collab
        .close(&first.app, &first.card_id)
        .await
        .expect("the body that was taken is written");
    // Opened again from its stored state and log.
    let (held, _) = joined(collab, &first.app, &first.card_id, 103).await;
    let body = held.body().expect("a body");
    let paragraphs = body.document()["content"].as_array().map_or(0, Vec::len);
    assert_eq!(
        paragraphs,
        1 + taken,
        "the first and those that were taken, not the refused ones"
    );
    assert_eq!(
        body_of(&card(&th.client, &first.card_id).await),
        Some(body.document().clone())
    );
    th.teardown().await;
}
