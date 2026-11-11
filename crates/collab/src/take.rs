//! What the server does with an update from a client, or with a stored state, in the
//! one order that keeps its document whole.

use fvoci_model::{Body, BodyError};
use thiserror::Error;
use yrs::Doc;

use crate::build::new_doc;
use crate::project::to_body;
use crate::repair::{Repair, repaired};
use crate::update::{UpdateError, UpdateLimits, settled, with_update};

/// A document that took an update, with its body.
#[derive(Debug)]
#[non_exhaustive]
pub struct Taken {
    /// The document with the update, and with what a repair deleted. It shares nothing
    /// with the document the update was given for, which is as it was.
    pub doc: Doc,
    /// The body of `doc`, and what a repair deleted to make it one: the deletions, as
    /// an update, are for the editors that hold the document, which did not make them.
    pub repair: Repair,
}

/// Why an update, or a stored state, is not taken. The document it was given for is as
/// it was.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum TakeError {
    /// The update cannot be taken at all.
    #[error(transparent)]
    Update(#[from] UpdateError),
    /// With the update the document is no body, and no repair makes it one.
    #[error(transparent)]
    Body(#[from] BodyError),
    /// With the update the body would be larger than [`UpdateLimits::body_bytes`].
    #[error("with the update the body would be {size} bytes, more than the {limit} allowed")]
    BodyTooLarge {
        /// The size the body would have, in bytes of its stored JSON.
        size: usize,
        /// The limit.
        limit: usize,
    },
}

/// Takes a client's update into a document, as the server does with every one: the
/// update is applied to a copy, the copy is read again from its state, projected to a
/// body, and repaired where that fails; only a copy that is a body within the limits
/// comes back. `doc` itself is never changed.
///
/// The deletions of a repair ([`Repair::update`]) go to every editor that holds the
/// document, the one that sent the update too: they did not make them.
///
/// How long this takes grows with the document as well as with the update: the copy
/// and the reading again each read the whole state (17 ms for 20,000 nodes, release
/// build), and the walk refuses the known ways to make that reading cost the square of
/// the state (a member written over and over). And the Y library places a change among
/// the others that claim the same place by looking at each of them, so that an update
/// made for it can take the square of its size (112 kB of changes that all claim the
/// end of a parent of 16,000 children took 29 seconds). What calls this keeps
/// [`UpdateLimits::update_bytes`] and [`UpdateLimits::state_bytes`] small and does not
/// hold anything others wait for while it runs.
///
/// # Errors
///
/// Returns [`TakeError::Update`] if the update cannot be taken at all (see
/// [`UpdateError`]), [`TakeError::Body`] if the document with it is no body and a
/// repair does not make it one, and [`TakeError::BodyTooLarge`] if its body is past
/// [`UpdateLimits::body_bytes`].
///
/// A document keeps what was deleted from it, so it grows with every edit: typing and
/// deleting 10 kB a hundred times leaves a body of two letters in a state of 1 MB. Once
/// it is near [`UpdateLimits::state_bytes`], every update is refused with
/// [`UpdateError::DocumentTooLarge`], deletions too. What holds the document then makes
/// it again from its body ([`crate::from_body`]), which starts without what was
/// deleted, and has the editors that hold the old one load the new one: an update made
/// against the old document is refused by the new one
/// ([`UpdateError::MissingChanges`]).
pub fn take_update(doc: &Doc, update: &[u8], limits: &UpdateLimits) -> Result<Taken, TakeError> {
    let with = with_update(doc, update, limits)?;
    to_taken(with, limits)
}

/// Opens the document of a stored state ([`crate::state_of`]), as an update to an
/// empty document is taken: with the same checks and repair, and the update's own
/// limit of size left out, as a state may be as large as
/// [`UpdateLimits::state_bytes`].
///
/// # Errors
///
/// As [`take_update`].
pub fn from_state(state: &[u8], limits: &UpdateLimits) -> Result<Taken, TakeError> {
    if state.len() > limits.state_bytes {
        return Err(UpdateError::DocumentTooLarge {
            size: state.len(),
            limit: limits.state_bytes,
        }
        .into());
    }
    let limits = UpdateLimits {
        update_bytes: state.len(),
        ..*limits
    };
    take_update(&new_doc(), state, &limits)
}

/// The document an update left, as a body: as it is, or repaired.
///
/// A repaired copy is held to the limit of size and read again from its state, as the
/// document an update leaves is ([`with_update`]): with changes no editor makes (origins that do not lie where the
/// change does), the library can place letters otherwise when it reads a whole state
/// than when it deletes in a document it holds. What is given back is what a server
/// that stored it reads, with the body of that; if that is no body, the update is
/// refused.
fn to_taken(with: Doc, limits: &UpdateLimits) -> Result<Taken, TakeError> {
    let (mut repair, repaired) = repaired(&with)?;
    let doc = match repaired {
        None => with,
        Some(copy) => {
            let again = settled(&copy, limits)?;
            repair.body = to_body(&again)?;
            again
        }
    };
    if let Some(body) = &repair.body {
        let size = size_of(body);
        if size > limits.body_bytes {
            return Err(TakeError::BodyTooLarge {
                size,
                limit: limits.body_bytes,
            });
        }
    }
    Ok(Taken { doc, repair })
}

/// The size of a body as JSON without whitespace, as its limit is measured.
fn size_of(body: &Body) -> usize {
    /// Counts what is written to it.
    struct Count(usize);

    impl std::io::Write for Count {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0 = self.0.saturating_add(bytes.len());
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    let mut count = Count(0);
    // Writing JSON to a counter does not fail: a `Value` is always JSON.
    let _ = serde_json::to_writer(&mut count, body.document());
    count.0
}
