//! What editors send to the largest documents is taken: a letter typed, pastes, undos
//! and what an editor sends when it is back from offline, into documents of as many
//! nodes, and as many bytes, as a body may have. How long each takes is measured in a
//! release build (`cost.rs`, `honest`). None of it comes near the steps a document may
//! take to read: a body's own document takes none, and an edit a few thousand.

mod support;

use std::time::{Duration, Instant};

use fvoci_collab::{
    BodyDoc, MAX_CLIENT_CHANGES, READING_STEPS, client_changes, reading_steps, take_update,
};
use fvoci_model::{BODY_MAX_BYTES, BODY_MAX_NODES};
use support::body_of;
use support::honest::{Between, SERVED, bodies, document, edits, marked, marked_paragraph};

/// The bodies of the most changes of one client and the largest states there are (of
/// the ways tried: letters of every mark in turn with letters of none, line breaks or
/// formulas, of every mark but the link, of a few, in a paragraph or in many) are made
/// into documents under three quarters of the changes a client may have, and within
/// the state a document may be, and are read in no steps: a document made from a body is
/// not made again at once (`OpenDoc::crowded`), and is held.
#[test]
fn the_bodies_of_the_most_changes_fit_a_document() {
    for between in Between::ALL {
        let body = body_of(&marked(between, BODY_MAX_NODES, BODY_MAX_BYTES));
        let state = BodyDoc::from_body(Some(&body)).state();
        let changes = client_changes(&state).expect("a state made here");
        assert!(
            changes * 4 < MAX_CLIENT_CHANGES * 3,
            "{between:?}: {changes} changes of one client"
        );
        let bytes = state.len();
        assert!(
            bytes <= SERVED.state_bytes,
            "{between:?}: {bytes} bytes of state"
        );
        let steps = reading_steps(&state, READING_STEPS).expect("a state made here");
        assert_eq!(steps, 0, "{between:?}: steps to read it");
    }
}

/// One paragraph of letters in turn with every mark and with none, as many as a body
/// may have, is made into a document well within a worker's time: each mark is written
/// in one pass over the text (0.6 s for 49,600 letters, release). Its runs still each
/// cut the text and move the changes after the cut, more than the text's length. Written
/// a letter at a time, each walked the text from its start, and 8,000 such letters took
/// 9 s.
#[test]
fn a_paragraph_of_many_marks_is_made_well_within_a_workers_time() {
    let document = marked_paragraph(BODY_MAX_NODES, BODY_MAX_BYTES);
    let letters = document["content"][0]["content"]
        .as_array()
        .map_or(0, Vec::len);
    assert!(letters > 40_000, "{letters} letters");
    let body = body_of(&document);
    let start = Instant::now();
    let doc = BodyDoc::from_body(Some(&body));
    let took = start.elapsed();
    assert!(
        took < Duration::from_secs(5),
        "{letters} letters made in {took:?}"
    );
    let state = doc.state();
    let changes = client_changes(&state).expect("a state made here");
    assert!(
        changes * 4 < MAX_CLIENT_CHANGES * 3,
        "{changes} changes of one client"
    );
    let steps = reading_steps(&state, READING_STEPS).expect("a state made here");
    assert_eq!(steps, 0, "steps to read it");
}

/// What editors send is taken, and leaves a document far from the steps a document may
/// take to read.
#[test]
fn what_editors_send_to_the_largest_documents_is_taken() {
    for (name, body) in bodies() {
        let doc = document(&body);
        for edit in edits(&doc, &SERVED) {
            assert!(
                edit.update.len() <= SERVED.update_bytes,
                "{name}, {}: {} bytes",
                edit.name,
                edit.update.len()
            );
            let taken = match take_update(&edit.doc, &edit.update, &SERVED) {
                Ok(taken) => taken,
                Err(error) => panic!("{name}, {}: {error}", edit.name),
            };
            // Far from the steps at which a document is made again (three quarters):
            // the changes of the body's client, each looked at once while the edit
            // waits for the one it names.
            assert!(
                taken.reading_steps * 64 < READING_STEPS,
                "{name}, {}: {} steps to read it",
                edit.name,
                taken.reading_steps
            );
        }
    }
}
