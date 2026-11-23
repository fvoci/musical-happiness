//! Reading stored states and settling documents by re-reading their encoded states
//! (`same_state`), run at open, fold, and close rather than on every live update.

use yrs::updates::decoder::Decode;
use yrs::{ClientID, Doc, ReadTxn, Transact, Update};

use crate::build::doc_of_client;
use crate::reading;
use crate::scan::{self, Refusal};
use crate::update::{
    Outcome, Settled, UpdateError, UpdateLimits, check_applied, check_client_changes, state_of,
};

/// The Y document whose state is `state`, read as any update is read: past the walk
/// (`scan`), with every change it builds on, and with nothing left waiting. It writes as
/// `writer`, or as a client new to it if `None`.
///
/// # Errors
///
/// Returns why the state is no whole document.
pub(crate) fn doc_of_state(state: &[u8], writer: Option<u64>) -> Result<Doc, String> {
    let walked = scan::check(state).map_err(|refusal| refusal.reason().to_owned())?;
    read_walked(state, &walked, writer)
}

/// [`doc_of_state`], with what the walk found of the state: how many changes it holds,
/// and the most of one client.
pub(crate) fn read_state(state: &[u8]) -> Result<(Doc, scan::Walked), String> {
    let walked = scan::check(state).map_err(|refusal| refusal.reason().to_owned())?;
    read_walked(state, &walked, None).map(|doc| (doc, walked))
}

/// The Y document of a state the walk has taken, which writes as `writer`, or as a
/// client new to it if `None`.
fn read_walked(state: &[u8], walked: &scan::Walked, writer: Option<u64>) -> Result<Doc, String> {
    if walked.begins.iter().any(|(_, clock)| *clock > 0) {
        return Err(UpdateError::MissingChanges.to_string());
    }
    let update = Update::decode_v1(state).map_err(|error| error.to_string())?;
    let doc = doc_of_client(writer);
    {
        let mut txn = doc.transact_mut();
        txn.apply_update(update)
            .map_err(|error| error.to_string())?;
        if txn.has_missing_updates() {
            return Err(UpdateError::MissingChanges.to_string());
        }
    }
    Ok(doc)
}

/// A document read from a state, with what was counted of the state before the library
/// read it.
struct Counted {
    doc: Doc,
    /// How many changes the state holds.
    changes: u64,
    /// The most changes of one client.
    client_changes: u64,
    /// The steps the reading took; 0 where they are not counted.
    reading_steps: u64,
    /// The most steps it may take once one more client has written; 0 likewise.
    steps_ahead: u64,
}

/// What [`count_and_read`] did with a state.
enum ReadOutcome {
    /// Within [`UpdateLimits::reading_steps`] (or uncounted) and read back into a fresh
    /// `Doc` (after dropping the prior `Doc`).
    Read(Counted),
    /// Past [`UpdateLimits::reading_steps`]: the library did not read it, and the prior
    /// `Doc` comes back unsettled.
    PastLimit {
        doc: Doc,
        changes: u64,
        client_changes: u64,
        steps: u64,
        steps_ahead: u64,
        limit: u64,
    },
}

/// Counts `state` against [`UpdateLimits::reading_steps`], [`UpdateLimits::client_changes`],
/// and strict origin placement; when within the reading-step limit, drops `doc` before
/// reading `state` back into a fresh `Doc` so only one `Doc` is live in memory at a time.
/// A state past the limit does not reach the library and keeps `doc` in
/// [`ReadOutcome::PastLimit`].
fn count_and_read(
    doc: Doc,
    state: &[u8],
    limits: &UpdateLimits,
) -> Result<ReadOutcome, UpdateError> {
    let unreadable = |refusal: Refusal| UpdateError::LeavesUnreadable(refusal.reason().to_owned());
    let Some(limit) = limits.reading_steps else {
        let read = reading::check_origins(state).map_err(unreadable)?;
        check_client_changes(read.walked.most, limits)?;
        drop(doc);
        let again =
            read_walked(state, &read.walked, None).map_err(UpdateError::LeavesUnreadable)?;
        check_applied(&again)?;
        check_ends(&again, &read.ends).map_err(unreadable)?;
        return Ok(ReadOutcome::Read(Counted {
            doc: again,
            changes: read.walked.changes,
            client_changes: read.walked.most,
            reading_steps: 0,
            steps_ahead: 0,
        }));
    };
    let read = reading::count_strict(state, limit).map_err(unreadable)?;
    check_client_changes(read.walked.most, limits)?;
    if read.steps > limit {
        return Ok(ReadOutcome::PastLimit {
            doc,
            changes: read.walked.changes,
            client_changes: read.walked.most,
            steps: read.steps,
            steps_ahead: read.ahead,
            limit,
        });
    }
    drop(doc);
    let again = read_walked(state, &read.walked, None).map_err(UpdateError::LeavesUnreadable)?;
    check_applied(&again)?;
    check_ends(&again, &read.ends).map_err(unreadable)?;
    Ok(ReadOutcome::Read(Counted {
        doc: again,
        changes: read.walked.changes,
        client_changes: read.walked.most,
        reading_steps: read.steps,
        steps_ahead: read.ahead,
    }))
}

/// Validates and reads `state` back into a fresh `Doc` without dropping any caller-owned
/// `Doc` beforehand, returning an error if `state` exceeds `state_bytes`,
/// `client_changes`, `reading_steps`, fails strict origin checks, or fails post-apply
/// invariants.
fn count_and_read_fresh(state: &[u8], limits: &UpdateLimits) -> Result<Counted, UpdateError> {
    if state.len() > limits.state_bytes {
        return Err(UpdateError::DocumentTooLarge {
            size: state.len(),
            limit: limits.state_bytes,
        });
    }
    let unreadable = |refusal: Refusal| UpdateError::LeavesUnreadable(refusal.reason().to_owned());
    let (read, reading_steps, steps_ahead) = if let Some(limit) = limits.reading_steps {
        let read = reading::count_strict(state, limit).map_err(unreadable)?;
        check_client_changes(read.walked.most, limits)?;
        if read.steps > limit {
            return Err(UpdateError::TooManyReadingSteps {
                steps: read.steps,
                limit,
            });
        }
        let steps = read.steps;
        let ahead = read.ahead;
        (read, steps, ahead)
    } else {
        let read = reading::check_origins(state).map_err(unreadable)?;
        check_client_changes(read.walked.most, limits)?;
        (read, 0, 0)
    };
    let doc = read_walked(state, &read.walked, None).map_err(UpdateError::LeavesUnreadable)?;
    check_applied(&doc)?;
    check_ends(&doc, &read.ends).map_err(unreadable)?;
    Ok(Counted {
        doc,
        changes: read.walked.changes,
        client_changes: read.walked.most,
        reading_steps,
        steps_ahead,
    })
}

/// Verifies that the library read every client's changes to the same length as
/// `reading::count_strict` / `reading::check_origins`: different lengths would mean
/// neighbours lay in other places.
pub(crate) fn check_ends(doc: &Doc, expected: &[(u64, u64)]) -> Result<(), Refusal> {
    let ends = doc.transact().state_vector();
    let as_the_library = ends.len() == expected.len()
        && expected
            .iter()
            .all(|(client, end)| u64::from(ends.get(&ClientID::new(*client))) == *end);
    if as_the_library {
        Ok(())
    } else {
        Err(Refusal::Lengths)
    }
}

/// Opens a stored state directly, enforcing strict origin placement and counting its
/// reading steps only once when `state` is already canonical (`same_state`). A state
/// past [`UpdateLimits::reading_steps`] is refused before `yrs` reads it.
pub(crate) fn from_state_settled(
    state: &[u8],
    limits: &UpdateLimits,
) -> Result<Settled, UpdateError> {
    if state.len() > limits.state_bytes {
        return Err(UpdateError::DocumentTooLarge {
            size: state.len(),
            limit: limits.state_bytes,
        });
    }
    let map_refusal = |refusal: Refusal| match refusal {
        Refusal::Gap => UpdateError::MissingChanges,
        other => UpdateError::Unreadable(other.reason().to_owned()),
    };
    let (read, steps, ahead) = if let Some(limit) = limits.reading_steps {
        let read = reading::count_strict(state, limit).map_err(map_refusal)?;
        if read.steps > limit {
            return Err(UpdateError::TooManyReadingSteps {
                steps: read.steps,
                limit,
            });
        }
        let steps = read.steps;
        let ahead = read.ahead;
        (read, steps, ahead)
    } else {
        let read = reading::check_origins(state).map_err(map_refusal)?;
        (read, 0, 0)
    };
    if let Some(limit) = limits.clients
        && read.walked.begins.len() > limit
    {
        return Err(UpdateError::TooManyClients {
            clients: read.walked.begins.len(),
            limit,
        });
    }
    check_client_changes(read.walked.most, limits)?;
    let update =
        Update::decode_v1(state).map_err(|error| UpdateError::Unreadable(error.to_string()))?;
    let doc = doc_of_client(None);
    {
        let mut txn = doc.transact_mut();
        txn.apply_update(update)
            .map_err(|error| UpdateError::Unappliable(error.to_string()))?;
    }
    check_applied(&doc)?;
    check_ends(&doc, &read.ends)
        .map_err(|refusal| UpdateError::LeavesUnreadable(refusal.reason().to_owned()))?;
    let encoded = state_of(&doc);
    if same_state(&encoded, state) {
        return Ok(Settled {
            doc,
            state: encoded,
            changes: read.walked.changes,
            client_changes: read.walked.most,
            reading_steps: steps,
            steps_ahead: ahead,
            outcome: Outcome::Settled,
            needs_settle: read.needs_settle,
        });
    }
    settled_from(doc, encoded, limits)
}

/// Rebuilds a document from a checkpoint `base` and the accepted updates and repairs
/// `since` it.
pub(crate) fn rebuild<U: AsRef<[u8]>>(base: &[u8], since: &[U]) -> Result<Doc, UpdateError> {
    let doc = doc_of_state(base, None).map_err(UpdateError::DocumentUnreadable)?;
    if since.is_empty() {
        return Ok(doc);
    }
    for entry in since {
        let bytes = entry.as_ref();
        scan::check(bytes)
            .map_err(|refusal| UpdateError::Unreadable(refusal.reason().to_owned()))?;
        let update =
            Update::decode_v1(bytes).map_err(|error| UpdateError::Unreadable(error.to_string()))?;
        let mut txn = doc.transact_mut();
        txn.apply_update(update)
            .map_err(|error| UpdateError::Unappliable(error.to_string()))?;
        if txn.has_missing_updates() {
            return Err(UpdateError::MissingChanges);
        }
    }
    let state = state_of(&doc);
    drop(doc);
    let again = doc_of_state(&state, None).map_err(UpdateError::LeavesUnreadable)?;
    let state_again = state_of(&again);
    if same_state(&state_again, &state) {
        return Ok(again);
    }
    drop(again);
    let twice = doc_of_state(&state_again, None).map_err(UpdateError::LeavesUnreadable)?;
    let state_settled = state_of(&twice);
    if !same_state(&state_settled, &state_again) {
        return Err(UpdateError::LeavesUnreadable(
            "its state reads to a document whose state reads to another".to_owned(),
        ));
    }
    Ok(twice)
}

/// A document as a server that stored it reads it: its state within the limit of size,
/// read again within the steps a reading may take, and reading again to the same changes
/// and deletions at the latest the second time ([`same_state`]). Where reading its state
/// would take more steps than [`UpdateLimits::reading_steps`], returns `doc` unsettled
/// with [`Outcome::Remake`] instead of handing the slow state to the library.
pub(crate) fn settled_sized(doc: Doc, limits: &UpdateLimits) -> Result<Settled, UpdateError> {
    let state = state_of(&doc);
    settled_from(doc, state, limits)
}

/// [`settled_sized`] given `doc` and its already-encoded `state`.
pub(crate) fn settled_from(
    doc: Doc,
    state: Vec<u8>,
    limits: &UpdateLimits,
) -> Result<Settled, UpdateError> {
    if state.len() > limits.state_bytes {
        return Err(UpdateError::DocumentTooLarge {
            size: state.len(),
            limit: limits.state_bytes,
        });
    }
    let again = match count_and_read(doc, &state, limits)? {
        ReadOutcome::Read(again) => again,
        ReadOutcome::PastLimit {
            doc,
            changes,
            client_changes,
            steps,
            steps_ahead,
            limit,
        } => {
            return Ok(Settled {
                doc,
                state,
                changes,
                client_changes,
                reading_steps: steps,
                steps_ahead,
                outcome: Outcome::Remake { steps, limit },
                needs_settle: true,
            });
        }
    };
    let state_again = state_of(&again.doc);
    if same_state(&state_again, &state) {
        return Ok(Settled {
            doc: again.doc,
            state: state_again,
            changes: again.changes,
            client_changes: again.client_changes,
            reading_steps: again.reading_steps,
            steps_ahead: again.steps_ahead,
            outcome: Outcome::Settled,
            needs_settle: false,
        });
    }
    drop(state);
    if state_again.len() > limits.state_bytes {
        return Err(UpdateError::DocumentTooLarge {
            size: state_again.len(),
            limit: limits.state_bytes,
        });
    }
    // A first reading may write the same document otherwise (neighbouring changes it
    // joins); a second must not. What it writes is what is stored and read from then
    // on, and a change it joined is cut again by every reading: counted as well.
    let read_twice = match count_and_read(again.doc, &state_again, limits)? {
        ReadOutcome::Read(read_twice) => read_twice,
        ReadOutcome::PastLimit {
            doc,
            changes,
            client_changes,
            steps,
            steps_ahead,
            limit,
        } => {
            return Ok(Settled {
                doc,
                state: state_again,
                changes,
                client_changes: again.client_changes.max(client_changes),
                reading_steps: steps,
                steps_ahead,
                outcome: Outcome::Remake { steps, limit },
                needs_settle: true,
            });
        }
    };
    let state_settled = state_of(&read_twice.doc);
    if !same_state(&state_settled, &state_again) {
        return Err(UpdateError::LeavesUnreadable(
            "its state reads to a document whose state reads to another".to_owned(),
        ));
    }
    Ok(Settled {
        doc: read_twice.doc,
        state: state_settled,
        changes: read_twice.changes,
        client_changes: again.client_changes.max(read_twice.client_changes),
        reading_steps: read_twice.reading_steps,
        steps_ahead: read_twice.steps_ahead,
        outcome: Outcome::Settled,
        needs_settle: false,
    })
}

/// Settles `doc` from its already-encoded `state` (at fold or close) while keeping `doc`
/// intact on any error so the caller can project its body and remake it without losing
/// updates.
pub(crate) fn settle_keeping(
    doc: Doc,
    state: Vec<u8>,
    limits: &UpdateLimits,
) -> Result<Settled, (Doc, UpdateError)> {
    let again = match count_and_read_fresh(&state, limits) {
        Ok(again) => again,
        Err(error) => return Err((doc, error)),
    };
    let state_again = state_of(&again.doc);
    if same_state(&state_again, &state) {
        drop(doc);
        return Ok(Settled {
            doc: again.doc,
            state: state_again,
            changes: again.changes,
            client_changes: again.client_changes,
            reading_steps: again.reading_steps,
            steps_ahead: again.steps_ahead,
            outcome: Outcome::Settled,
            needs_settle: false,
        });
    }
    drop(state);
    let first_client_changes = again.client_changes;
    drop(again);
    let read_twice = match count_and_read_fresh(&state_again, limits) {
        Ok(read_twice) => read_twice,
        Err(error) => return Err((doc, error)),
    };
    let state_settled = state_of(&read_twice.doc);
    if !same_state(&state_settled, &state_again) {
        return Err((
            doc,
            UpdateError::LeavesUnreadable(
                "its state reads to a document whose state reads to another".to_owned(),
            ),
        ));
    }
    drop(doc);
    Ok(Settled {
        doc: read_twice.doc,
        state: state_settled,
        changes: read_twice.changes,
        client_changes: first_client_changes.max(read_twice.client_changes),
        reading_steps: read_twice.reading_steps,
        steps_ahead: read_twice.steps_ahead,
        outcome: Outcome::Settled,
        needs_settle: false,
    })
}

/// Whether two states represent the same changes and deletions regardless of hash-table
/// encoding order (the library writes each client's deletions in table order). Misplaced
/// origins that could read non-deterministically are refused earlier during strict origin
/// validation.
#[must_use]
pub fn same_state(one: &[u8], other: &[u8]) -> bool {
    one == other
        || matches!(
            (Update::decode_v1(one), Update::decode_v1(other)),
            (Ok(one), Ok(other)) if one == other
        )
}
