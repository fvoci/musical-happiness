//! Taking an update from a client.

use thiserror::Error;
use yrs::updates::decoder::Decode;
use yrs::{ClientID, Doc, Out, ReadTxn, StateVector, Transact, Update};

use crate::FIELD;
use crate::build::doc_of_client;
use crate::scan;

/// The sizes an update may have and leave.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UpdateLimits {
    /// The largest update, in bytes as it arrives.
    pub update_bytes: usize,
    /// The largest Y document an update may leave, in bytes as it is stored (a version
    /// 1 update of the whole document). A Y document keeps a trace of what was deleted,
    /// so it grows with every edit even where the body does not.
    pub state_bytes: usize,
}

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
pub fn copy_of(doc: &Doc) -> Result<Doc, UpdateError> {
    doc_of_state(&state_of(doc)).map_err(UpdateError::DocumentUnreadable)
}

/// The Y document whose state is `state`, read as any update is read: past the walk
/// (`scan`), with every change it builds on, and with nothing left waiting.
///
/// # Errors
///
/// Returns why the state is no whole document.
fn doc_of_state(state: &[u8]) -> Result<Doc, String> {
    let begins = scan::check(state).map_err(|refusal| refusal.reason().to_owned())?;
    if begins.iter().any(|(_, clock)| *clock > 0) {
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
    Ok(doc)
}

/// Reads the bytes of an update for a document, after the walk that keeps from the
/// library what it does not survive (`scan`).
fn read(doc: &Doc, update: &[u8]) -> Result<Update, UpdateError> {
    let begins = scan::check(update)
        .map_err(|refusal| UpdateError::Unreadable(refusal.reason().to_owned()))?;
    // A client's changes go on from where the document has them: the library would
    // take changes that begin later and leave a hole before them.
    let known = doc.transact().state_vector();
    if begins
        .into_iter()
        .any(|(client, clock)| clock > u64::from(known.get(&ClientID::new(client))))
    {
        return Err(UpdateError::MissingChanges);
    }
    Update::decode_v1(update).map_err(|error| UpdateError::Unreadable(error.to_string()))
}

/// The Y document as it would be with a client's update: a copy of `doc` with the
/// update applied, read again from its state. `doc` itself is not changed.
///
/// What comes back has still to be projected ([`crate::to_body`]) and, where that
/// fails, repaired ([`crate::repair`]): this only refuses what cannot be taken at all.
///
/// Not everything an update may write is read by the projection: members of the root,
/// attributes the schema does not have, attributes of a text. No editor reads them
/// either. They stay in the document, and [`UpdateLimits::state_bytes`] is what bounds
/// them.
///
/// How long this takes is not bounded by the size of the update alone. The library
/// places a change among the others that claim the same place by looking at each of
/// them, so an update made for it can take the square of its size: 112 kB of changes
/// that all claim the end of a parent of 16,000 children took 29 seconds. What calls
/// this for a client keeps [`UpdateLimits::update_bytes`] small and does not hold
/// anything others wait for while it runs.
///
/// # Errors
///
/// Returns an [`UpdateError`] if the update is too large, is no update, cannot be
/// applied, leaves a part of itself waiting for changes the document does not have,
/// writes to another root than the body's, leaves the document too large or in a
/// state that cannot be read again, or if `doc` itself cannot be read again.
pub fn with_update(doc: &Doc, update: &[u8], limits: &UpdateLimits) -> Result<Doc, UpdateError> {
    if update.len() > limits.update_bytes {
        return Err(UpdateError::TooLarge {
            size: update.len(),
            limit: limits.update_bytes,
        });
    }
    let update = read(doc, update)?;
    let copy = copy_of(doc)?;
    {
        let mut txn = copy.transact_mut();
        txn.apply_update(update)
            .map_err(|error| UpdateError::Unappliable(error.to_string()))?;
    }
    let txn = copy.transact();
    if txn.has_missing_updates() {
        return Err(UpdateError::MissingChanges);
    }
    for (name, root) in txn.root_refs() {
        if name != FIELD || !matches!(root, Out::YXmlFragment(_)) {
            return Err(UpdateError::UnexpectedRoot(name.to_owned()));
        }
    }
    let state = txn.encode_state_as_update_v1(&StateVector::default());
    if state.len() > limits.state_bytes {
        return Err(UpdateError::DocumentTooLarge {
            size: state.len(),
            limit: limits.state_bytes,
        });
    }
    // What the library took it may write in a form it does not read: the walk keeps
    // out what is known of that, and this is for what is not. The document given back
    // is the one read from the state, which is what a server that stored it would read.
    doc_of_state(&state).map_err(UpdateError::LeavesUnreadable)
}

#[cfg(test)]
mod tests {
    use yrs::{Any, Number, Xml, XmlElementPrelim, XmlFragment};

    use super::*;
    use crate::build::new_doc;

    const LIMITS: UpdateLimits = UpdateLimits {
        update_bytes: 1024,
        state_bytes: 1024,
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
