//! A body's Y document behind a type of this crate, for what holds one without naming
//! the Y library: the server's worker process.

use fvoci_model::{Body, BodyError};
use yrs::updates::decoder::Decode;
use yrs::updates::encoder::Encode;
use yrs::{Doc, ReadTxn, StateVector, Transact};

use crate::build::{from_body, new_doc};
use crate::project::to_body;
use crate::repair::Repair;
use crate::settle::{rebuild, settle_keeping};
use crate::take::{TakeError, TakeInPlaceError, from_state, take_update, take_update_in_place};
use crate::update::{UpdateError, UpdateLimits, state_of};

/// The Y document of a body. It is made from a stored body or state; callers either
/// take updates on a copy ([`BodyDoc::take`]) or consume it in place
/// ([`BodyDoc::take_in_place`]) and rebuild from their checkpoint ([`BodyDoc::rebuild`])
/// if an update is refused after modifying it, so no caller ever observes a partially
/// mutated document.
#[derive(Debug)]
pub struct BodyDoc {
    doc: Doc,
}

/// A document that an update, or the reading of a stored state, left.
#[derive(Debug)]
#[non_exhaustive]
pub struct Changed {
    /// The new document.
    pub doc: BodyDoc,
    /// Its body, and what a repair changed to make it one ([`Repair`]).
    pub repair: Repair,
    /// Its whole state ([`BodyDoc::state`]), already encoded.
    pub state: Vec<u8>,
    /// The size of its state ([`BodyDoc::state`]), in bytes.
    pub state_bytes: usize,
    /// How many changes it holds in total.
    pub changes: u64,
    /// The most changes one of its clients has, deleted ones too
    /// ([`crate::MAX_CLIENT_CHANGES`]).
    pub client_changes: u64,
    /// The steps the Y library takes to read it from its state
    /// ([`crate::READING_STEPS`]); 0 where the limits had no count of them.
    pub reading_steps: u64,
    /// The most steps the library may take to read it once one more client has
    /// written into it ([`steps_ahead`]); 0 where the limits had no count.
    pub steps_ahead: u64,
    /// Whether `doc` must be made again from its body ([`BodyDoc::from_body`]) before
    /// its state is stored or another update is taken ([`crate::Taken::remake`]).
    pub remake: bool,
}

/// Why [`BodyDoc::take_in_place`] refused an update.
#[derive(Debug, thiserror::Error)]
pub enum BodyTakeError {
    /// Refused before the document was mutated; the untouched `doc` is returned.
    #[error("{error}")]
    Untouched {
        /// The unchanged document.
        doc: BodyDoc,
        /// Why the update was refused.
        #[source]
        error: TakeError,
    },
    /// Refused during or after mutating the document; the document was dropped and the
    /// caller must rebuild from its checkpoint with [`BodyDoc::rebuild`].
    #[error(transparent)]
    Dirty(#[from] TakeError),
}

/// A document verified to read back deterministically to the same state
/// ([`BodyDoc::settle`]), with its canonical state bytes and counts.
#[derive(Debug)]
#[non_exhaustive]
pub struct SettledDoc {
    /// The settled document.
    pub doc: BodyDoc,
    /// Its canonical whole state ([`BodyDoc::state`]), verified to round-trip via
    /// [`crate::same_state`].
    pub state: Vec<u8>,
    /// The size of `state`, in bytes.
    pub state_bytes: usize,
    /// How many changes it holds in total.
    pub changes: u64,
    /// The most changes one of its clients has, deleted ones too.
    pub client_changes: u64,
    /// The steps the Y library takes to read `state`.
    pub reading_steps: u64,
    /// The most steps the library may take once one more client has written into it.
    pub steps_ahead: u64,
}

/// Why [`BodyDoc::settle`] could not settle a document, returning the intact live
/// document so the caller can project its body and remake it without losing edits.
#[derive(Debug, thiserror::Error)]
#[error("{error}")]
pub struct SettleError {
    /// The intact live document whose body still holds every accepted edit.
    pub doc: BodyDoc,
    /// Why settling failed.
    #[source]
    pub error: UpdateError,
}

/// Counts read from a document's state in one pass ([`state_counts`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StateCounts {
    /// How many changes the state holds in total.
    pub changes: u64,
    /// The most changes one client has in the state, deleted ones too.
    pub client_changes: u64,
    /// The most steps the Y library may take to read it once one more client has
    /// written into it ([`steps_ahead`]).
    pub steps_ahead: u64,
}

impl Default for BodyDoc {
    fn default() -> Self {
        Self::new()
    }
}

impl BodyDoc {
    /// The document of a card without a body ([`crate::new_doc`]).
    #[must_use]
    pub fn new() -> Self {
        Self { doc: new_doc() }
    }

    /// The document of a stored body, written by a writer taken at random
    /// ([`crate::from_body`]); the document of no body for `None`.
    #[must_use]
    pub fn from_body(body: Option<&Body>) -> Self {
        body.map_or_else(Self::new, |body| Self {
            doc: from_body(body),
        })
    }

    /// Opens a stored state ([`crate::from_state`]).
    ///
    /// # Errors
    ///
    /// As [`crate::from_state`].
    pub fn from_state(state: &[u8], limits: &UpdateLimits) -> Result<Changed, TakeError> {
        from_state(state, limits).map(Changed::from)
    }

    /// The document with a client's update ([`crate::take_update`]). This one is as it
    /// was.
    ///
    /// # Errors
    ///
    /// As [`crate::take_update`].
    pub fn take(&self, update: &[u8], limits: &UpdateLimits) -> Result<Changed, TakeError> {
        take_update(&self.doc, update, limits).map(Changed::from)
    }

    /// Takes a client's update into `self` in place, using `prior_state` and
    /// `prior_changes` to charge placing steps without copying `self`.
    ///
    /// `prior_state` and `prior_changes` must match `self` (as returned by opening via
    /// [`BodyDoc::from_state`], [`BodyDoc::from_body`] + [`state_counts`], or the
    /// preceding [`Changed`]); passing a stale or underestimated `prior_changes` would
    /// undercharge `placing_steps`.
    ///
    /// # Errors
    ///
    /// Returns [`BodyTakeError::Untouched`] with `self` intact if the update was
    /// refused before mutation, or [`BodyTakeError::Dirty`] if the update was refused
    /// during or after mutation (callers must rebuild with [`BodyDoc::rebuild`]).
    pub fn take_in_place(
        self,
        prior_state: &[u8],
        prior_changes: u64,
        update: &[u8],
        limits: &UpdateLimits,
    ) -> Result<Changed, BodyTakeError> {
        match take_update_in_place(self.doc, prior_state, prior_changes, update, limits) {
            Ok(taken) => Ok(Changed::from(taken)),
            Err(TakeInPlaceError::Untouched(doc, error)) => Err(BodyTakeError::Untouched {
                doc: Self { doc },
                error,
            }),
            Err(TakeInPlaceError::Dirty(error)) => Err(BodyTakeError::Dirty(error)),
        }
    }

    /// Rebuilds a document from a checkpoint `base` state (as returned by
    /// [`BodyDoc::from_state`], [`BodyDoc::from_body`], or [`Changed::state`]) and the
    /// accepted updates and repairs `since` it.
    ///
    /// When `since` is empty, `base` is decoded directly without re-settling.
    ///
    /// # Errors
    ///
    /// Returns [`UpdateError`] if `base` or any update in `since` cannot be read or
    /// settled.
    pub fn rebuild<U: AsRef<[u8]>>(base: &[u8], since: &[U]) -> Result<Self, UpdateError> {
        rebuild(base, since).map(|doc| Self { doc })
    }

    /// Re-reads `self`'s encoded `state` and verifies it settles to the same state
    /// ([`crate::same_state`]) within `limits`, run at fold and close instead of on
    /// every live update.
    ///
    /// `state` must be the current encoded state of `self` (as returned by
    /// [`BodyDoc::state`] or the preceding [`Changed::state`]); it is taken by value so
    /// the buffer can be freed before any second settle pass.
    ///
    /// On any error (non-deterministic state, limit exceeded, or unreadable state), the
    /// original intact `self` comes back inside [`SettleError::doc`] so callers can
    /// project its body and remake a fresh 0-step document without losing edits.
    ///
    /// # Errors
    ///
    /// Returns [`SettleError`] with `self` intact if `state` cannot be settled within
    /// `limits`.
    pub fn settle(self, state: Vec<u8>, limits: &UpdateLimits) -> Result<SettledDoc, SettleError> {
        match settle_keeping(self.doc, state, limits) {
            Ok(settled) => {
                let state_bytes = settled.state.len();
                Ok(SettledDoc {
                    doc: Self { doc: settled.doc },
                    state: settled.state,
                    state_bytes,
                    changes: settled.changes,
                    client_changes: settled.client_changes,
                    reading_steps: settled.reading_steps,
                    steps_ahead: settled.steps_ahead,
                })
            }
            Err((doc, error)) => Err(SettleError {
                doc: Self { doc },
                error,
            }),
        }
    }

    /// The whole document as a version 1 update, as it is stored ([`crate::state_of`]).
    #[must_use]
    pub fn state(&self) -> Vec<u8> {
        state_of(&self.doc)
    }

    /// How many clients the document names: each that wrote to it.
    #[must_use]
    pub fn clients(&self) -> usize {
        self.doc.transact().state_vector().len()
    }

    /// The state vector of the document, version 1: what it has of each client.
    #[must_use]
    pub fn state_vector(&self) -> Vec<u8> {
        self.doc.transact().state_vector().encode_v1()
    }

    /// What the document has that a holder of `state_vector` has not, as a version 1
    /// update.
    ///
    /// # Errors
    ///
    /// Returns [`UpdateError::Unreadable`] if the bytes are no state vector.
    pub fn diff(&self, state_vector: &[u8]) -> Result<Vec<u8>, UpdateError> {
        let vector = read_state_vector(state_vector)?;
        Ok(self.doc.transact().encode_diff_v1(&vector))
    }

    /// The body of the document, as an editor would store it.
    ///
    /// # Errors
    ///
    /// Returns why the document is no body, which nothing this type holds is.
    pub fn body(&self) -> Result<Option<Body>, BodyError> {
        to_body(&self.doc)
    }
}

impl From<crate::Taken> for Changed {
    fn from(taken: crate::Taken) -> Self {
        Self {
            doc: BodyDoc { doc: taken.doc },
            repair: taken.repair,
            state: taken.state,
            state_bytes: taken.state_bytes,
            changes: taken.changes,
            client_changes: taken.client_changes,
            reading_steps: taken.reading_steps,
            steps_ahead: taken.steps_ahead,
            remake: taken.remake,
        }
    }
}

/// Counts total changes, max changes per client, and [`steps_ahead`] of a document's
/// `state` in a single pass.
///
/// # Errors
///
/// Returns [`UpdateError::Unreadable`] if the bytes are no whole state the crate reads.
pub fn state_counts(state: &[u8]) -> Result<StateCounts, UpdateError> {
    crate::reading::count(state, crate::READING_STEPS)
        .map(|read| StateCounts {
            changes: read.walked.changes,
            client_changes: read.walked.most,
            steps_ahead: read.ahead,
        })
        .map_err(|refusal| UpdateError::Unreadable(refusal.reason().to_owned()))
}

/// The most changes one client has in a document's `state`, deleted ones too
/// ([`crate::MAX_CLIENT_CHANGES`]): for a document that was not taken
/// ([`BodyDoc::from_body`]), as [`Changed::client_changes`] is for one that was.
///
/// # Errors
///
/// Returns [`UpdateError::Unreadable`] if the bytes are no state the crate reads.
pub fn client_changes(state: &[u8]) -> Result<u64, UpdateError> {
    crate::scan::check(state)
        .map(|walked| walked.most)
        .map_err(|refusal| UpdateError::Unreadable(refusal.reason().to_owned()))
}

/// The steps the Y library takes to read a document's `state` ([`crate::READING_STEPS`]),
/// counted no further than a little past `stop_past`: for a document that was not
/// taken, as [`Changed::reading_steps`] is for one that was.
///
/// # Errors
///
/// Returns [`UpdateError::Unreadable`] if the bytes are no whole state the crate reads.
pub fn reading_steps(state: &[u8], stop_past: u64) -> Result<u64, UpdateError> {
    crate::reading::count(state, stop_past)
        .map(|read| read.steps)
        .map_err(|refusal| UpdateError::Unreadable(refusal.reason().to_owned()))
}

/// The most steps the Y library may take to read the changes a document has now, once
/// one more client is read before the others: the steps its `state` takes to read
/// ([`reading_steps`]), with every change of it looked at once for every client it
/// names while changes wait for another client's. A document that thousands of
/// sessions wrote one after the other is read in few steps, or in none, until a
/// session whose client is read before the others writes after the last of them:
/// the update that brings it past [`crate::READING_STEPS`] forces an immediate remake
/// from its body ([`Changed::remake`]), though
/// nothing in it was made to be slow. What holds a document makes it again from its body
/// while this is far from that limit, before such an update comes.
///
/// It is not the most a later update may leave: each change an update adds, and each
/// change of the document it cuts in two by writing or deleting inside it, is looked
/// at for every client as well, a step for every 128 clients. An update of tens of
/// thousands of those into a document of thousands of clients can still pass the
/// limit, and is taken (and repaired in place if needed) with [`Changed::remake`] set
/// so the document is made again from its body without losing it.
/// Counted no further than a little past [`crate::READING_STEPS`].
///
/// # Errors
///
/// Returns [`UpdateError::Unreadable`] if the bytes are no whole state the crate reads.
pub fn steps_ahead(state: &[u8]) -> Result<u64, UpdateError> {
    crate::reading::count(state, crate::READING_STEPS)
        .map(|read| read.ahead)
        .map_err(|refusal| UpdateError::Unreadable(refusal.reason().to_owned()))
}

/// Reads a state vector from anybody. The library reserves room for as many entries as
/// the first number says, before it reads them; that number is held to what the bytes
/// can hold (two bytes an entry at the least) first.
fn read_state_vector(bytes: &[u8]) -> Result<StateVector, UpdateError> {
    let unreadable = || UpdateError::Unreadable("the bytes are no state vector".to_owned());
    let (count, read) = var_u64(bytes).ok_or_else(unreadable)?;
    let room = bytes.len().saturating_sub(read) / 2;
    if usize::try_from(count).map_or(true, |count| count > room) {
        return Err(unreadable());
    }
    let vector = StateVector::decode_v1(bytes).map_err(|_| unreadable())?;
    Ok(vector)
}

/// A variable-length number at the start of `bytes` (seven bits a byte, the lowest
/// first), with how many bytes it took; `None` if it does not end or does not fit.
fn var_u64(bytes: &[u8]) -> Option<(u64, usize)> {
    let mut value = 0_u64;
    for (index, byte) in bytes.iter().enumerate().take(10) {
        let shift = u32::try_from(index * 7).ok()?;
        value |= u64::from(byte & 0x7f).checked_shl(shift)?;
        if byte & 0x80 == 0 {
            return Some((value, index + 1));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_state_vector_that_claims_more_entries_than_it_has_is_refused() {
        // 2^32 - 1 entries, in five bytes, and none of them.
        let claim = [0xff, 0xff, 0xff, 0xff, 0x0f];
        assert!(matches!(
            BodyDoc::new().diff(&claim),
            Err(UpdateError::Unreadable(_))
        ));
        assert!(
            BodyDoc::new().diff(&[0x80]).is_err(),
            "a number that does not end"
        );
        assert!(BodyDoc::new().diff(&[]).is_err(), "nothing");
    }

    #[test]
    fn the_diff_against_an_empty_state_vector_is_the_whole_document() {
        let doc = BodyDoc::new();
        let empty = doc.state_vector();
        assert_eq!(doc.diff(&empty).expect("a diff"), doc.state());
    }
}
