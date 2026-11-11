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
/// # Panics
///
/// Panics if the document's own state cannot be applied to an empty document, which
/// would be a defect of the Y library.
#[must_use]
pub fn copy_of(doc: &Doc) -> Doc {
    let copy = doc_of_client(None);
    let state = Update::decode_v1(&state_of(doc)).expect("a document's own state is an update");
    copy.transact_mut()
        .apply_update(state)
        .expect("a document's own state applies to an empty document");
    copy
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
/// update applied. `doc` itself is not changed.
///
/// What comes back has still to be projected ([`crate::to_body`]) and, where that
/// fails, repaired ([`crate::repair`]): this only refuses what cannot be taken at all.
///
/// Not everything an update may write is read by the projection: members of the root,
/// attributes the schema does not have, attributes of a text. No editor reads them
/// either. They stay in the document, and [`UpdateLimits::state_bytes`] is what bounds
/// them.
///
/// # Errors
///
/// Returns an [`UpdateError`] if the update is too large, is no update, cannot be
/// applied, leaves a part of itself waiting for changes the document does not have,
/// writes to another root than the body's, or leaves the document too large.
pub fn with_update(doc: &Doc, update: &[u8], limits: &UpdateLimits) -> Result<Doc, UpdateError> {
    if update.len() > limits.update_bytes {
        return Err(UpdateError::TooLarge {
            size: update.len(),
            limit: limits.update_bytes,
        });
    }
    let update = read(doc, update)?;
    let copy = copy_of(doc);
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
    let size = txn.encode_state_as_update_v1(&StateVector::default()).len();
    if size > limits.state_bytes {
        return Err(UpdateError::DocumentTooLarge {
            size,
            limit: limits.state_bytes,
        });
    }
    drop(txn);
    Ok(copy)
}
