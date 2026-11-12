//! A body's Y document behind a type of this crate, for what holds one without naming
//! the Y library: the server's worker process.

use fvoci_model::{Body, BodyError};
use yrs::updates::decoder::Decode;
use yrs::updates::encoder::Encode;
use yrs::{Doc, ReadTxn, StateVector, Transact};

use crate::build::{from_body, new_doc};
use crate::project::to_body;
use crate::repair::Repair;
use crate::take::{TakeError, from_state, take_update};
use crate::update::{UpdateError, UpdateLimits, state_of};

/// The Y document of a body. It is made from a stored body or state and replaced by
/// the one an update leaves; it is never changed in place.
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
    /// Its body, and what a repair deleted to make it one ([`Repair`]).
    pub repair: Repair,
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

    /// The whole document as a version 1 update, as it is stored ([`crate::state_of`]).
    #[must_use]
    pub fn state(&self) -> Vec<u8> {
        state_of(&self.doc)
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
        }
    }
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
