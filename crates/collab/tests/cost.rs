//! What the worker that holds a document pays for it at the limits of a body, measured
//! and printed. Nothing here runs with the other tests: each is ignored, and is run by
//! name in a release build,
//!
//! ```text
//! cargo test --release -p fvoci-collab --test cost -- --ignored --nocapture times
//! cargo test --release -p fvoci-collab --test cost -- --ignored --nocapture memory
//! cargo test --release -p fvoci-collab --test cost -- --ignored --nocapture honest
//! ```
//!
//! The times are of one thread.

mod support;

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use fvoci_collab::testing::{changes_of, placing_steps, to_body};
use fvoci_collab::{
    BodyDoc, MAX_CLIENT_CHANGES, MAX_CLIENTS, PLACING_STEPS, UpdateLimits, from_body, from_state,
    new_doc, state_of, take_update,
};
use fvoci_model::{BODY_MAX_BYTES, BODY_MAX_DEPTH, BODY_MAX_NODES};
use serde_json::{Value, json};
use support::honest::Between;
use support::wire::{Change, Content, Parent, Update};
use support::{body_of, edited, honest};
use yrs::{Doc, XmlElementPrelim, XmlFragment, XmlTextPrelim};

/// The limits the server holds an update to with its settings as they come.
const LIMITS: UpdateLimits = UpdateLimits {
    update_bytes: 64 * 1024,
    state_bytes: 8 * 1024 * 1024,
    body_bytes: BODY_MAX_BYTES,
    placing_steps: Some(PLACING_STEPS),
    clients: Some(MAX_CLIENTS),
    client_changes: Some(MAX_CLIENT_CHANGES),
};

/// The nodes a shape leaves free: a paragraph with a text is appended to it.
const SPARE_NODES: usize = 4;

fn paragraph(text: &str) -> Value {
    json!({"type": "paragraph", "content": [{"type": "text", "text": text}]})
}

fn document(blocks: &[Value]) -> Value {
    json!({"type": "doc", "content": blocks})
}

/// A chain of quotes down to the deepest a body goes, with a text at its end, and how
/// many nodes it is.
fn chain() -> (Value, usize) {
    let mut node = paragraph("deep");
    // The document, the quotes, the paragraph and its text.
    let quotes = BODY_MAX_DEPTH - 3;
    for _ in 0..quotes {
        node = json!({"type": "blockquote", "content": [node]});
    }
    (node, quotes + 2)
}

/// The bodies that are measured, each as large as a body may be but for
/// [`SPARE_NODES`], by name.
fn shapes() -> Vec<(&'static str, Value)> {
    let nodes = BODY_MAX_NODES - SPARE_NODES - 1;
    let list = || {
        let items: Vec<Value> = (0..10)
            .map(|_| json!({"type": "listItem", "content": [paragraph("entry")]}))
            .collect();
        json!({"type": "bulletList", "content": items})
    };
    let (chain, chain_nodes) = chain();
    let long = "가나다라마바사아자차".repeat(100);
    let long_bytes = serde_json::to_vec(&paragraph(&long)).expect("JSON").len() + 1;
    vec![
        ("flat", document(&vec![paragraph("a paragraph"); nodes / 2])),
        ("lists", document(&vec![list(); nodes / 31])),
        ("deep", document(&vec![chain; nodes / chain_nodes])),
        (
            "text",
            document(&vec![
                paragraph(&long);
                (BODY_MAX_BYTES - 1024) / long_bytes
            ]),
        ),
        ("wide", document(&vec![json!({"type": "paragraph"}); nodes])),
        (
            "breaks",
            document(&[
                json!({"type": "paragraph", "content": vec![json!({"type": "hardBreak"}); nodes - 1]}),
            ]),
        ),
        (
            "marks",
            honest::marked(Between::Letter, nodes, BODY_MAX_BYTES - 1024),
        ),
        (
            "marks and breaks",
            honest::marked(Between::Break, nodes, BODY_MAX_BYTES - 1024),
        ),
        (
            "marks and formulas",
            honest::marked(Between::Math, nodes, BODY_MAX_BYTES - 1024),
        ),
        (
            "marked line",
            honest::marked_paragraph(nodes, BODY_MAX_BYTES - 1024),
        ),
    ]
}

/// The update of a paragraph typed at the end.
fn typed(doc: &Doc) -> Vec<u8> {
    edited(doc, |root, txn| {
        let paragraph = root.push_back(txn, XmlElementPrelim::empty("paragraph"));
        paragraph.push_back(txn, XmlTextPrelim::new("typed"));
    })
}

/// The update of a node no body has at the end: taking it repairs the document.
fn unknown(doc: &Doc) -> Vec<u8> {
    edited(doc, |root, txn| {
        root.push_back(txn, XmlElementPrelim::empty("nothing-a-body-has"));
    })
}

/// The middle of three runs.
fn timed<T>(mut run: impl FnMut() -> T) -> (Duration, T) {
    let mut runs: Vec<(Duration, T)> = (0..3)
        .map(|_| {
            let start = Instant::now();
            let value = run();
            (start.elapsed(), value)
        })
        .collect();
    runs.sort_by_key(|(took, _)| *took);
    runs.swap_remove(1)
}

fn millis(took: Duration) -> String {
    format!("{:.1}", took.as_secs_f64() * 1000.0)
}

#[test]
#[ignore = "a measurement, run by name in a release build"]
fn times() {
    println!(
        "| shape | nodes | body | state | from_body | project | take | take+repair | from_state | lists |"
    );
    println!("| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |");
    for (name, document) in shapes() {
        let body = body_of(&document);
        let body_bytes = serde_json::to_vec(body.document()).expect("JSON").len();
        let (made, doc) = timed(|| from_body(&body));
        let state = state_of(&doc);
        let (projected, read) = timed(|| to_body(&doc).expect("a body"));
        assert!(read.is_some(), "{name}: a body");
        let update = typed(&doc);
        let (taken, with) = timed(|| take_update(&doc, &update, &LIMITS).expect("taken"));
        assert_eq!(with.repair.deleted, 0, "{name}: nothing to repair");
        let update = unknown(&doc);
        let (repaired, with) = timed(|| take_update(&doc, &update, &LIMITS).expect("repaired"));
        assert_eq!(with.repair.deleted, 1, "{name}: the unknown node went");
        let (opened, _) = timed(|| from_state(&state, &LIMITS).expect("opened"));
        // Reading the lists of the document, which an update of many changes pays
        // on top of being taken.
        let (lists, _) = timed(|| placing_steps(&doc, &update).expect("counted"));
        println!(
            "| {name} | {} | {:.2} MB | {:.2} MB | {} | {} | {} | {} | {} | {} |",
            nodes_of(&document),
            mega(body_bytes),
            mega(state.len()),
            millis(made),
            millis(projected),
            millis(taken),
            millis(repaired),
            millis(opened),
            millis(lists),
        );
    }
}

/// How long what editors send to the largest documents takes ([`support::honest`]),
/// and making such a document again from its body, as the restore of a version does:
/// each within [`honest::LONGEST`] for the documents of the shapes people write
/// ([`honest::WRITTEN`]).
#[test]
#[ignore = "a measurement, run by name in a release build"]
fn honest() {
    println!("| document | edit | bytes | ms |");
    println!("| --- | --- | --- | --- |");
    let mut slow = Vec::new();
    for (name, document) in honest::bodies() {
        let body = body_of(&document);
        let (made, (state, changes)) = timed(|| {
            let made = BodyDoc::from_body(Some(&body));
            let state = made.state();
            let changes = fvoci_collab::client_changes(&state).expect("a state made here");
            (state.len(), changes)
        });
        println!(
            "| {name} | made again from its body: a state of {state} B, {changes} changes of its writer | | {} |",
            millis(made)
        );
        let held = honest::WRITTEN.contains(&name);
        if held && made > honest::LONGEST {
            slow.push((name, "made again from its body"));
        }
        let doc = honest::document(&document);
        for edit in honest::edits(&doc, &LIMITS) {
            let (took, _) = timed(|| take_update(&edit.doc, &edit.update, &LIMITS).expect("taken"));
            println!(
                "| {name} | {} | {} | {} |",
                edit.name,
                edit.update.len(),
                millis(took)
            );
            if held && took > honest::LONGEST {
                slow.push((name, edit.name));
            }
        }
    }
    assert!(slow.is_empty(), "past {:?}: {slow:?}", honest::LONGEST);
}

fn mega(bytes: usize) -> f64 {
    f64::from(u32::try_from(bytes).expect("a size of megabytes")) / 1e6
}

fn nodes_of(node: &Value) -> usize {
    1 + node["content"]
        .as_array()
        .map_or(0, |children| children.iter().map(nodes_of).sum())
}

/// An update of `count` changes of one client, each deleted content of one place that
/// names the root as its parent and no neighbour: the library walks past every child
/// of the root that names none to its left either, which is all of them where the
/// server made the document from a body, to place each.
fn crowding(client: u64, count: usize) -> Vec<u8> {
    let changes = (0..count)
        .map(|_| Change::under(Parent::body(), Content::Deleted(1)))
        .collect();
    Update::of(client, 0, changes).bytes()
}

/// A document of `children` empty paragraphs typed one after the other by one client:
/// each names the one before it to its left, so that the library goes past each by
/// what it passed already, which takes longer than past children made from a body.
fn typed_in_order(children: usize) -> Doc {
    let changes = (0..u64::try_from(children).expect("a count fits"))
        .map(|clock| match clock.checked_sub(1) {
            None => Change::under(Parent::body(), Content::element("paragraph")),
            Some(before) => Change::after((7, before), Content::element("paragraph")),
        })
        .collect();
    let roomy = UpdateLimits {
        update_bytes: usize::MAX,
        state_bytes: usize::MAX,
        placing_steps: None,
        clients: None,
        client_changes: None,
        ..LIMITS
    };
    take_update(&new_doc(), &Update::of(7, 0, changes).bytes(), &roomy)
        .expect("typed")
        .doc
}

/// How long one update that is made to be slow takes, by the children of the root it
/// is walked through, how they were made, and the changes it holds.
#[test]
#[ignore = "a measurement, run by name in a release build"]
fn crowded() {
    println!("| children | made | changes | update | outcome | ms |");
    println!("| --- | --- | --- | --- | --- | --- |");
    let filter: Option<usize> = std::env::var("FVOCI_COST_CHILDREN")
        .ok()
        .and_then(|children| children.parse().ok());
    for children in [1_000, 10_000, 25_000, BODY_MAX_NODES - 2] {
        if filter.is_some_and(|only| only != children) {
            continue;
        }
        let body = body_of(&document(&vec![json!({"type": "paragraph"}); children]));
        for (made, doc) in [
            ("from a body", from_body(&body)),
            ("typed in order", typed_in_order(children)),
        ] {
            for row in crowd(children, made, &doc) {
                println!("{row}");
            }
        }
        for row in walked_again(children, &typed_in_order(children)) {
            println!("{row}");
        }
    }
}

/// How long the most changes the limit takes of a client below the document's take,
/// each named after the middle child and before a child ahead of it, so that the
/// library walks on through the second half: once as the update is taken and again
/// every time it reads the document again. With a node no body has, the update is
/// repaired, which reads it twice more. A row of the table each.
fn walked_again(children: usize, doc: &Doc) -> Vec<String> {
    let children = u64::try_from(children).expect("a count fits");
    let walking = |count: u64, repaired: bool| {
        let mut changes: Vec<Change> = (0..count)
            .map(|right| Change::Item {
                left: Some((7, children / 2)),
                right: Some((7, right)),
                parent: Parent::body(),
                key: None,
                content: Content::Deleted(1),
            })
            .collect();
        if repaired {
            changes.push(Change::after(
                (7, children - 1),
                Content::element("nothing-a-body-has"),
            ));
        }
        Update::of(1, 0, changes).bytes()
    };
    let within =
        |count: u64| placing_steps(doc, &walking(count, true)).expect("counted") <= PLACING_STEPS;
    let (mut most, mut past) = (0_u64, children / 2);
    while past - most > 1 {
        let count = most.midpoint(past);
        if within(count) {
            most = count;
        } else {
            past = count;
        }
    }
    [
        ("a client below", false),
        ("a client below, repaired", true),
    ]
    .into_iter()
    .map(|(made, repaired)| {
        let update = walking(most, repaired);
        let start = Instant::now();
        let outcome = match take_update(doc, &update, &LIMITS) {
            Ok(_) => "taken".to_owned(),
            Err(error) => format!("refused: {error}"),
        };
        format!(
            "| {children} | {made} | {most} | {} B | {outcome} | {} |",
            update.len(),
            millis(start.elapsed())
        )
    })
    .collect()
}

/// How long updates that crowd the root of `doc`, of `children`, take: a row of the
/// table each.
fn crowd(children: usize, made: &str, doc: &Doc) -> Vec<String> {
    // The most changes the limit takes: each walks the children and each other.
    let most = (1..usize::MAX)
        .take_while(|changes: &usize| {
            let steps = changes * (children + changes);
            u64::try_from(steps).is_ok_and(|steps| steps <= PLACING_STEPS)
        })
        .last()
        .unwrap_or(1);
    [100, most, 1_000, 5_400]
        .into_iter()
        .map(|count| {
            // Above every client the server takes at random (32 bits), so that each
            // change is placed after the children and not before them.
            let update = crowding(1 << 40, count);
            let start = Instant::now();
            let outcome = match take_update(doc, &update, &LIMITS) {
                Ok(_) => "taken".to_owned(),
                Err(error) => format!("refused: {error}"),
            };
            format!(
                "| {children} | {made} | {count} | {} B | {outcome} | {} |",
                update.len(),
                millis(start.elapsed())
            )
        })
        .collect()
}

/// The allocator of this test: the system's, which counts what is allocated now and
/// the most that was, so that the memory of a document is read without what the
/// allocator keeps for itself.
struct Counting;

static ALLOCATED: AtomicUsize = AtomicUsize::new(0);
static MOST: AtomicUsize = AtomicUsize::new(0);

// SAFETY: every call is handed on to the system's allocator with the arguments it was
// given; the counters beside it hold no memory.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let now = ALLOCATED.fetch_add(layout.size(), Ordering::Relaxed) + layout.size();
        MOST.fetch_max(now, Ordering::Relaxed);
        // SAFETY: the caller keeps the contract of `alloc` for this layout.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        ALLOCATED.fetch_sub(layout.size(), Ordering::Relaxed);
        // SAFETY: the caller gives a block this allocator gave it with this layout.
        unsafe { System.dealloc(pointer, layout) }
    }
}

#[global_allocator]
static COUNTING: Counting = Counting;

/// What is allocated now, in bytes.
fn allocated() -> usize {
    ALLOCATED.load(Ordering::Relaxed)
}

/// The most that is allocated while `run` runs, above what was allocated before it.
fn most_during<T>(run: impl FnOnce() -> T) -> usize {
    let before = allocated();
    MOST.store(before, Ordering::Relaxed);
    drop(run());
    MOST.load(Ordering::Relaxed).saturating_sub(before)
}

/// The memory of each shape: what the document a worker holds takes, and the most an
/// update takes above it while it is taken, with a repair and without. Bytes the
/// program asked for; what the allocator keeps around them comes on top.
#[test]
#[ignore = "a measurement, run by name in a release build"]
fn memory() {
    println!("| shape | state | changes | held | take | take+repair |");
    println!("| --- | --- | --- | --- | --- | --- |");
    for (name, document) in shapes() {
        let state = state_of(&from_body(&body_of(&document)));
        drop(document);
        let before = allocated();
        let held = from_state(&state, &LIMITS).expect("opened").doc;
        let holding = allocated().saturating_sub(before);
        let typed = typed(&held);
        let unknown = unknown(&held);
        let taking = most_during(|| take_update(&held, &typed, &LIMITS).expect("taken"));
        let repairing = most_during(|| take_update(&held, &unknown, &LIMITS).expect("repaired"));
        let per = |bytes: usize| mega(bytes) / mega(state.len());
        println!(
            "| {name} | {:.2} MB | {} | {:.1} MB ({:.1}x) | +{:.1} MB ({:.1}x) | +{:.1} MB ({:.1}x) |",
            mega(state.len()),
            changes_of(&state),
            mega(holding),
            per(holding),
            mega(taking),
            per(taking),
            mega(repairing),
            per(repairing),
        );
    }
}
