//! Updates from anybody that passed the walk before the Y library and still broke what
//! is stored or what the server relies on, each kept as a test of its own.
//!
//! Each test here states what the server that calls this crate relies on. One that
//! fails names a defect; what it shows is in the message of the assertion.

mod support;

use fvoci_collab::testing::{repair, to_body, with_update};
use fvoci_collab::{TakeError, UpdateError, new_doc, state_of, take_update};
use support::wire::{Change, Content, Parent, Update, value};
use support::{ROOMY, from_base64};

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

/// The first case the search found (seed 111): an update leaves a text that holds
/// something that is no text, and with what fails deleted the root would hold
/// something that is neither a node nor a text. No repair mends that: the repair
/// fails, and deletes nothing, where it used to make its deletions first.
#[test]
fn a_repair_that_fails_leaves_the_document_as_it_was() {
    let before = from_base64(concat!(
        "ASQHAAcBB2RlZmF1bHQDC29yZGVyZWRMaXN0BwAHAAMIbGlzdEl0ZW0HAAcBAwlwYXJhZ3JhcGgHAAcC",
        "BgQABwMHd29yZAkiXIQHChvDqfCfkanigI3wn5Gp4oCN8J+Rp+KAjfCfkaZGBwQEbGluaxR7ImhyZWYi",
        "OiIvYm9hcmRzLzUifcYHCgcLBGxpbmsEbnVsbCgABwAFc3RhcnQBfQFHBwADCXBhcmFncmFwaAcABxoG",
        "BAAHGwEKhAccBQk8Yj5hRgccBGxpbmsUeyJocmVmIjoiL2JvYXJkcy8wIn3GBxwHHQRsaW5rBG51bGxG",
        "ByIGaXRhbGljAnt9wQcjBx0BRgckBnN0cmlrZQJ7fcEHJQcdAUYHJgRjb2RlAnt9wQcnBx0BhgchBml0",
        "YWxpYwRudWxshgcqBnN0cmlrZQRudWxshgcrBGNvZGUEbnVsbEcHGwMKaW5saW5lTWF0aCgABy0FbGF0",
        "ZXgBdxwJ8J+RqeKAjfCfkanigI3wn5Gn4oCN8J+RpsOpRwctBgQABy8cCSJc8J+RqeKAjfCfkanigI3w",
        "n5Gn4oCN8J+RpkYHMARib2xkAnt9hgc9BGJvbGQEbnVsbEYHPgZpdGFsaWMCe32GBz8GaXRhbGljBG51",
        "bGxGB0AGc3RyaWtlAnt9hgdBBnN0cmlrZQRudWxsRgdCBGNvZGUCe32GB0MEY29kZQRudWxsAQcDJQEn",
        "ASkB",
    ));
    let update = from_base64(concat!(
        "AwAHRgYDAIEHMQEGAQdkZWZhdWx0DmJvbGQtLWFiY2RlZmdoC1sxLFsyLFszXV1dgQMHAIcCAQYBAQdk",
        "ZWZhdWx0/////wMHAAMCBgMCAIQHFAZhd29yZCAHAQdkZWZhdWx0Awh0YXNrSXRlbUQHOwLDqQA=",
    ));
    let doc = with_update(&new_doc(), &before, &ROOMY).expect("a body");
    let with = with_update(&doc, &update, &ROOMY).expect("taken");
    let refused = to_body(&with);
    assert!(refused.is_err(), "no body as it is");
    let state = state_of(&with);
    assert!(repair(&with).is_err(), "no repair mends it");
    assert_eq!(state_of(&with), state, "the repair deleted nothing");
    assert_eq!(to_body(&with), refused, "the same refusal");
    // What the server calls refuses the update, and the document stays.
    let state = state_of(&doc);
    assert!(matches!(
        take_update(&doc, &update, &ROOMY),
        Err(TakeError::Body(_))
    ));
    assert_eq!(state_of(&doc), state, "the document is as it was");
}
