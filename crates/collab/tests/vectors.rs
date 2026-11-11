//! The vectors of `contracts/ydoc-vectors.json`, which the web app writes with the
//! editor's own binding: this crate reads the same JSON from every Y document, makes Y
//! documents an editor reads the same JSON from, and repairs as an editor repairs.

mod support;

use fvoci_collab::{from_body, repair, state_of, to_body};
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
/// binding's document of that body, also after it has travelled as an update.
#[test]
fn a_document_made_here_projects_to_what_an_editor_reads() {
    let all = body_vectors();
    assert!(all.len() > 50, "the accepted bodies and the vectors' own");
    for one in all {
        let name = &one.vector.name;
        let made = from_body(&body_of(&one.body));
        let direct = to_body(&made).unwrap_or_else(|error| panic!("{name}: {error}"));
        assert_eq!(json_of(direct.as_ref()), one.vector.json, "{name}");
        let travelled =
            to_body(&doc_of(&state_of(&made))).unwrap_or_else(|error| panic!("{name}: {error}"));
        assert_eq!(
            json_of(travelled.as_ref()),
            one.vector.json,
            "{name}, as an update"
        );
    }
}

/// A body is always made into the same Y document, byte for byte.
#[test]
fn a_document_is_made_the_same_every_time() {
    for one in body_vectors() {
        let body = body_of(&one.body);
        assert_eq!(
            state_of(&from_body(&body)),
            state_of(&from_body(&body)),
            "{}",
            one.vector.name
        );
    }
}

/// What the editors keep though it is no body is refused here, and a repair deletes
/// what the allowlist refuses in it and leaves a body.
#[test]
fn what_only_the_servers_rules_refuse_is_refused_and_repaired() {
    for vector in vectors("refused") {
        let name = &vector.name;
        let doc = doc_of(&vector.update);
        assert!(to_body(&doc).is_err(), "{name}: refused as it is");
        let repaired = repair(&doc).unwrap_or_else(|error| panic!("{name}: {error}"));
        assert!(repaired.deleted > 0, "{name}: something was deleted");
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
        let other = fvoci_collab::with_update(&other, &deletions, &support::ROOMY)
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
            let update = to_base64(&state_of(&from_body(&body_of(&one.body))));
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
