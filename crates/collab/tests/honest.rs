//! What editors send to the largest documents is taken: a letter typed, pastes, undos
//! and what an editor sends when it is back from offline, into documents of as many
//! nodes, and as many bytes, as a body may have. How long each takes is measured in a
//! release build (`cost.rs`, `honest`).

mod support;

use fvoci_collab::{BodyDoc, MAX_CLIENT_CHANGES, take_update};
use fvoci_model::{BODY_MAX_BYTES, BODY_MAX_NODES};
use support::body_of;
use support::honest::{SERVED, bodies, document, edits, marked};

/// The body of the most changes of one client and the largest state there is (of the
/// ways tried: marks in turn with letters of none, of every mark, of every mark but the
/// link, of a few) is made into a document under three quarters of either limit: a
/// document made from a body is not made again at once (`OpenDoc::crowded`), and is
/// held. It is 296,894 changes and 7.38 MB.
#[test]
fn the_body_of_the_most_changes_fits_a_document() {
    let doc = BodyDoc::from_body(Some(&body_of(&marked(BODY_MAX_NODES, BODY_MAX_BYTES))));
    let changes = doc.client_changes();
    assert!(
        changes * 4 < MAX_CLIENT_CHANGES * 3,
        "{changes} changes of one client"
    );
    let state = doc.state().len();
    assert!(state <= SERVED.state_bytes, "{state} bytes of state");
}

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
            if let Err(error) = take_update(&edit.doc, &edit.update, &SERVED) {
                panic!("{name}, {}: {error}", edit.name);
            }
        }
    }
}
