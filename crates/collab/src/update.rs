//! Taking an update from a client.

use thiserror::Error;
use yrs::updates::decoder::Decode;
use yrs::{ClientID, Doc, Out, ReadTxn, StateVector, Transact, Update};

use crate::FIELD;
use crate::build::doc_of_client;
use crate::placing::{self, Held, Sent};
use crate::scan;

/// The sizes an update may have and leave.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UpdateLimits {
    /// The largest update, in bytes as it arrives.
    pub update_bytes: usize,
    /// The largest Y document an update may leave, in bytes as it is stored (a version
    /// 1 update of the whole document). The documents of this crate keep what was
    /// deleted (they are made without garbage collection), so a document grows with
    /// every edit even where the body does not.
    pub state_bytes: usize,
    /// The largest body an update may leave, in bytes of its stored JSON. Every body is
    /// held to [`fvoci_model::BODY_MAX_BYTES`] as well, so a larger value is that.
    pub body_bytes: usize,
    /// How many changes of the document the Y library may have to walk past to place
    /// the changes of one update ([`PLACING_STEPS`] for what a client sends); `None`
    /// for no count, for what was taken before and is read again.
    pub placing_steps: Option<u64>,
    /// How many clients the document an update leaves may name ([`MAX_CLIENTS`] for
    /// what a client sends); `None` for no count, for what was taken before.
    pub clients: Option<usize>,
}

/// How many clients a document an update a client sends leaves may name
/// ([`UpdateLimits::clients`]). The Y library reads the changes of a state client by
/// client and looks for each among the others, so that reading a state takes the
/// square of the clients it names: a letter typed into a document of 10,000 clients is
/// taken in 0.1 s, of 20,000 in 0.9 s and of 40,000 in 4.9 s (release build). An
/// editor takes a client for each time it opens the document, so a document grows to
/// it only in years; what holds one makes it again from its body, with one client,
/// well before.
pub const MAX_CLIENTS: usize = 16 * 1024;

/// How many changes of a document the changes of one update a client sends may be
/// placed past ([`UpdateLimits::placing_steps`]): the Y library walks 16 million
/// children made from a body in 0.8 s and as many typed one after the other in 1.9 s
/// (50 and 110 ns each, release build), and an update placed past as many is taken in
/// a fifth of the time a worker has for its answer. An editor's update is
/// placed past a handful. Past it are updates of hundreds of changes that each go
/// into a list of tens of thousands: 500 separate insertions among 33,000 children
/// of one node, sent as one update.
pub const PLACING_STEPS: u64 = 16 * 1024 * 1024;

/// Why an update is refused.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum UpdateError {
    /// The update is larger than [`UpdateLimits::update_bytes`].
    #[error("the update is {size} bytes, more than the {limit} allowed")]
    TooLarge {
        /// The size of the update.
        size: usize,
        /// The limit.
        limit: usize,
    },
    /// The bytes are no update.
    #[error("the update cannot be read: {0}")]
    Unreadable(String),
    /// The update cannot be applied to the document.
    #[error("the update cannot be applied: {0}")]
    Unappliable(String),
    /// The update builds on changes the document does not have: a client's changes
    /// begin later than where the document has them, or part of the update waits for
    /// changes of another client instead of taking effect. A client sends what it has
    /// in order; one that leaves a gap is not in step with the document.
    #[error("the update builds on changes the document does not have")]
    MissingChanges,
    /// The update writes outside the fragment that holds the body.
    #[error("the update writes to the root {0:?}, which is not the body's")]
    UnexpectedRoot(String),
    /// The changes of the update would be placed past more changes of the document
    /// than [`UpdateLimits::placing_steps`]: each change that names a neighbour the
    /// document has to its left, or none, is counted the list it goes into.
    #[error(
        "the changes of the update would be placed past {steps} changes of the document, \
         more than the {limit} allowed"
    )]
    PlacedPastTooMany {
        /// The changes of the document its changes may be placed past, counted no
        /// further than is needed to know that they are more than the limit.
        steps: u64,
        /// The limit.
        limit: u64,
    },
    /// With the update the document would name more clients than
    /// [`UpdateLimits::clients`].
    #[error(
        "with the update the document would name {clients} clients, more than the {limit} allowed"
    )]
    TooManyClients {
        /// The clients it would name.
        clients: usize,
        /// The limit.
        limit: usize,
    },
    /// With the update the document would be larger than [`UpdateLimits::state_bytes`].
    #[error("with the update the document would be {size} bytes, more than the {limit} allowed")]
    DocumentTooLarge {
        /// The size the document would have.
        size: usize,
        /// The limit.
        limit: usize,
    },
    /// With the update the document could not be read again from what is stored of it:
    /// the library took something that it writes in another form than it read it.
    #[error("with the update the document could not be read again: {0}")]
    LeavesUnreadable(String),
    /// The document the update was given for cannot be read again from its own state,
    /// so that no update can be taken for it. Nothing this crate takes leaves one so.
    #[error("the document cannot be read again from its own state: {0}")]
    DocumentUnreadable(String),
}

/// The whole of a Y document as a version 1 update: what is stored, and what brings an
/// empty document to the same state.
#[must_use]
pub fn state_of(doc: &Doc) -> Vec<u8> {
    doc.transact()
        .encode_state_as_update_v1(&StateVector::default())
}

/// A copy of a Y document that shares nothing with it. (Cloning a `Doc` gives a second
/// handle to the same document.)
///
/// # Errors
///
/// Returns [`UpdateError::DocumentUnreadable`] if the document's own state cannot be
/// read again as any update is read.
pub(crate) fn copy_of(doc: &Doc) -> Result<Doc, UpdateError> {
    doc_of_state(&state_of(doc)).map_err(UpdateError::DocumentUnreadable)
}

/// The Y document whose state is `state`, read as any update is read: past the walk
/// (`scan`), with every change it builds on, and with nothing left waiting.
///
/// # Errors
///
/// Returns why the state is no whole document.
fn doc_of_state(state: &[u8]) -> Result<Doc, String> {
    read_state(state).map(|(doc, _)| doc)
}

/// [`doc_of_state`], with how many changes the state holds.
fn read_state(state: &[u8]) -> Result<(Doc, u64), String> {
    let walked = scan::check(state).map_err(|refusal| refusal.reason().to_owned())?;
    if walked.begins.iter().any(|(_, clock)| *clock > 0) {
        return Err(UpdateError::MissingChanges.to_string());
    }
    let update = Update::decode_v1(state).map_err(|error| error.to_string())?;
    let doc = doc_of_client(None);
    {
        let mut txn = doc.transact_mut();
        txn.apply_update(update)
            .map_err(|error| error.to_string())?;
        if txn.has_missing_updates() {
            return Err(UpdateError::MissingChanges.to_string());
        }
    }
    Ok((doc, walked.changes))
}

/// Reads the bytes of an update for a document, after the walk that keeps from the
/// library what it does not survive (`scan`), with how many clients the document names
/// with it.
fn read(doc: &Doc, update: &[u8]) -> Result<(Update, usize), UpdateError> {
    let begins = scan::check(update)
        .map_err(|refusal| UpdateError::Unreadable(refusal.reason().to_owned()))?
        .begins;
    // A client's changes go on from where the document has them: the library would
    // take changes that begin later and leave a hole before them.
    let known = doc.transact().state_vector();
    let mut clients = known.len();
    for (client, clock) in begins {
        let client = ClientID::new(client);
        if clock > u64::from(known.get(&client)) {
            return Err(UpdateError::MissingChanges);
        }
        if !known.contains_client(&client) {
            clients += 1;
        }
    }
    let update =
        Update::decode_v1(update).map_err(|error| UpdateError::Unreadable(error.to_string()))?;
    Ok((update, clients))
}

/// The Y document as it would be with a client's update: a copy of `doc` with the
/// update applied, read again from its state. `doc` itself is not changed.
///
/// What comes back has still to be projected and, where that fails, repaired: this only
/// refuses what cannot be taken at all. [`crate::take_update`] does all of it, and says
/// what a server that calls it keeps in mind about time.
///
/// Not everything an update may write is read by the projection: attributes the schema
/// does not have, attributes of a text. No editor reads them
/// either. They stay in the document, and [`UpdateLimits::state_bytes`] is what bounds
/// them.
///
/// # Errors
///
/// Returns an [`UpdateError`] if the update is too large, is no update, cannot be
/// applied, leaves a part of itself waiting for changes the document does not have,
/// writes to another root than the body's, leaves the document too large or in a
/// state that cannot be read again, or if `doc` itself cannot be read again.
#[cfg(any(test, feature = "test-util"))]
pub(crate) fn with_update(
    doc: &Doc,
    update: &[u8],
    limits: &UpdateLimits,
) -> Result<Doc, UpdateError> {
    with_update_sized(doc, update, limits).map(|(doc, _)| doc)
}

/// [`with_update`], with the size of the state of what comes back.
pub(crate) fn with_update_sized(
    doc: &Doc,
    update: &[u8],
    limits: &UpdateLimits,
) -> Result<(Doc, usize), UpdateError> {
    if update.len() > limits.update_bytes {
        return Err(UpdateError::TooLarge {
            size: update.len(),
            limit: limits.update_bytes,
        });
    }
    let (sent, clients) = read(doc, update)?;
    // Before the document is read, which takes the square of its clients.
    if let Some(limit) = limits.clients
        && clients > limit
    {
        return Err(UpdateError::TooManyClients { clients, limit });
    }
    let state = state_of(doc);
    let (copy, changes) = read_state(&state).map_err(UpdateError::DocumentUnreadable)?;
    if let Some(limit) = limits.placing_steps {
        let steps = charged(&copy, (&state, changes), (update, &sent), limit)?;
        if steps > limit {
            return Err(UpdateError::PlacedPastTooMany { steps, limit });
        }
    }
    drop(state);
    let update = sent;
    {
        let mut txn = copy.transact_mut();
        txn.apply_update(update)
            .map_err(|error| UpdateError::Unappliable(error.to_string()))?;
    }
    {
        let txn = copy.transact();
        if txn.has_missing_updates() {
            return Err(UpdateError::MissingChanges);
        }
        for (name, root) in txn.root_refs() {
            if name != FIELD || !matches!(root, Out::YXmlFragment(_)) {
                return Err(UpdateError::UnexpectedRoot(name.to_owned()));
            }
        }
    }
    settled_sized(&copy, limits)
}

/// What the changes of an update are charged for the document of a state
/// ([`placing::charge`]): `copy` is the document as the library read it from `state`,
/// which holds `changes`, and `sent` the update as the library read it from `update`.
fn charged(
    copy: &Doc,
    (state, changes): (&[u8], u64),
    (update, sent): (&[u8], &Update),
    limit: u64,
) -> Result<u64, UpdateError> {
    let held = Held {
        state,
        changes,
        ends: &copy.transact().state_vector(),
    };
    let sent = Sent {
        update,
        covered: &sent.insertions(true),
    };
    placing::charge(&held, &sent, limit)
        .map_err(|refusal| UpdateError::Unreadable(refusal.reason().to_owned()))
}

/// What the changes of `update` are charged for `doc`, counted list by list whatever
/// the update holds.
#[cfg(any(test, feature = "test-util"))]
pub(crate) fn placing_steps(doc: &Doc, update: &[u8]) -> Result<u64, UpdateError> {
    let (sent, _) = read(doc, update)?;
    let state = state_of(doc);
    let (copy, changes) = read_state(&state).map_err(UpdateError::DocumentUnreadable)?;
    // Past a limit of nothing every update is counted list by list.
    charged(&copy, (&state, changes), (update, &sent), 0)
}

/// A document as a server that stored it reads it: its state within the limit of
/// size, read again, and reading again to the same changes and deletions at the latest
/// the second time ([`same_state`]).
///
/// What the library took it may write in a form it does not read: the walk keeps out
/// what is known of that, and this is for what is not. Gives the size of its state
/// with it.
///
/// # Errors
///
/// Returns [`UpdateError::DocumentTooLarge`] past [`UpdateLimits::state_bytes`], and
/// [`UpdateError::LeavesUnreadable`] for a state that does not read again to itself.
pub(crate) fn settled_sized(doc: &Doc, limits: &UpdateLimits) -> Result<(Doc, usize), UpdateError> {
    let state = state_of(doc);
    if state.len() > limits.state_bytes {
        return Err(UpdateError::DocumentTooLarge {
            size: state.len(),
            limit: limits.state_bytes,
        });
    }
    let again = doc_of_state(&state).map_err(UpdateError::LeavesUnreadable)?;
    let state_again = state_of(&again);
    if same_state(&state_again, &state) {
        return Ok((again, state_again.len()));
    }
    // A first reading may write the same document otherwise (neighbouring changes it
    // joins); a second must not.
    let settled = doc_of_state(&state_again).map_err(UpdateError::LeavesUnreadable)?;
    let state_settled = state_of(&settled);
    if !same_state(&state_settled, &state_again) {
        return Err(UpdateError::LeavesUnreadable(
            "its state reads to a document whose state reads to another".to_owned(),
        ));
    }
    Ok((settled, state_settled.len()))
}

/// Whether two states are the same as far as can be told the same way every time: the
/// same bytes, or the same changes by their identifiers and the same deletions, in any
/// order (the library writes each client's deletions in the order of a table). Not
/// their content: with changes no editor makes (origins that do not lie where the
/// change does), the library reads one state to documents that differ from one reading
/// to the next, so that comparing content would refuse such an update only now and
/// then. Refusing those origins in the walk is what would close that.
fn same_state(one: &[u8], other: &[u8]) -> bool {
    one == other
        || matches!(
            (Update::decode_v1(one), Update::decode_v1(other)),
            (Ok(one), Ok(other)) if one == other
        )
}

#[cfg(test)]
mod tests {
    use yrs::{Any, Number, Xml, XmlElementPrelim, XmlFragment};

    use super::*;
    use crate::build::new_doc;

    const LIMITS: UpdateLimits = UpdateLimits {
        update_bytes: 1024,
        state_bytes: 1024,
        body_bytes: 1024,
        placing_steps: Some(PLACING_STEPS),
        clients: Some(MAX_CLIENTS),
    };

    /// A document that holds what the library writes in a form the walk refuses: a
    /// whole number past 53 bits, put in here instead of by an update, as a part of the
    /// library that the walk does not know of could.
    fn unreadable() -> Doc {
        let doc = new_doc();
        let root = doc.get_or_insert_xml_fragment(FIELD);
        let mut txn = doc.transact_mut();
        let element = root.push_back(&mut txn, XmlElementPrelim::empty("paragraph"));
        element.insert_attribute(&mut txn, "n", Any::Number(Number::Int((1 << 53) + 1)));
        drop(txn);
        doc
    }

    #[test]
    fn a_document_that_cannot_be_read_again_is_refused_and_does_not_panic() {
        let doc = unreadable();
        assert!(
            doc_of_state(&state_of(&doc)).is_err(),
            "its state is not read"
        );
        assert!(
            matches!(copy_of(&doc), Err(UpdateError::DocumentUnreadable(_))),
            "no copy"
        );
        let empty = [0, 0];
        assert!(
            matches!(
                with_update(&doc, &empty, &LIMITS),
                Err(UpdateError::DocumentUnreadable(_))
            ),
            "no update is taken for it"
        );
    }

    #[test]
    fn a_state_is_read_again_only_whole() {
        let doc = new_doc();
        assert!(doc_of_state(&state_of(&doc)).is_ok(), "the empty document");
        // A change of client 5 that begins at clock 3: the changes before it are missing.
        let mut later = vec![1, 1, 5, 3, 4, 1, 7];
        later.extend_from_slice(b"default");
        later.extend_from_slice(&[1, b'a', 0]);
        assert!(doc_of_state(&later).is_err(), "a state with a hole");
    }
}
