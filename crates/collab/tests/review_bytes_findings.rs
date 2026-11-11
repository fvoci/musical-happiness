//! Bytes from anybody, second review: what an update that passes the walk before the
//! Y library can still do to the document, to what is stored and to the server.
//!
//! Each test here states what the server that calls this crate relies on. One that
//! fails names a defect; what it shows is in the message of the assertion.

mod support;

use fvoci_collab::{UpdateError, new_doc, state_of, to_body, with_update};
use support::ROOMY;
use support::wire::{Change, Content, Parent, Update, value};

/// A paragraph under the body's root by client 5, and the update that makes it.
fn paragraph() -> Update {
    Update::of(
        5,
        0,
        vec![Change::under(Parent::body(), Content::element("paragraph"))],
    )
}

/// The library reads one text of JSON more than the number before them says, and
/// writes exactly as many as it says: what it took, it cannot read back. The walk
/// follows the reading, so an update of twelve bytes is taken, and the state of the
/// document it leaves is no update any more. The next update for that document, from
/// anybody, panics where the document is copied.
#[test]
fn an_update_with_texts_of_json_leaves_a_state_that_can_be_read_again() {
    let update = Update::of(
        5,
        0,
        vec![Change::under(
            Parent::body(),
            Content::Json {
                count: 0,
                texts: vec![b"1".to_vec()],
            },
        )],
    )
    .bytes();
    let Ok(with) = with_update(&new_doc(), &update, &ROOMY) else {
        // Refused: the document never holds what it cannot write back.
        return;
    };
    let stored = state_of(&with);
    assert!(
        with_update(&new_doc(), &stored, &ROOMY).is_ok(),
        "the update {update:?} was taken, and the state it leaves ({stored:?}) is refused"
    );
}

/// The same, where it ends the server: the document that took the update is given the
/// next one, an empty update. (This test dies of the panic in `copy_of`: "a document's
/// own state is an update".)
#[test]
fn the_update_after_one_with_texts_of_json_does_not_panic() {
    let update = Update::of(
        5,
        0,
        vec![Change::under(
            Parent::body(),
            Content::Json {
                count: 0,
                texts: vec![b"1".to_vec()],
            },
        )],
    )
    .bytes();
    let Ok(with) = with_update(&new_doc(), &update, &ROOMY) else {
        return;
    };
    let empty = Update::default().bytes();
    let _ = with_update(&with, &empty, &ROOMY);
}

/// A whole number of more than 53 bits is taken in the short form (nine bytes hold 62
/// bits), and written back in the form of 64 bits, which the walk refuses: the state
/// of a document that took the update is refused by `with_update`.
#[test]
fn an_update_with_a_large_whole_number_leaves_a_state_that_is_taken_again() {
    let mut update = paragraph();
    update.runs[0].changes.push(Change::member(
        Parent::Change((5, 0)),
        "n",
        Content::values(vec![value::whole((1 << 53) + 1)]),
    ));
    let update = update.bytes();
    let Ok(with) = with_update(&new_doc(), &update, &ROOMY) else {
        return;
    };
    assert!(to_body(&with).is_ok(), "the paragraph is a body");
    let stored = state_of(&with);
    assert_eq!(
        with_update(&new_doc(), &stored, &ROOMY).map(|_| ()),
        Ok::<(), UpdateError>(()),
        "the update was taken, and the state it leaves is refused"
    );
}
