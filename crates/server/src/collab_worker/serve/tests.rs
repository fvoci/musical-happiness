use super::super::frame::read_frame;
use super::super::wire::decode_response;
use super::*;

const LIMITS: Limits = Limits {
    update_bytes: 1024,
    state_bytes: 4096,
    body_bytes: 4096,
    documents: 1,
    held_bytes: 1 << 20,
};

/// The answers of a worker to `requests`, the configuration of [`LIMITS`] first.
fn answers(requests: &[Request]) -> Vec<Response> {
    let mut input = Vec::new();
    write_frame(&mut input, &Request::Configure(LIMITS).encode(), 64).expect("written");
    for request in requests {
        write_frame(&mut input, &request.encode(), 1 << 20).expect("written");
    }
    let mut output = Vec::new();
    serve(&mut input.as_slice(), &mut output).expect("served");
    let mut output = output.as_slice();
    let configured = read_frame(&mut output, 1 << 20).expect("an answer");
    assert_eq!(
        configured.map(|frame| decode_response(&frame)),
        Some(Ok(Ok(Answer::Done))),
        "the configuration is answered"
    );
    let mut answers = Vec::new();
    while let Some(frame) = read_frame(&mut output, 1 << 20).expect("an answer") {
        answers.push(decode_response(&frame).expect("an answer"));
    }
    answers
}

#[test]
fn a_document_opens_once_and_within_the_number_of_documents() {
    let body = br#"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"x"}]}]}"#;
    let got = answers(&[
        Request::Open {
            doc: 1,
            source: Source::Body(Some(body.to_vec())),
            share: Share::Kept,
        },
        Request::Open {
            doc: 1,
            source: Source::Body(None),
            share: Share::Kept,
        },
        Request::Open {
            doc: 2,
            source: Source::Body(None),
            share: Share::Kept,
        },
        Request::Body(1),
        Request::Close(1),
        Request::Body(1),
    ]);
    assert!(matches!(got[0], Ok(Answer::Opened { state: Some(_), .. })));
    let refusal = |index: usize| got[index].as_ref().map_err(|refused| refused.refusal).err();
    assert_eq!(refusal(1), Some(Refusal::UnknownDocument), "open already");
    assert_eq!(refusal(2), Some(Refusal::TooManyDocuments), "one at a time");
    let Ok(Answer::Body(Some(json))) = &got[3] else {
        panic!("its body: {:?}", got[3]);
    };
    let json: serde_json::Value = serde_json::from_slice(json).expect("JSON");
    let sent: serde_json::Value = serde_json::from_slice(body).expect("JSON");
    assert_eq!(json, sent, "its body");
    assert_eq!(refusal(5), Some(Refusal::UnknownDocument), "closed");
}

#[test]
fn a_worker_that_holds_as_many_documents_as_it_may_opens_some_aside() {
    let open = |doc: DocNumber, share: Share| Request::Open {
        doc,
        source: Source::Body(None),
        share,
    };
    let aside: Vec<DocNumber> = (10..).take(ASIDE_DOCUMENTS + 1).collect();
    let mut requests = vec![open(1, Share::Kept)];
    requests.extend(aside.iter().map(|doc| open(*doc, Share::Aside)));
    requests.extend([
        Request::Close(10),
        open(30, Share::Aside),
        open(2, Share::Kept),
    ]);
    let got = answers(&requests);
    let refused = |index: usize| got[index].as_ref().map_err(|refused| refused.refusal).err();
    let last = ASIDE_DOCUMENTS;
    assert!((0..=last).all(|index| refused(index).is_none()), "{got:?}");
    assert_eq!(
        refused(last + 1),
        Some(Refusal::TooManyDocuments),
        "past the share aside"
    );
    assert_eq!(refused(last + 3), None, "one closed aside makes room aside");
    assert_eq!(
        refused(last + 4),
        Some(Refusal::TooManyDocuments),
        "none kept past the limit"
    );
}

#[test]
fn documents_aside_leave_the_kept_share_alone() {
    let open = |doc: DocNumber, share: Share| Request::Open {
        doc,
        source: Source::Body(None),
        share,
    };
    let mut requests: Vec<Request> = (10..)
        .take(ASIDE_DOCUMENTS)
        .map(|doc| open(doc, Share::Aside))
        .collect();
    requests.push(open(1, Share::Kept));
    let got = answers(&requests);
    assert!(got.iter().all(Result::is_ok), "{got:?}");
}

#[test]
fn a_worker_that_is_sent_no_configuration_first_ends() {
    let mut input = Vec::new();
    write_frame(&mut input, &Request::State(1).encode(), 64).expect("written");
    assert!(serve(&mut input.as_slice(), &mut Vec::new()).is_err());
    assert!(
        serve(&mut [].as_slice(), &mut Vec::new()).is_ok(),
        "a pipe closed at once"
    );
}

#[test]
fn a_document_marked_for_remake_allows_only_body_and_close() {
    let mut worker = Worker::new(LIMITS);
    let opened = worker.answer(Request::Open {
        doc: 1,
        source: Source::Body(None),
        share: Share::Kept,
    });
    assert!(
        matches!(opened, Ok(Answer::Opened { .. })),
        "opened: {opened:?}"
    );
    worker.docs.get_mut(&1).expect("held").must_remake = true;

    let refusal = |response: Response| response.err().map(|refused| refused.refusal);
    assert_eq!(
        refusal(worker.answer(Request::Take {
            doc: 1,
            update: Vec::new(),
        })),
        Some(Refusal::OutOfOrder),
        "Take is blocked until remade"
    );
    assert_eq!(
        refusal(worker.answer(Request::Retake {
            doc: 1,
            update: Vec::new(),
        })),
        Some(Refusal::Unappliable),
        "Retake is refused cleanly so replay falls back to the card"
    );
    assert_eq!(
        refusal(worker.answer(Request::State(1))),
        Some(Refusal::OutOfOrder),
        "State is blocked so unsettled state cannot be folded"
    );
    assert_eq!(
        refusal(worker.answer(Request::StateVector(1))),
        Some(Refusal::OutOfOrder),
        "StateVector is blocked until remade"
    );
    assert_eq!(
        refusal(worker.answer(Request::Diff {
            doc: 1,
            state_vector: vec![0],
        })),
        Some(Refusal::OutOfOrder),
        "Diff is blocked until remade"
    );
    assert!(
        matches!(worker.answer(Request::Body(1)), Ok(Answer::Body(None))),
        "Body is allowed for projection"
    );
    assert_eq!(
        worker.answer(Request::Close(1)),
        Ok(Answer::Done),
        "Close is allowed for remake or forget"
    );
}

/// Verifies exact `held_bytes` accounting across `Open`, `Take`, repaired `Take`,
/// no-op `Take`, pre-apply (`Untouched`) and post-apply (`Dirty`) refusals,
/// `State` rebase, and `Close`.
#[test]
fn since_bytes_are_counted_in_held_bytes_and_reclaimed_by_state_and_close() {
    let mut worker = Worker::new(Limits {
        update_bytes: 4096,
        state_bytes: 16 * 1024,
        body_bytes: 16 * 1024,
        documents: 4,
        held_bytes: 1 << 20,
    });
    let Ok(Answer::Opened {
        state: Some(initial_state),
        state_bytes: s0,
        ..
    }) = worker.answer(Request::Open {
        doc: 1,
        source: Source::Body(None),
        share: Share::Kept,
    })
    else {
        panic!("opened");
    };
    assert_eq!(worker.held_bytes, s0, "open accounts state_bytes only");

    let editor = fvoci_collab::testing::Editor::join(101, &initial_state);
    let u1 = editor.append_paragraph("hello");
    let Ok(Answer::Taken {
        repair: None,
        state_bytes: s1,
        fold: false,
        ..
    }) = worker.answer(Request::Take {
        doc: 1,
        update: u1.clone(),
    })
    else {
        panic!("u1 taken");
    };
    let held = worker.docs.get(&1).expect("held");
    assert_eq!(held.since.len(), 1);
    assert_eq!(held.since_bytes, size_of(&u1));
    assert_eq!(worker.held_bytes, s1 + size_of(&u1));

    // Duplicate (no-op) update does not bloat `since` or `held_bytes`.
    let before_bytes = worker.held_bytes;
    let Ok(Answer::Taken { repair: None, .. }) = worker.answer(Request::Take {
        doc: 1,
        update: u1.clone(),
    }) else {
        panic!("duplicate taken as no-op");
    };
    assert_eq!(worker.docs.get(&1).expect("held").since.len(), 1);
    assert_eq!(worker.held_bytes, before_bytes);

    // Repaired update adds both update and repair bytes to `since` and `held_bytes`.
    let u2 = editor.append_element("nothing-a-body-has");
    let Ok(Answer::Taken {
        repair: Some(r2),
        state_bytes: s2,
        ..
    }) = worker.answer(Request::Take {
        doc: 1,
        update: u2.clone(),
    })
    else {
        panic!("u2 repaired");
    };
    editor.receive(&r2);
    let expected_since = size_of(&u1) + size_of(&u2) + size_of(&r2);
    assert_eq!(worker.docs.get(&1).expect("held").since.len(), 3);
    assert_eq!(
        worker.docs.get(&1).expect("held").since_bytes,
        expected_since
    );
    assert_eq!(worker.held_bytes, s2 + expected_since);

    let state_before_rebase = worker.docs.get(&1).expect("held").doc.state();

    // `Request::State` returns exact state, rebases checkpoint, clears `since`, and
    // reclaims `since_bytes` from `held_bytes`.
    let Ok(Answer::Bytes(got_state)) = worker.answer(Request::State(1)) else {
        panic!("state");
    };
    assert_eq!(got_state, state_before_rebase);
    let held = worker.docs.get(&1).expect("held");
    assert_eq!(held.since.len(), 0);
    assert_eq!(held.since_bytes, 0);
    assert_eq!(worker.held_bytes, s2);

    // Add another update then Close: all `state_bytes + since_bytes` reclaimed to 0.
    let u3 = editor.append_paragraph("after rebase");
    assert!(worker.answer(Request::Take { doc: 1, update: u3 }).is_ok());
    assert!(worker.held_bytes > s2);
    assert_eq!(worker.answer(Request::Close(1)), Ok(Answer::Done));
    assert_eq!(worker.held_bytes, 0, "no leak or underflow on Close");
}

/// Pre-apply (`Untouched`) and post-apply (`Dirty`) refusals preserve the held
/// document byte-for-byte and leave `held_bytes` unchanged.
#[test]
fn refused_in_place_take_preserves_state_and_held_bytes() {
    let mut worker = Worker::new(Limits {
        update_bytes: 4096,
        state_bytes: 16 * 1024,
        body_bytes: 16 * 1024,
        documents: 4,
        held_bytes: 1 << 20,
    });
    let Ok(Answer::Opened {
        state: Some(initial_state),
        ..
    }) = worker.answer(Request::Open {
        doc: 1,
        source: Source::Body(None),
        share: Share::Kept,
    })
    else {
        panic!("opened");
    };
    let editor = fvoci_collab::testing::Editor::join(111, &initial_state);
    let u1 = editor.append_paragraph("hello");
    assert!(worker.answer(Request::Take { doc: 1, update: u1 }).is_ok());
    let u2 = editor.append_element("nothing-a-body-has");
    let Ok(Answer::Taken {
        repair: Some(r2), ..
    }) = worker.answer(Request::Take { doc: 1, update: u2 })
    else {
        panic!("u2 repaired");
    };
    editor.receive(&r2);

    let state_before = worker.docs.get(&1).expect("held").doc.state();
    let held_before = worker.held_bytes;

    // Pre-apply refusal (Untouched) preserves state and held_bytes.
    assert!(
        worker
            .answer(Request::Take {
                doc: 1,
                update: vec![1, 2, 3],
            })
            .is_err()
    );
    assert_eq!(worker.docs.get(&1).expect("held").doc.state(), state_before);
    assert_eq!(worker.held_bytes, held_before);

    // Post-apply refusal (Dirty: body_bytes exceeded) rebuilds byte-for-byte from
    // `base` + `since` and preserves held_bytes.
    let huge = editor.append_paragraph(&"x".repeat(3000));
    worker.update_limits.body_bytes = 10;
    let refused = worker.answer(Request::Take {
        doc: 1,
        update: huge,
    });
    assert_eq!(
        refused.err().map(|r| r.refusal),
        Some(Refusal::BodyTooLarge)
    );
    assert_eq!(
        worker.docs.get(&1).expect("held").doc.state(),
        state_before,
        "rebuilt byte-for-byte identical after Dirty refusal"
    );
    assert_eq!(worker.held_bytes, held_before);
}

/// `since` triggers `fold: true` at `state_bytes / 2` and hard-caps with
/// `Refusal::TooManyDocuments` at `state_bytes` (rolling back cleanly); rebasing via
/// `Request::State` lets updates resume.
#[test]
fn since_fold_signal_and_hard_cap_enforce_bounds_without_corrupting_state() {
    let mut worker = Worker::new(Limits {
        update_bytes: 4096,
        state_bytes: 1024,
        body_bytes: 4096,
        documents: 1,
        held_bytes: 1 << 20,
    });
    let Ok(Answer::Opened {
        state: Some(initial_state),
        ..
    }) = worker.answer(Request::Open {
        doc: 1,
        source: Source::Body(None),
        share: Share::Kept,
    })
    else {
        panic!("opened");
    };
    let editor = fvoci_collab::testing::Editor::join(102, &initial_state);
    // Repaired updates store both the client update and the repair update in `since`, so
    // `since_bytes` grows faster than `state_bytes` and crosses both `fold_since_bytes`
    // (512) and `max_since_bytes` (1024) while `state_bytes` stays below 1024.
    let mut saw_fold = false;
    let mut refused_cap = false;
    let mut last_good_state = initial_state;
    let mut last_held_bytes = worker.held_bytes;
    for _ in 0..40 {
        let update = editor.append_element("nothing-a-body-has");
        match worker.answer(Request::Take { doc: 1, update }) {
            Ok(Answer::Taken { repair, fold, .. }) => {
                if let Some(repair) = repair {
                    editor.receive(&repair);
                }
                saw_fold |= fold;
                last_good_state = worker.docs.get(&1).expect("held").doc.state();
                last_held_bytes = worker.held_bytes;
            }
            Err(refused) => {
                assert_eq!(refused.refusal, Refusal::TooManyDocuments);
                assert_eq!(
                    worker.docs.get(&1).expect("held").doc.state(),
                    last_good_state,
                    "state rolled back byte-for-byte on since hard cap"
                );
                assert_eq!(
                    worker.held_bytes, last_held_bytes,
                    "held_bytes unchanged on since hard cap refusal"
                );
                refused_cap = true;
                break;
            }
            other => panic!("unexpected: {other:?}"),
        }
    }
    assert!(saw_fold, "fold signal fired at state_bytes / 2");
    assert!(refused_cap, "hard cap refused past state_bytes");

    // After `Request::State` rebases `since`, updates can continue.
    let Ok(Answer::Bytes(rebased)) = worker.answer(Request::State(1)) else {
        panic!("state");
    };
    assert_eq!(rebased, last_good_state);
    assert_eq!(worker.docs.get(&1).expect("held").since_bytes, 0);
    let editor2 = fvoci_collab::testing::Editor::join(103, &rebased);
    let small = editor2.append_paragraph("ok");
    assert!(matches!(
        worker.answer(Request::Take {
            doc: 1,
            update: small,
        }),
        Ok(Answer::Taken { fold: false, .. })
    ));
}

/// Tight `held_bytes` counts `since_bytes` alongside `state_bytes`, refuses updates that
/// would push `state_bytes + since_bytes` over `held_bytes` with exact rollback, and
/// frees room once `Request::State` rebases `since`.
#[test]
fn held_bytes_budget_enforces_state_plus_since_and_rolls_back_on_overflow() {
    let mut worker = Worker::new(Limits {
        update_bytes: 4096,
        state_bytes: 16 * 1024,
        body_bytes: 16 * 1024,
        documents: 2,
        held_bytes: 350,
    });
    let Ok(Answer::Opened {
        state: Some(initial_state),
        ..
    }) = worker.answer(Request::Open {
        doc: 1,
        source: Source::Body(None),
        share: Share::Kept,
    })
    else {
        panic!("opened");
    };
    let editor = fvoci_collab::testing::Editor::join(201, &initial_state);
    let first = editor.append_paragraph("short");
    assert!(
        worker
            .answer(Request::Take {
                doc: 1,
                update: first,
            })
            .is_ok()
    );
    let before_state = worker.docs.get(&1).expect("held").doc.state();
    let before_held = worker.held_bytes;

    // A 200-byte update fits `state_bytes` (16 KiB) alone, but `state_bytes + since_bytes`
    // exceeds `held_bytes` (350): refused with `TooManyDocuments` and rolled back cleanly.
    let overflow = editor.append_paragraph(&"z".repeat(200));
    let err = worker
        .answer(Request::Take {
            doc: 1,
            update: overflow,
        })
        .expect_err("over held_bytes");
    assert_eq!(err.refusal, Refusal::TooManyDocuments);
    assert_eq!(
        worker.docs.get(&1).expect("held").doc.state(),
        before_state,
        "exact rollback on held_bytes overflow"
    );
    assert_eq!(worker.held_bytes, before_held);
    assert_eq!(worker.answer(Request::Close(1)), Ok(Answer::Done));
    assert_eq!(worker.held_bytes, 0);
}
