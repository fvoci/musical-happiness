//! Documents made slow to read: changes that each walk a long list every time the Y
//! library reads the document from its state. An update that leaves one is refused, a
//! document near the steps it may take is made again from its body, and one stored
//! before the steps were counted is opened without ending a worker.

use fvoci_collab::testing::Editor;
use fvoci_collab::{BodyDoc, READING_STEPS, STORED_READING_STEPS, UpdateLimits, reading_steps};
use fvoci_server::collab::DocChange;
use fvoci_server::error::{AppError, Reason};
use fvoci_store::BodyBefore;
use serde_json::Value;

use crate::collab::{Editing, body_of, card, editing, joined, paragraphs};
use crate::collab_written_past::stored;
use crate::common::{Setup, TestServer};

/// The client that types the list, and the one whose changes walk it.
const TYPIST: u64 = 7;
const WALKER: u64 = 1 << 40;

fn number(out: &mut Vec<u8>, mut number: u64) {
    while number >= 0x80 {
        out.push(number.to_le_bytes()[0] | 0x80);
        number >>= 7;
    }
    out.push(number.to_le_bytes()[0]);
}

/// An update of one client's changes from `clock` on, as the Y library writes one
/// (version 1), with nothing deleted.
fn update_of(client: u64, clock: u64, count: u64, changes: &[u8]) -> Vec<u8> {
    let mut out = vec![1];
    for value in [count, client, clock] {
        number(&mut out, value);
    }
    out.extend_from_slice(changes);
    number(&mut out, 0);
    out
}

/// `children` empty paragraphs typed one after the other by one client, the first as
/// a child of the root: each names the one before it to its left.
fn typed_in_order(children: u64) -> Vec<u8> {
    const PARAGRAPH: &[u8] = b"\x03\x09paragraph";
    let mut changes = Vec::new();
    // A shared type (7) under the root of that name.
    changes.extend_from_slice(b"\x07\x01\x07default");
    changes.extend_from_slice(PARAGRAPH);
    for clock in 1..children {
        // A shared type (7) after a neighbour (0x80).
        changes.push(0x87);
        number(&mut changes, TYPIST);
        number(&mut changes, clock - 1);
        changes.extend_from_slice(PARAGRAPH);
    }
    update_of(TYPIST, 0, children, &changes)
}

/// `count` changes from `clock` on, each deleted content of one place (1) that names
/// the last of `children` typed paragraphs to its right (0x40) and nothing to its left:
/// the library walks the whole list to place each, every time it reads the document.
fn walking(clock: u64, count: u64, children: u64) -> Vec<u8> {
    let mut changes = Vec::new();
    for _ in 0..count {
        changes.push(0x41);
        number(&mut changes, TYPIST);
        number(&mut changes, children - 1);
        number(&mut changes, 1);
    }
    update_of(WALKER, clock, count, &changes)
}

fn reason_of(error: &AppError) -> Option<&'static str> {
    error.reason().map(Reason::as_str)
}

/// A server that takes an update large enough to type a long list in.
async fn roomy() -> TestServer {
    Setup::native()
        .configure(|config| config.collab.collab_update_bytes = 1024 * 1024)
        .start()
        .await
        .init_basic()
        .await
}

fn paragraphs_of(body: Option<&Value>) -> usize {
    body.and_then(|body| body["content"].as_array())
        .map_or(0, Vec::len)
}

/// An update that would leave a document past the steps it may take to read is
/// refused, and the document is as it was; one that leaves it at three quarters of
/// them is taken, and the document is made again from its body, which takes none: the
/// room is told, and an editor that loads it again goes on.
#[tokio::test]
async fn a_document_that_takes_many_steps_to_read_is_made_again_from_its_body() {
    const CHILDREN: u64 = 10_000;
    let th = roomy().await;
    let e = editing(&th, &paragraphs(&["kept"])).await;
    let collab = e.app.collab();
    let mut events = collab.subscribe();
    let loaded = collab.open(&e.app, &e.card_id).await.expect("opened");
    let take = async |update: Vec<u8>| {
        collab
            .take(&e.app, &e.card_id, loaded.epoch, &e.user1, &update)
            .await
    };
    let typed = take(typed_in_order(CHILDREN)).await.expect("typed");
    assert!(!typed.remade, "a list typed in order takes no steps");

    let past = READING_STEPS / CHILDREN + 5;
    let refused = take(walking(0, past, CHILDREN))
        .await
        .expect_err("past the steps a document may take");
    assert_eq!(reason_of(&refused), Some("document_too_large"));
    assert_eq!(stored(&e).await.epoch, loaded.epoch, "not made again");

    let near = READING_STEPS * 3 / 4 / CHILDREN + 1;
    let taken = take(walking(0, near, CHILDREN)).await.expect("taken");
    assert!(taken.remade, "made again");
    assert!(
        std::iter::from_fn(|| events.try_recv().ok())
            .any(|event| event.card_id == e.card_id && event.change == DocChange::Remade),
        "the room is told"
    );
    let made = stored(&e).await;
    assert_eq!(
        reading_steps(&made.state, u64::MAX).expect("a state"),
        0,
        "a body's own document"
    );
    let (again, again_epoch) = joined(collab, &e.app, &e.card_id, 102).await;
    let body = again.body().map(|body| body.document().clone());
    assert_eq!(paragraphs_of(body.as_ref()), 10_001);
    assert_eq!(body_of(&card(&th.client, &e.card_id).await), body);
    let on = collab
        .take(
            &e.app,
            &e.card_id,
            again_epoch,
            &e.user1,
            &again.append_paragraph("on"),
        )
        .await
        .expect("an editor that loaded it again goes on");
    assert!(!on.remade, "not made again at every update");
    th.teardown().await;
}

/// A card whose document is stored as updates that were taken before the steps of a
/// reading were counted left it: a list of `CHILDREN` paragraphs typed in order, which
/// its card holds, `walkers` changes that walk the list, and after that state a logged
/// update of one more paragraph, which its card does not hold. No worker runs.
async fn stored_slow(th: &TestServer, walkers: u64) -> (Editing, Vec<u8>) {
    let uncounted = UpdateLimits {
        update_bytes: 1024 * 1024,
        state_bytes: 8 * 1024 * 1024,
        body_bytes: fvoci_model::BODY_MAX_BYTES,
        placing_steps: None,
        clients: None,
        client_changes: None,
        reading_steps: None,
    };
    let e = editing(th, &paragraphs(&["first"])).await;
    let collab = e.app.collab();
    let loaded = collab.open(&e.app, &e.card_id).await.expect("opened");
    collab
        .take(
            &e.app,
            &e.card_id,
            loaded.epoch,
            &e.user1,
            &typed_in_order(CHILDREN),
        )
        .await
        .expect("typed");
    collab.close(&e.app, &e.card_id).await.expect("written");
    collab.abandon().await;
    let before = stored(&e).await;
    assert_eq!(
        before.projected_seq, before.log_end,
        "the card holds the list"
    );
    let state = BodyDoc::from_state(&before.state, &uncounted)
        .expect("read")
        .doc
        .take(&walking(0, walkers, CHILDREN), &uncounted)
        .expect("taken before the steps were counted")
        .doc
        .state();
    let store = e.app.store();
    store
        .fold_body_doc(&e.card_id, &state, before.state_seq, before.as_read())
        .await
        .expect("stored past the server");
    let late = Editor::join(104, &state).append_paragraph("typed late");
    store
        .append_body_updates(
            &e.card_id,
            &[(late.as_slice(), e.user1.as_str())],
            before.as_read(),
            &[],
            &[],
            BodyBefore::Leave,
        )
        .await
        .expect("logged past the server");
    (e, state)
}

/// The paragraphs the documents stored slow hold ([`stored_slow`]).
const CHILDREN: u64 = 20_000;

/// Starts the first worker of a server, by a document opened in it; its process.
async fn worker_of(th: &TestServer, e: &Editing) -> u32 {
    let other = editing(th, &paragraphs(&["other"])).await;
    let collab = e.app.collab();
    collab
        .open(&other.app, &other.card_id)
        .await
        .expect("opened");
    collab.worker_pids().await[0].expect("a worker runs")
}

fn last_text(body: Option<&Value>) -> Option<&str> {
    body?["content"].as_array()?.last()?["content"][0]["text"].as_str()
}

/// A document stored before the steps of a reading were counted, which takes more
/// steps to read than an update may leave and fewer than a stored one may take, is
/// opened with all it holds: its body, with the update its card did not hold yet, is
/// written to the card, and it is made again from that body. The worker that opened it
/// runs on.
#[tokio::test]
async fn a_stored_document_that_takes_many_steps_to_read_is_written_and_made_again() {
    let th = roomy().await;
    let (e, state) = stored_slow(&th, READING_STEPS / CHILDREN + 8).await;
    let steps = reading_steps(&state, u64::MAX).expect("a state");
    assert!(
        steps > READING_STEPS && steps < STORED_READING_STEPS,
        "{steps} steps"
    );
    let before = stored(&e).await;
    let collab = e.app.collab();
    let worker = worker_of(&th, &e).await;
    let mut events = collab.subscribe();

    let (again, again_epoch) = joined(collab, &e.app, &e.card_id, 102).await;
    assert_eq!(
        collab.worker_pids().await[0],
        Some(worker),
        "the worker runs on"
    );
    let body = again.body().map(|body| body.document().clone());
    assert_eq!(paragraphs_of(body.as_ref()), 20_002);
    assert_eq!(last_text(body.as_ref()), Some("typed late"));
    assert_eq!(body_of(&card(&th.client, &e.card_id).await), body);
    assert!(
        std::iter::from_fn(|| events.try_recv().ok())
            .any(|event| event.card_id == e.card_id && event.change == DocChange::Remade),
        "the room is told"
    );
    let made = stored(&e).await;
    assert_eq!(made.epoch, again_epoch);
    assert_eq!(made.epoch, before.epoch + 1, "made again");
    assert_eq!(reading_steps(&made.state, u64::MAX).expect("a state"), 0);
    th.teardown().await;
}

/// A document stored before the steps of a reading were counted that would take a
/// worker longer to read than it has is not read: it is made again from its card's
/// body, without the update the card did not hold, and the log after it is dropped.
/// The worker it was opened in runs on, and the document is edited.
#[tokio::test]
async fn a_stored_document_no_worker_reads_in_time_is_made_again_from_its_card() {
    let th = roomy().await;
    let (e, state) = stored_slow(&th, STORED_READING_STEPS / CHILDREN + 8).await;
    let steps = reading_steps(&state, u64::MAX).expect("a state");
    assert!(steps > STORED_READING_STEPS, "{steps} steps");
    let before = stored(&e).await;
    let collab = e.app.collab();
    let worker = worker_of(&th, &e).await;

    let (again, again_epoch) = joined(collab, &e.app, &e.card_id, 102).await;
    assert_eq!(
        collab.worker_pids().await[0],
        Some(worker),
        "the worker runs on"
    );
    let body = again.body().map(|body| body.document().clone());
    assert_eq!(paragraphs_of(body.as_ref()), 20_001, "the card's body");
    assert_eq!(body_of(&card(&th.client, &e.card_id).await), body);
    let made = stored(&e).await;
    assert_eq!(made.epoch, before.epoch + 1, "made again");
    assert_eq!(
        (made.updates.len(), made.projected_seq),
        (0, made.log_end),
        "the log is dropped"
    );
    assert_eq!(reading_steps(&made.state, u64::MAX).expect("a state"), 0);
    collab
        .take(
            &e.app,
            &e.card_id,
            again_epoch,
            &e.user1,
            &again.append_paragraph("on"),
        )
        .await
        .expect("edited");
    th.teardown().await;
}

/// Updates that were logged before the steps of a reading were counted, and together
/// leave a document no worker reads in time, are not taken again as the document is
/// opened: it is made again from its card's body, and the worker runs on.
#[tokio::test]
async fn logged_updates_that_leave_a_document_no_worker_reads_are_not_taken_again() {
    let th = roomy().await;
    let e = editing(&th, &paragraphs(&["first"])).await;
    let collab = e.app.collab();
    collab.open(&e.app, &e.card_id).await.expect("opened");
    collab.abandon().await;
    let each = STORED_READING_STEPS / CHILDREN / 2 + 8;
    let updates = [
        typed_in_order(CHILDREN),
        walking(0, each, CHILDREN),
        walking(each, each, CHILDREN),
    ];
    let entries: Vec<(&[u8], &str)> = updates
        .iter()
        .map(|update| (update.as_slice(), e.user1.as_str()))
        .collect();
    let before = stored(&e).await;
    e.app
        .store()
        .append_body_updates(
            &e.card_id,
            &entries,
            before.as_read(),
            &[],
            &[],
            BodyBefore::Leave,
        )
        .await
        .expect("logged past the server");
    let worker = worker_of(&th, &e).await;

    let (again, _) = joined(collab, &e.app, &e.card_id, 102).await;
    assert_eq!(
        collab.worker_pids().await[0],
        Some(worker),
        "the worker runs on"
    );
    assert_eq!(
        again.body().map(|body| body.document().clone()),
        Some(paragraphs(&["first"])),
        "the card's body"
    );
    let made = stored(&e).await;
    assert_eq!(made.epoch, before.epoch + 1, "made again");
    assert_eq!(made.updates.len(), 0, "the log is dropped");
    th.teardown().await;
}
