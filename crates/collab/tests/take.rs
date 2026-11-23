//! What the server calls: an update taken whole or not at all, a stored state opened,
//! and the limits each is held to.

mod support;

use fvoci_collab::testing::{copy_of, to_body, with_update};
use fvoci_collab::{
    BodyDoc, BodyTakeError, FIELD, READING_STEPS, TakeError, UpdateError, UpdateLimits, from_body,
    from_state, new_doc, reading_steps, state_counts, state_of, take_update,
};
use fvoci_model::BodyError;
use serde_json::{Value, json};
use support::random::{self, Random};
use support::wire::{Change, Content, Parent, Update};
use support::{ROOMY, body_of, json_of};
use yrs::updates::decoder::Decode as _;
use yrs::{
    Array, ArrayPrelim, ArrayRef, Doc, MapPrelim, ReadTxn, Transact, TransactionMut, Xml,
    XmlElementPrelim, XmlFragment, XmlFragmentRef, XmlOut, XmlTextPrelim,
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
        Err(TakeError::Update(UpdateError::DocumentTooLarge { .. }))
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

struct RollbackPoint<'a> {
    base: &'a [u8],
    since: &'a [Vec<u8>],
    state: &'a [u8],
    changes: u64,
}

/// Applies an arbitrary random or crafted rejected update in place, then rebuilds from
/// `base` + `since` when `Dirty`, asserting the restored document state is 100%
/// byte-for-byte identical to `point.state`.
fn assert_rollback_byte_equal(
    doc: BodyDoc,
    point: &RollbackPoint<'_>,
    rejected: &[u8],
    limits: &UpdateLimits,
    context: &str,
) -> BodyDoc {
    match doc.take_in_place(point.state, point.changes, rejected, limits) {
        Ok(unexpected) => panic!("{context}: expected refusal, got Ok ({unexpected:?})"),
        Err(BodyTakeError::Untouched { doc, .. }) => {
            assert_eq!(
                doc.state(),
                point.state,
                "{context}: untouched doc state differs"
            );
            doc
        }
        Err(BodyTakeError::Dirty(_)) => {
            let rebuilt = BodyDoc::rebuild(point.base, point.since).expect("rebuild succeeds");
            assert_eq!(
                rebuilt.state(),
                point.state,
                "{context}: rebuilt state differs byte-for-byte from prior state"
            );
            rebuilt
        }
    }
}

/// Across random documents and sequences of accepted edits (including repairs and table
/// mends), every rejected update — whether refused before mutation (`Untouched`) or
/// after mutation (`Dirty`, rebuilt from `base` + `since`) — leaves the document state
/// byte-for-byte identical to before the rejected update.
#[test]
fn random_refused_in_place_updates_rebuild_to_exact_prior_bytes() {
    for seed in 1..=40_u64 {
        let mut rng = Random::new(seed);
        let initial = body_of(&random::document(&mut rng));
        let mut doc = BodyDoc::from_body(Some(&initial));
        let base = doc.state();
        let mut state = base.clone();
        let mut doc_changes = state_counts(&state).expect("counts").changes;
        let mut since: Vec<Vec<u8>> = Vec::new();
        let mut saw_dirty = false;

        for step in 0..5_usize {
            let client = 100 + u64::try_from(step).expect("fits");
            let editor = fvoci_collab::testing::Editor::join(client, &state);
            let update = match rng.below(4) {
                0 => editor.append_paragraph(&format!("seed-{seed}-step-{step}")),
                1 => editor.append_element("nothing-a-body-has"),
                2 => editor.delete_last(),
                _ => editor.append_mention("01930000-0000-7000-8000-00000000c001"),
            };
            // Reference copy-based take must agree with in-place take.
            let ref_doc = from_state(&state, &ROOMY).expect("ref").doc;
            let expected = take_update(&ref_doc, &update, &ROOMY).expect("ref taken");

            let changed = doc
                .take_in_place(&state, doc_changes, &update, &ROOMY)
                .expect("in-place taken");
            assert_eq!(
                changed.state, expected.state,
                "seed {seed} step {step}: state"
            );
            assert_eq!(
                changed.repair.body, expected.repair.body,
                "seed {seed} step {step}: body"
            );
            assert_eq!(
                changed.repair.update, expected.repair.update,
                "seed {seed} step {step}: repair"
            );
            if changed.state != state || changed.repair.update.is_some() {
                since.push(update);
                if let Some(repair) = changed.repair.update {
                    since.push(repair);
                }
                state = changed.state;
                doc_changes = changed.changes;
            }
            doc = changed.doc;

            let point = RollbackPoint {
                base: &base,
                since: &since,
                state: &state,
                changes: doc_changes,
            };
            let (rebuilt_doc, hit_dirty) =
                exercise_step_refusals(doc, &mut rng, seed, step, &point);
            doc = rebuilt_doc;
            saw_dirty |= hit_dirty;
        }
        assert!(saw_dirty, "seed {seed}: exercised Dirty rollback path");
    }
}

fn exercise_step_refusals(
    mut doc: BodyDoc,
    rng: &mut Random,
    seed: u64,
    step: usize,
    point: &RollbackPoint<'_>,
) -> (BodyDoc, bool) {
    // 1. Pre-apply refusal (Untouched): random unreadable bytes and gap update.
    let junk_len = 1 + rng.below(32);
    let junk = rng.bytes(junk_len);
    let is_refused = take_update(
        &from_state(point.state, &ROOMY).expect("ref").doc,
        &junk,
        &ROOMY,
    )
    .is_err();
    if is_refused {
        doc = assert_rollback_byte_equal(
            doc,
            point,
            &junk,
            &ROOMY,
            &format!("seed {seed} step {step} random bytes"),
        );
    }
    let gap = Update::of(
        9_000 + seed,
        5,
        vec![Change::under(Parent::body(), Content::element("paragraph"))],
    )
    .bytes();
    doc = assert_rollback_byte_equal(
        doc,
        point,
        &gap,
        &ROOMY,
        &format!("seed {seed} step {step} missing changes"),
    );

    exercise_post_apply_refusals(doc, seed, step, point)
}

fn exercise_post_apply_refusals(
    mut doc: BodyDoc,
    seed: u64,
    step: usize,
    point: &RollbackPoint<'_>,
) -> (BodyDoc, bool) {
    // 2. Post-apply refusals (Dirty -> rebuild from base + since):
    //    unexpected root, state_bytes too small, body_bytes too small, and
    //    unrepairable root value.
    let bad_root = Update::of(
        20_000 + seed * 10 + u64::try_from(step).expect("fits"),
        0,
        vec![Change::under(
            Parent::Root(b"other".to_vec()),
            Content::element("paragraph"),
        )],
    )
    .bytes();
    let hit_dirty = match doc.take_in_place(point.state, point.changes, &bad_root, &ROOMY) {
        Err(BodyTakeError::Dirty(TakeError::Update(UpdateError::UnexpectedRoot(_)))) => {
            doc = BodyDoc::rebuild(point.base, point.since).expect("rebuilt");
            assert_eq!(doc.state(), point.state, "seed {seed} step {step} bad_root");
            true
        }
        other => {
            panic!("seed {seed} step {step}: expected Dirty(UnexpectedRoot), got {other:?}")
        }
    };

    let probe = fvoci_collab::testing::Editor::join(30_000 + seed, point.state);
    let grown = probe.append_paragraph("too large for tight limit");
    let tight_state = UpdateLimits {
        state_bytes: point.state.len(),
        ..ROOMY
    };
    doc = assert_rollback_byte_equal(
        doc,
        point,
        &grown,
        &tight_state,
        &format!("seed {seed} step {step} tight state_bytes"),
    );

    let tight_body = UpdateLimits {
        body_bytes: 1,
        ..ROOMY
    };
    doc = assert_rollback_byte_equal(
        doc,
        point,
        &grown,
        &tight_body,
        &format!("seed {seed} step {step} tight body_bytes"),
    );

    let root_value = Update::of(
        40_000 + seed * 10 + u64::try_from(step).expect("fits"),
        0,
        vec![Change::under(
            Parent::body(),
            Content::values(vec![support::wire::value::whole(7)]),
        )],
    )
    .bytes();
    doc = assert_rollback_byte_equal(
        doc,
        point,
        &root_value,
        &ROOMY,
        &format!("seed {seed} step {step} unrepairable root value"),
    );
    (doc, hit_dirty)
}

fn two_by_two_table_body() -> serde_json::Value {
    let cell = json!({"type": "tableCell", "content": [{"type": "paragraph"}]});
    let row = json!({"type": "tableRow", "content": [cell.clone(), cell]});
    json!({"type": "doc", "content": [{"type": "table", "content": [row.clone(), row]}]})
}

/// An update that BOTH pushes reading steps past `READING_STEPS` AND leaves a broken
/// table grid (needing in-place `Mend::Pad` + `Mend::Spans` without `copy_of`) is
/// accepted, mended in place, and marked for remake, and remaking from its body takes 0
/// reading steps.
#[test]
fn table_mend_past_reading_steps_is_taken_in_place_and_marked_for_remake() {
    const TYPIST: u64 = 7;
    const CHILDREN: u64 = 20_000;
    let served = UpdateLimits {
        update_bytes: 64 * 1024,
        state_bytes: 8 * 1024 * 1024,
        body_bytes: fvoci_model::BODY_MAX_BYTES,
        placing_steps: Some(fvoci_collab::PLACING_STEPS),
        clients: Some(fvoci_collab::MAX_CLIENTS),
        client_changes: Some(fvoci_collab::MAX_CLIENT_CHANGES),
        reading_steps: Some(READING_STEPS),
    };
    let uncounted = UpdateLimits {
        update_bytes: usize::MAX,
        state_bytes: usize::MAX,
        placing_steps: None,
        clients: None,
        client_changes: None,
        reading_steps: None,
        ..served
    };
    let doc = from_body(&body_of(&two_by_two_table_body()));
    let typed_paras: Vec<Change> = (0..CHILDREN)
        .map(|clock| match clock.checked_sub(1) {
            None => Change::under(Parent::body(), Content::element("paragraph")),
            Some(before) => Change::after((TYPIST, before), Content::element("paragraph")),
        })
        .collect();
    let base_doc = take_update(
        &doc,
        &Update::of(TYPIST, 0, typed_paras).bytes(),
        &uncounted,
    )
    .expect("table + typed paragraphs")
    .doc;
    let base_state = state_of(&base_doc);
    let base_changes = state_counts(&base_state).expect("counts").changes;

    // One update from a concurrent editor adds a 1-cell row to the 2-column table
    // (needing Mend::Pad!) while also adding enough walking changes to exceed
    // READING_STEPS.
    let editor = fvoci_collab::testing::Editor::join(200, &base_state);
    let short_row = editor.add_table_row(1);
    let walkers_count = READING_STEPS / CHILDREN + 5;
    let walkers = Update::of(
        1_u64 << 40,
        0,
        (0..walkers_count)
            .map(|_| Change::Item {
                left: None,
                right: Some((TYPIST, CHILDREN - 1)),
                parent: Parent::body(),
                key: None,
                content: Content::Deleted(1),
            })
            .collect(),
    )
    .bytes();
    let combined = yrs::merge_updates_v1([&short_row, &walkers]).expect("merged");

    let held = BodyDoc::from_state(&base_state, &served)
        .expect("opened")
        .doc;
    let changed = held
        .take_in_place(&base_state, base_changes, &combined, &served)
        .expect("table mend + reading steps is taken in place");
    assert!(changed.remake, "past reading steps: must remake");
    assert_eq!(changed.repair.mended, 1, "short row padded by 1 cell");
    assert!(changed.repair.update.is_some(), "mend update emitted");
    assert!(
        changed.reading_steps > READING_STEPS,
        "{} steps",
        changed.reading_steps
    );
    let body = changed.repair.body.expect("valid mended body");
    let rows = body.document()["content"]
        .as_array()
        .expect("content")
        .iter()
        .find(|n| n["type"] == "table")
        .expect("table")["content"]
        .as_array()
        .expect("rows");
    assert_eq!(rows.len(), 3, "three rows");
    assert_eq!(
        rows[2]["content"].as_array().expect("cells").len(),
        2,
        "third row padded to 2 cells"
    );
    let remade = from_body(&body);
    assert_eq!(
        reading_steps(&state_of(&remade), u64::MAX).expect("steps"),
        0,
        "remade takes 0 reading steps"
    );
}

/// `from_state` opens canonical states (single-count fast path) and non-canonical states
/// cleanly, and refuses states past `READING_STEPS` before `yrs` reads them.
#[test]
fn from_state_opens_canonical_and_non_canonical_and_refuses_slow_states() {
    let doc = from_body(&body_of(&paragraphs(&["alpha", "beta"])));
    let canonical = state_of(&doc);
    let opened = from_state(&canonical, &ROOMY).expect("canonical state");
    assert_eq!(opened.state, canonical);
    assert_eq!(opened.state_bytes, canonical.len());
    assert_eq!(
        opened.changes,
        state_counts(&canonical).expect("counts").changes
    );
    assert!(!opened.remake);

    // Non-canonical state: two separate adjacent text blocks of one client that `yrs`
    // squashes on read-back (so `state_of(&doc) != input`).
    let non_canonical = Update::of(
        7,
        0,
        vec![
            Change::under(Parent::body(), Content::element("paragraph")),
            Change::under(Parent::Change((7, 0)), Content::xml_text()),
            Change::under(Parent::Change((7, 1)), Content::text("ab")),
            Change::after((7, 3), Content::text("cd")),
        ],
    )
    .bytes();
    let opened = from_state(&non_canonical, &ROOMY).expect("non-canonical state settles");
    assert_ne!(opened.state, non_canonical, "squashed into canonical form");
    assert_eq!(json_of(opened.repair.body.as_ref()), paragraphs(&["abcd"]));
    assert_eq!(
        from_state(&opened.state, &ROOMY).expect("reopen").state,
        opened.state
    );

    // State past `reading_steps` is refused before `yrs` reads it.
    let slow_limits = UpdateLimits {
        reading_steps: Some(0),
        ..ROOMY
    };
    let with_step = Update {
        runs: vec![
            support::wire::Run {
                client: 1,
                clock: 0,
                changes: vec![
                    Change::under(Parent::body(), Content::element("paragraph")),
                    Change::after((1, 0), Content::element("paragraph")),
                ],
            },
            support::wire::Run {
                client: 2,
                clock: 0,
                changes: vec![Change::Item {
                    left: None,
                    right: Some((1, 1)),
                    parent: Parent::body(),
                    key: None,
                    content: Content::element("paragraph"),
                }],
            },
        ],
        deleted: Vec::new(),
    }
    .bytes();
    assert!(matches!(
        from_state(&with_step, &slow_limits),
        Err(TakeError::Update(UpdateError::TooManyReadingSteps { .. }))
    ));
}

/// Regression for in-place `Mend::Spans` when an untrusted update sets `colspan` or
/// `rowspan` on a table cell to a non-empty nested shared type (`YArray` / `YMap`):
/// deleting the attribute recursively deletes its descendants in `txn.delete_set()`, so
/// `remove_attr` must read the attribute's own `BranchID::Nested` ID before deletion
/// rather than picking a descendant ID as the replacement's left origin.
#[test]
fn mend_spans_replaces_non_empty_nested_shared_type_attr() {
    let base_doc = from_body(&body_of(&two_by_two_table_body()));
    let base_state = state_of(&base_doc);
    let base_changes = state_counts(&base_state).expect("counts").changes;

    // Peer sets `colspan` to a non-empty YArray and `rowspan` to a non-empty YMap on cell (0,0).
    let peer = copy_of(&base_doc).expect("copy");
    let before_sv = peer.transact().state_vector();
    {
        let root = peer.get_or_insert_xml_fragment(FIELD);
        let mut txn = peer.transact_mut();
        let Some(XmlOut::Element(table)) = root.get(&txn, 0) else {
            panic!("table");
        };
        let Some(XmlOut::Element(row)) = table.get(&txn, 0) else {
            panic!("row");
        };
        let Some(XmlOut::Element(cell)) = row.get(&txn, 0) else {
            panic!("cell");
        };
        cell.insert_attribute(&mut txn, "colspan", ArrayPrelim::from(["bad"]));
        cell.insert_attribute(&mut txn, "rowspan", MapPrelim::from([("k", "v")]));
    }
    let bad_spans = peer.transact().encode_diff_v1(&before_sv);

    let held = BodyDoc::from_state(&base_state, &ROOMY).expect("open").doc;
    let changed = held
        .take_in_place(&base_state, base_changes, &bad_spans, &ROOMY)
        .expect("nested shared-type spans mended cleanly in place");
    assert_eq!(changed.repair.mended, 1, "cell spans mended");
    let repair_update = changed.repair.update.expect("mend update emitted");
    assert_eq!(
        json_of(changed.repair.body.as_ref()),
        two_by_two_table_body(),
        "mended table has valid default spans (1x1)"
    );

    // Applying the emitted repair update to the peer converges to the exact same state and body.
    let decoded = yrs::Update::decode_v1(&repair_update).expect("valid repair update");
    peer.transact_mut().apply_update(decoded).expect("applies");
    assert_eq!(
        json_of(to_body(&peer).expect("peer body").as_ref()),
        two_by_two_table_body(),
        "peer converges after repair update"
    );
    assert!(
        fvoci_collab::same_state(&state_of(&peer), &changed.state),
        "peer and server states match"
    );
}

/// A table mend that adds `writer_for` as a new client must still enforce `limits.clients`
/// and roll back byte-for-byte via `BodyDoc::rebuild` when exceeded.
#[test]
fn table_mend_refused_when_writer_exceeds_max_clients() {
    let base_doc = from_body(&body_of(&two_by_two_table_body()));
    let base_state = state_of(&base_doc);
    let base_changes = state_counts(&base_state).expect("counts").changes;
    let editor = fvoci_collab::testing::Editor::join(200, &base_state);
    let short_row = editor.add_table_row(1);

    let two_clients = UpdateLimits {
        clients: Some(2),
        ..ROOMY
    };
    let held = BodyDoc::from_state(&base_state, &two_clients)
        .expect("open")
        .doc;
    let err = held
        .take_in_place(&base_state, base_changes, &short_row, &two_clients)
        .expect_err("3rd client from mend writer refused");
    assert!(
        matches!(
            err,
            BodyTakeError::Dirty(TakeError::Update(UpdateError::TooManyClients {
                clients: 3,
                limit: 2
            }))
        ),
        "expected Dirty(TooManyClients(3, 2)), got {err:?}"
    );
    let rebuilt = BodyDoc::rebuild::<&[u8]>(&base_state, &[]).expect("rebuilt");
    assert_eq!(rebuilt.state(), base_state, "exact rollback");
}
