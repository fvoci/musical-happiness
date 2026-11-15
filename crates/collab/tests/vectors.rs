//! The vectors of `contracts/ydoc-vectors.json`, which the web app writes with the
//! editor's own binding: this crate reads the same JSON from every Y document, makes Y
//! documents an editor reads the same JSON from, and repairs as an editor repairs.

mod support;

use fvoci_collab::testing::{repair, to_body};
use fvoci_collab::{from_body, from_body_by, state_of};
use serde_json::{Value, json};
use support::{body_of, body_vectors, doc_of, json_of, to_base64, vectors};

/// Every Y document the binding makes of a body projects to the JSON an editor reads
/// from it, and so does shared content no editor writes and every editor keeps.
#[test]
fn a_document_of_the_binding_projects_to_what_an_editor_reads() {
    for section in ["documents", "bodies", "readings"] {
        for vector in vectors(section) {
            let body = to_body(&doc_of(&vector.update))
                .unwrap_or_else(|error| panic!("{}: {error}", vector.name));
            assert_eq!(json_of(body.as_ref()), vector.json, "{}", vector.name);
        }
    }
}

/// The Y document made of a body projects to the JSON an editor reads from the
/// binding's document of that body, also after it has travelled as an update, and is
/// no larger than the body.
#[test]
fn a_document_made_here_projects_to_what_an_editor_reads() {
    let all = body_vectors();
    assert!(all.len() > 50, "the accepted bodies and the vectors' own");
    for one in all {
        let name = &one.vector.name;
        let made = from_body(&body_of(&one.body));
        let direct = to_body(&made).unwrap_or_else(|error| panic!("{name}: {error}"));
        assert_eq!(json_of(direct.as_ref()), one.vector.json, "{name}");
        let size = |json: &Value| serde_json::to_vec(json).expect("JSON").len();
        assert!(
            size(&one.vector.json) <= size(&one.body),
            "{name}: no larger than the body it was made of"
        );
        let travelled =
            to_body(&doc_of(&state_of(&made))).unwrap_or_else(|error| panic!("{name}: {error}"));
        assert_eq!(
            json_of(travelled.as_ref()),
            one.vector.json,
            "{name}, as an update"
        );
    }
}

/// The same writer makes the same Y document of a body, byte for byte; left to
/// itself, every document has a writer of its own.
#[test]
fn a_document_is_made_the_same_by_the_same_writer() {
    for one in body_vectors() {
        let body = body_of(&one.body);
        assert_eq!(
            state_of(&from_body_by(&body, 1)),
            state_of(&from_body_by(&body, 1)),
            "{}",
            one.vector.name
        );
        assert_ne!(
            state_of(&from_body(&body)),
            state_of(&from_body(&body)),
            "{}: a writer of its own each time",
            one.vector.name
        );
    }
}

/// What the editors keep though it is no body is refused here, and a repair deletes
/// what the allowlist refuses in it and nothing else. No editor says what is left; this
/// table does, as how many deletions and the body.
#[test]
fn what_only_the_servers_rules_refuse_is_refused_and_repaired() {
    let a =
        r#"{"content":[{"content":[{"text":"a","type":"text"}],"type":"paragraph"}],"type":"doc"}"#;
    let two_empty = r#"{"content":[{"type":"paragraph"},{"type":"paragraph"}],"type":"doc"}"#;
    let left: [(&str, usize, &str); 18] = [
        ("a heading of level 7", 1, a),
        ("a heading whose level is a text", 1, "null"),
        ("a heading whose level is null", 1, "null"),
        ("an image without a file", 1, a),
        ("an image whose file is empty", 1, "null"),
        // The mentions go, the paragraphs they stood in stay.
        (
            "a mention of nobody, and of somebody who is no identifier",
            2,
            two_empty,
        ),
        // As the mentions: the references go, their paragraphs stay.
        (
            "a reference to nothing, and to something that is no identifier",
            2,
            two_empty,
        ),
        ("formulas without a source and with an empty one", 3, a),
        // The text goes, and a document of one empty paragraph is no body.
        ("a link without a target", 1, "null"),
        ("a link whose value is true", 1, "null"),
        ("a link whose value is a text", 1, "null"),
        ("a link that runs a script", 1, a),
        ("an ordered list that starts below zero", 1, "null"),
        // The item, and with it the list it leaves without one: one deletion.
        ("a task item whose checked is a text", 1, "null"),
        ("a code block whose language has a space", 1, "null"),
        ("a text with the character U+0000", 1, "null"),
        (
            "a mark given twice, once under a name with a hash",
            1,
            "null",
        ),
        ("two links on one text, under names with a hash", 1, "null"),
    ];
    let refused = vectors("refused");
    assert_eq!(
        refused
            .iter()
            .map(|one| one.name.as_str())
            .collect::<Vec<_>>(),
        left.iter().map(|(name, _, _)| *name).collect::<Vec<_>>(),
        "the table has every refused vector"
    );
    for (vector, (name, deletions, body)) in refused.iter().zip(left) {
        let doc = doc_of(&vector.update);
        assert!(to_body(&doc).is_err(), "{name}: refused as it is");
        let repaired = repair(&doc).unwrap_or_else(|error| panic!("{name}: {error}"));
        assert_eq!(repaired.deleted, deletions, "{name}: deletions");
        assert_eq!(json_of(repaired.body.as_ref()).to_string(), body, "{name}");
        let now = to_body(&doc).unwrap_or_else(|error| panic!("{name}: {error}"));
        assert_eq!(now, repaired.body, "{name}: the document is the body given");
    }
}

/// Shared content that does not fit is refused as it is, and repaired to what is left
/// when an editor has deleted what fails. A second repair finds nothing to do, and an
/// editor that is handed the deletions holds the same.
#[test]
fn a_repair_leaves_what_an_editor_leaves() {
    for vector in vectors("repairs") {
        let name = &vector.name;
        let doc = doc_of(&vector.update);
        assert!(to_body(&doc).is_err(), "{name}: refused as it is");

        let repaired = repair(&doc).unwrap_or_else(|error| panic!("{name}: {error}"));
        assert_eq!(json_of(repaired.body.as_ref()), vector.json, "{name}");
        assert!(repaired.deleted > 0, "{name}: something was deleted");
        let now = to_body(&doc).unwrap_or_else(|error| panic!("{name}: {error}"));
        assert_eq!(now, repaired.body, "{name}: the document is the body given");

        let again = repair(&doc).unwrap_or_else(|error| panic!("{name}: {error}"));
        assert_eq!(
            (again.deleted, &again.update),
            (0, &None),
            "{name}: once is enough"
        );
        assert_eq!(again.body, repaired.body, "{name}: and changes nothing");

        // Another holder of the same document, handed the deletions alone.
        let other = doc_of(&vector.update);
        let deletions = repaired.update.expect("the deletions as an update");
        let other = fvoci_collab::testing::with_update(&other, &deletions, &support::ROOMY)
            .unwrap_or_else(|error| panic!("{name}: {error}"));
        let theirs = to_body(&other).unwrap_or_else(|error| panic!("{name}: {error}"));
        assert_eq!(
            json_of(theirs.as_ref()),
            vector.json,
            "{name}: after the deletions"
        );
    }
}

/// What this crate makes, for the web app's test to read with the editor's binding
/// (`apps/web/tests/editor/ydoc-vectors.test.ts`): the Y document of every accepted
/// body, and every repair's document after the repair.
fn made_here() -> Value {
    let documents: Vec<Value> = body_vectors()
        .iter()
        .map(|one| {
            let update = to_base64(&state_of(&from_body_by(&body_of(&one.body), 1)));
            json!({"name": one.vector.name, "update": update})
        })
        .collect();
    let repairs: Vec<Value> = vectors("repairs")
        .iter()
        .map(|vector| {
            let doc = doc_of(&vector.update);
            repair(&doc).unwrap_or_else(|error| panic!("{}: {error}", vector.name));
            json!({"name": vector.name, "update": to_base64(&state_of(&doc))})
        })
        .collect();
    json!({
        "description": "Y documents made by crates/collab, written by its test \
            (FVOCI_WRITE_VECTORS=1 cargo test -p fvoci-collab --test vectors). documents: \
            from_body of every body contracts/ydoc-vectors.json has under documents and \
            bodies. repairs: every repair of that file after repair. The web app's test reads each \
            with the editor's binding and expects the JSON of the vector of the same name, and \
            that the editor deletes nothing more.",
        "documents": documents,
        "repairs": repairs,
    })
}

/// The file the web app's test reads is what this crate makes now. Run with
/// `FVOCI_WRITE_VECTORS=1` to write it.
#[test]
fn the_file_for_the_web_app_is_up_to_date() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/made-here.json");
    let made = format!(
        "{}\n",
        serde_json::to_string_pretty(&made_here()).expect("JSON")
    );
    if std::env::var_os("FVOCI_WRITE_VECTORS").is_some() {
        std::fs::write(&path, &made).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    }
    let committed = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(
        committed == made,
        "{} is stale: run FVOCI_WRITE_VECTORS=1 cargo test -p fvoci-collab --test vectors",
        path.display()
    );
}

/// One editor's session, as the vectors recorded it from the binding: the document the
/// server opens, then every update the editor sends. Each is taken in its turn, taking
/// one twice changes nothing, one that comes before the one it builds on is refused,
/// and the end is the body an editor reads there.
#[test]
fn the_updates_of_an_editing_session_are_taken_in_order() {
    let file = support::read_json(&support::contract("ydoc-vectors.json"));
    let session = &file["session"];
    let bytes = |text: &Value| support::from_base64(text.as_str().expect("base64"));
    let updates: Vec<Vec<u8>> = session["updates"]
        .as_array()
        .expect("the updates of the session")
        .iter()
        .map(bytes)
        .collect();
    assert!(updates.len() >= 5, "an update for every edit");
    let opened = doc_of(&bytes(&session["start"]));

    let ahead = updates.last().expect("a last update");
    assert_eq!(
        fvoci_collab::testing::with_update(&opened, ahead, &support::ROOMY).map(|_| ()),
        Err(fvoci_collab::UpdateError::MissingChanges),
        "the last update first"
    );

    let mut doc = opened;
    for (index, update) in updates.iter().enumerate() {
        doc = fvoci_collab::testing::with_update(&doc, update, &support::ROOMY)
            .unwrap_or_else(|error| panic!("update {index}: {error}"));
        assert!(
            to_body(&doc).is_ok(),
            "update {index}: a body after every edit"
        );
        let again = fvoci_collab::testing::with_update(&doc, update, &support::ROOMY)
            .unwrap_or_else(|error| panic!("update {index}, again: {error}"));
        assert_eq!(
            state_of(&again),
            state_of(&doc),
            "update {index}: twice is once"
        );
    }
    let end = to_body(&doc).expect("a body at the end");
    assert_eq!(json_of(end.as_ref()), session["json"]);
}
