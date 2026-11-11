//! What the server calls: an update taken whole or not at all, a stored state opened,
//! and the limits each is held to.

mod support;

use fvoci_collab::testing::{copy_of, to_body, with_update};
use fvoci_collab::{
    FIELD, TakeError, UpdateError, UpdateLimits, from_body, from_state, new_doc, state_of,
    take_update,
};
use fvoci_model::BodyError;
use serde_json::{Value, json};
use support::{ROOMY, body_of, json_of};
use yrs::{
    Array, ArrayRef, Doc, ReadTxn, Transact, TransactionMut, XmlElementPrelim, XmlFragment,
    XmlFragmentRef, XmlTextPrelim,
};

fn paragraphs(texts: &[&str]) -> Value {
    let content: Vec<Value> = texts
        .iter()
        .map(|text| json!({"type": "paragraph", "content": [{"type": "text", "text": text}]}))
        .collect();
    json!({"type": "doc", "content": content})
}

/// What a client that holds `doc` sends after it writes into the root with `write`.
fn edit(doc: &Doc, write: impl FnOnce(&mut TransactionMut<'_>, &XmlFragmentRef)) -> Vec<u8> {
    let client = copy_of(doc).expect("the document can be read again");
    let before = client.transact().state_vector();
    {
        let root = client.get_or_insert_xml_fragment(FIELD);
        let mut txn = client.transact_mut();
        write(&mut txn, &root);
    }
    client.transact().encode_diff_v1(&before)
}

/// The update of a paragraph appended with `text`.
fn appended(doc: &Doc, text: &str) -> Vec<u8> {
    edit(doc, |txn, root| {
        let paragraph = root.push_back(txn, XmlElementPrelim::empty("paragraph"));
        paragraph.push_back(txn, XmlTextPrelim::new(text));
    })
}

/// The update of a list item appended at the root, which the document may not hold.
fn stray_entry(doc: &Doc) -> Vec<u8> {
    edit(doc, |txn, root| {
        let entry = root.push_back(txn, XmlElementPrelim::empty("listItem"));
        let paragraph = entry.push_back(txn, XmlElementPrelim::empty("paragraph"));
        paragraph.push_back(txn, XmlTextPrelim::new("stray"));
    })
}

#[test]
fn an_update_that_leaves_a_body_is_taken_as_it_is() {
    let doc = from_body(&body_of(&paragraphs(&["first"])));
    let before = state_of(&doc);
    let taken = take_update(&doc, &appended(&doc, "second"), &ROOMY).expect("taken");
    assert_eq!(
        json_of(taken.repair.body.as_ref()),
        paragraphs(&["first", "second"])
    );
    assert_eq!((taken.repair.deleted, &taken.repair.update), (0, &None));
    assert_eq!(to_body(&taken.doc), Ok(taken.repair.body.clone()));
    assert_eq!(state_of(&doc), before, "the document given is as it was");
}

#[test]
fn an_update_that_leaves_no_body_is_taken_repaired_with_the_deletions() {
    let doc = from_body(&body_of(&paragraphs(&["kept"])));
    let before = state_of(&doc);
    let update = stray_entry(&doc);
    let taken = take_update(&doc, &update, &ROOMY).expect("taken, repaired");
    assert_eq!(json_of(taken.repair.body.as_ref()), paragraphs(&["kept"]));
    assert_eq!(taken.repair.deleted, 1);
    assert_eq!(to_body(&taken.doc), Ok(taken.repair.body.clone()));
    assert_eq!(state_of(&doc), before, "the document given is as it was");
    // An editor that sent the update, or took it, holds the same body once it is
    // given the deletions.
    let editor = with_update(&doc, &update, &ROOMY).expect("the update alone");
    let deletions = taken.repair.update.expect("the deletions");
    let editor = with_update(&editor, &deletions, &ROOMY).expect("the deletions");
    assert_eq!(to_body(&editor), Ok(taken.repair.body));
}

#[test]
fn a_refused_update_leaves_the_document_as_it_was() {
    let doc = from_body(&body_of(&paragraphs(&["first"])));
    let before = state_of(&doc);
    // No update at all, and one past the limit of size.
    let refused = take_update(&doc, &[1, 2, 3], &ROOMY);
    assert!(
        matches!(refused, Err(TakeError::Update(UpdateError::Unreadable(_)))),
        "{refused:?}"
    );
    let update = appended(&doc, "second");
    let tight = UpdateLimits {
        update_bytes: update.len() - 1,
        ..ROOMY
    };
    assert!(matches!(
        take_update(&doc, &update, &tight),
        Err(TakeError::Update(UpdateError::TooLarge { .. }))
    ));
    // A value put straight into the root, with a list that has no entries: no repair
    // mends a root that holds a value, as there is no node to delete in its place.
    let unmendable = edit(&doc, |txn, root| {
        let values: &ArrayRef = root.as_ref();
        values.push_back(txn, 7);
        root.push_back(txn, XmlElementPrelim::empty("bulletList"));
    });
    assert_eq!(
        take_update(&doc, &unmendable, &ROOMY).map(|_| ()),
        Err(TakeError::Body(BodyError::InvalidNode {
            path: String::new(),
            problem: "the root holds something that is neither a node nor a text",
        }))
    );
    assert_eq!(state_of(&doc), before, "the document given is as it was");
}

#[test]
fn a_body_past_its_limit_is_refused() {
    let doc = from_body(&body_of(&paragraphs(&["first"])));
    let update = appended(&doc, "second");
    let size = serde_json::to_vec(&paragraphs(&["first", "second"]))
        .expect("JSON")
        .len();
    let exact = UpdateLimits {
        body_bytes: size,
        ..ROOMY
    };
    assert!(take_update(&doc, &update, &exact).is_ok(), "at the limit");
    let tight = UpdateLimits {
        body_bytes: size - 1,
        ..ROOMY
    };
    assert_eq!(
        take_update(&doc, &update, &tight).map(|_| ()),
        Err(TakeError::BodyTooLarge {
            size,
            limit: size - 1
        })
    );
    // A repaired body is held to it as well.
    let stray = stray_entry(&doc);
    let kept = serde_json::to_vec(&paragraphs(&["first"]))
        .expect("JSON")
        .len();
    let tight = UpdateLimits {
        body_bytes: kept - 1,
        ..ROOMY
    };
    assert_eq!(
        take_update(&doc, &stray, &tight).map(|_| ()),
        Err(TakeError::BodyTooLarge {
            size: kept,
            limit: kept - 1
        })
    );
}

#[test]
fn a_stored_state_is_opened_as_an_update_to_an_empty_document_is_taken() {
    let doc = from_body(&body_of(&paragraphs(&["first", "second"])));
    let state = state_of(&doc);
    // A state may be larger than an update.
    let limits = UpdateLimits {
        update_bytes: 1,
        ..ROOMY
    };
    let opened = from_state(&state, &limits).expect("opened");
    assert_eq!(state_of(&opened.doc), state);
    assert_eq!(
        json_of(opened.repair.body.as_ref()),
        paragraphs(&["first", "second"])
    );
    let small = UpdateLimits {
        state_bytes: state.len() - 1,
        ..ROOMY
    };
    assert!(matches!(
        from_state(&state, &small),
        Err(TakeError::Update(UpdateError::TooLarge { .. }))
    ));
    // The state of an empty document is no body.
    let empty = from_state(&state_of(&new_doc()), &ROOMY).expect("opened");
    assert_eq!(empty.repair.body, None);
    assert!(empty.doc.transact().get_xml_fragment(FIELD).is_some());
}

/// A repaired document is held to the limit of the state as well: the deletions of a
/// repair add to it.
#[test]
fn a_repaired_document_past_the_limit_of_its_state_is_refused() {
    let doc = from_body(&body_of(&paragraphs(&["first"])));
    let stray = stray_entry(&doc);
    let with = with_update(&doc, &stray, &ROOMY).expect("taken as it is");
    let taken = take_update(&doc, &stray, &ROOMY).expect("taken, repaired");
    let (unrepaired, repaired) = (state_of(&with).len(), state_of(&taken.doc).len());
    assert!(repaired > unrepaired, "{repaired} > {unrepaired}");
    let tight = UpdateLimits {
        state_bytes: unrepaired,
        ..ROOMY
    };
    assert!(matches!(
        take_update(&doc, &stray, &tight),
        Err(TakeError::Update(UpdateError::DocumentTooLarge { .. }))
    ));
}
