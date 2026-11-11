//! Taking a client's update: what is refused before anything is projected, and that
//! the document an update is tried on stays as it was.

mod support;

use fvoci_collab::{
    FIELD, UpdateError, UpdateLimits, copy_of, from_body, new_doc, state_of, to_body, with_update,
};
use serde_json::json;
use support::{ROOMY, body_of, json_of};
use yrs::updates::decoder::Decode;
use yrs::{
    Doc, GetString, ReadTxn, StateVector, Text, Transact, Update, XmlElementPrelim, XmlFragment,
    XmlTextPrelim,
};

/// A Y document of one paragraph, as the server makes it.
fn opened(text: &str) -> Doc {
    from_body(&body_of(&json!({"type": "doc", "content": [
        {"type": "paragraph", "content": [{"type": "text", "text": text}]},
    ]})))
}

/// What a client that holds `doc` sends after appending a paragraph: the update of
/// that change alone.
fn appended(doc: &Doc, text: &str) -> Vec<u8> {
    let client = copy_of(doc);
    let before = client.transact().state_vector();
    {
        let root = client.get_or_insert_xml_fragment(FIELD);
        let mut txn = client.transact_mut();
        let paragraph = root.push_back(&mut txn, XmlElementPrelim::empty("paragraph"));
        paragraph.push_back(&mut txn, XmlTextPrelim::new(text));
    }
    client.transact().encode_diff_v1(&before)
}

fn texts(doc: &Doc) -> Vec<String> {
    let body = to_body(doc).expect("a body");
    json_of(body.as_ref())["content"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|block| {
            block["content"][0]["text"]
                .as_str()
                .unwrap_or_default()
                .to_owned()
        })
        .collect()
}

#[test]
fn an_update_is_applied_to_a_copy_and_the_document_stays() {
    let doc = opened("first");
    let before = state_of(&doc);
    let update = appended(&doc, "second");
    let with = with_update(&doc, &update, &ROOMY).expect("the update is taken");
    assert_eq!(texts(&with), ["first", "second"]);
    assert_eq!(state_of(&doc), before, "the document itself is as it was");
    assert_eq!(texts(&doc), ["first"]);
    // The same update again changes nothing.
    let again = with_update(&with, &update, &ROOMY).expect("the update is taken again");
    assert_eq!(state_of(&again), state_of(&with));
}

#[test]
fn a_copy_shares_nothing_with_its_document() {
    let doc = opened("first");
    let copy = copy_of(&doc);
    assert_eq!(state_of(&copy), state_of(&doc));
    {
        let root = copy.get_or_insert_xml_fragment(FIELD);
        let mut txn = copy.transact_mut();
        root.remove_range(&mut txn, 0, 1);
    }
    assert_eq!(to_body(&copy), Ok(None));
    assert_eq!(texts(&doc), ["first"]);
}

#[test]
fn an_update_over_the_limit_or_that_is_none_is_refused() {
    let doc = opened("first");
    let update = appended(&doc, "second");
    let tight = UpdateLimits {
        update_bytes: update.len() - 1,
        ..ROOMY
    };
    assert_eq!(
        with_update(&doc, &update, &tight).map(|_| ()),
        Err(UpdateError::TooLarge {
            size: update.len(),
            limit: update.len() - 1,
        })
    );
    let exact = UpdateLimits {
        update_bytes: update.len(),
        ..ROOMY
    };
    assert!(with_update(&doc, &update, &exact).is_ok(), "at the limit");

    for bytes in [&b""[..], &b"\xff\xff\xff"[..], &update[..update.len() / 2]] {
        assert!(
            matches!(
                with_update(&doc, bytes, &ROOMY),
                Err(UpdateError::Unreadable(_))
            ),
            "{bytes:?}"
        );
    }
}

/// A client's second change without its first: the second waits in the document
/// instead of taking effect, and is refused.
#[test]
fn an_update_that_builds_on_changes_the_document_lacks_is_refused() {
    let doc = opened("first");
    let client = copy_of(&doc);
    let root = client.get_or_insert_xml_fragment(FIELD);
    let start = client.transact().state_vector();
    {
        let mut txn = client.transact_mut();
        let paragraph = root.push_back(&mut txn, XmlElementPrelim::empty("paragraph"));
        paragraph.push_back(&mut txn, XmlTextPrelim::new("second"));
    }
    let middle = client.transact().state_vector();
    {
        let mut txn = client.transact_mut();
        let paragraph = root.push_back(&mut txn, XmlElementPrelim::empty("paragraph"));
        paragraph.push_back(&mut txn, XmlTextPrelim::new("third"));
    }
    let first_change = client.transact().encode_diff_v1(&start);
    let second_change = client.transact().encode_diff_v1(&middle);
    assert_ne!(first_change, second_change);

    assert_eq!(
        with_update(&doc, &second_change, &ROOMY).map(|_| ()),
        Err(UpdateError::MissingChanges)
    );
    let with = with_update(&doc, &first_change, &ROOMY).expect("in order");
    assert_eq!(texts(&with), ["first", "second", "third"]);
}

/// Changes of a client nobody has heard of that begin at clock 100: the library would
/// take them and leave the hundred before them open.
#[test]
fn an_update_that_begins_after_a_hole_is_refused() {
    let doc = opened("first");
    let text_under_root = |clock: u8| {
        let mut bytes = vec![1, 1, 9, clock, 7, 1, 7];
        bytes.extend_from_slice(b"default");
        bytes.extend_from_slice(&[3, 9]);
        bytes.extend_from_slice(b"paragraph");
        bytes.push(0);
        bytes
    };
    assert_eq!(
        with_update(&doc, &text_under_root(100), &ROOMY).map(|_| ()),
        Err(UpdateError::MissingChanges)
    );
    let with = with_update(&doc, &text_under_root(0), &ROOMY).expect("from the beginning");
    assert_eq!(texts(&with).len(), 2, "the paragraph the update adds");
}

/// An update may write only into the fragment that holds the body.
#[test]
fn an_update_that_writes_to_another_root_is_refused() {
    let doc = opened("first");
    let other_root = {
        let client = Doc::new();
        let text = client.get_or_insert_text("notes");
        text.insert(&mut client.transact_mut(), 0, "aside");
        assert_eq!(text.get_string(&client.transact()), "aside");
        client
            .transact()
            .encode_state_as_update_v1(&StateVector::default())
    };
    assert_eq!(
        with_update(&doc, &other_root, &ROOMY).map(|_| ()),
        Err(UpdateError::UnexpectedRoot("notes".to_owned()))
    );
}

/// A Y document keeps a trace of what was deleted, so it is the document's size that
/// is limited, not only the body's.
#[test]
fn an_update_that_leaves_the_document_too_large_is_refused() {
    let doc = opened("first");
    let size = state_of(&doc).len();
    let update = appended(&doc, &"x".repeat(1000));
    let limits = |state_bytes| UpdateLimits {
        state_bytes,
        ..ROOMY
    };
    let grown = with_update(&doc, &update, &ROOMY).expect("taken with room");
    let larger = state_of(&grown).len();
    assert!(
        larger > size + 1000,
        "the document grew by the text and more"
    );
    assert!(
        with_update(&doc, &update, &limits(larger)).is_ok(),
        "at the limit"
    );
    assert_eq!(
        with_update(&doc, &update, &limits(larger - 1)).map(|_| ()),
        Err(UpdateError::DocumentTooLarge {
            size: larger,
            limit: larger - 1,
        })
    );
}

/// The state of a document is an update that brings an empty one to the same state,
/// here and in the Y library's own reading.
#[test]
fn the_state_of_a_document_is_an_update() {
    let doc = opened("first");
    let state = state_of(&doc);
    assert!(Update::decode_v1(&state).is_ok(), "the library reads it");
    let from_state = with_update(&new_doc(), &state, &ROOMY).expect("taken");
    assert_eq!(state_of(&from_state), state);
}

/// A Y document can be handed from one task to another and used from several: the room
/// that will hold one does not have to stay on a thread.
#[test]
fn a_document_may_cross_threads() {
    fn assert_send_and_sync<T: Send + Sync + 'static>() {}
    assert_send_and_sync::<Doc>();
    let doc = opened("first");
    let moved = std::thread::spawn(move || texts(&doc))
        .join()
        .expect("the thread");
    assert_eq!(moved, ["first"]);
}

/// What the room that holds a document pays for each update, measured on the largest
/// body (1 MiB of text) and on the one with the most nodes, each with room left for a
/// paragraph more, and printed: run with
/// `--release -- --nocapture` to read it. Nothing is asserted about the times.
#[test]
fn the_cost_of_taking_an_update_into_the_largest_bodies() {
    use fvoci_model::{BODY_MAX_BYTES, BODY_MAX_NODES};
    use std::time::Instant;

    let frame =
        r#"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":""}]}]}"#;
    let largest = json!({"type": "doc", "content": [
        {"type": "paragraph", "content": [{"type": "text", "text": "a".repeat(BODY_MAX_BYTES - 2 * frame.len())}]},
    ]});
    let paragraphs = vec![json!({"type": "paragraph"}); BODY_MAX_NODES - 3];
    let most = json!({"type": "doc", "content": paragraphs});

    for (name, document) in [("1 MiB of text", largest), ("20,000 nodes", most)] {
        let body = body_of(&document);
        let timed = Instant::now();
        let doc = from_body(&body);
        let made = timed.elapsed();
        let timed = Instant::now();
        let state = state_of(&doc);
        let encoded = timed.elapsed();
        let timed = Instant::now();
        let copy = copy_of(&doc);
        let copied = timed.elapsed();
        let timed = Instant::now();
        let projected = to_body(&copy).expect("a body");
        let read = timed.elapsed();
        assert!(projected.is_some(), "{name}: a body");
        let update = appended(&doc, "typed");
        let timed = Instant::now();
        let with = with_update(&doc, &update, &ROOMY).expect("taken");
        let taken = timed.elapsed();
        assert_eq!(
            texts(&with).last().map(String::as_str),
            Some("typed"),
            "{name}"
        );
        println!(
            "{name}: state {} bytes; from_body {made:?}, state_of {encoded:?}, copy_of {copied:?}, \
             to_body {read:?}, with_update of {} bytes {taken:?}",
            state.len(),
            update.len()
        );
    }
}
