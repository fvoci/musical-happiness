//! What the server does with an update from a client, or with a stored state, in the
//! one order that keeps its document whole.

use fvoci_model::{Body, BodyError};
use thiserror::Error;
use yrs::Doc;

use crate::build::new_doc;
use crate::repair::{Repair, repaired};
use crate::update::{UpdateError, UpdateLimits, with_update};

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
    /// With the update the document is no body, and no repair makes it one; or its body
    /// is larger than [`UpdateLimits::body_bytes`] ([`BodyError::TooLarge`]).
    #[error(transparent)]
    Body(#[from] BodyError),
}

/// Takes a client's update into a document, as the server does with every one: the
/// update is applied to a copy, the copy is read again from its state, projected to a
/// body, and repaired where that fails; only a copy that is a body within the limits
/// comes back. `doc` itself is never changed.
///
/// The deletions of a repair ([`Repair::update`]) go to every editor that holds the
/// document, the one that sent the update too: they did not make them.
///
/// How long this takes is not bounded by the size of the update alone: the Y library
/// places a change among the others that claim the same place by looking at each of
/// them, so that an update made for it can take the square of its size (112 kB of
/// changes that all claim the end of a parent of 16,000 children took 29 seconds). What
/// calls this keeps [`UpdateLimits::update_bytes`] small and does not hold anything
/// others wait for while it runs.
///
/// # Errors
///
/// Returns [`TakeError::Update`] if the update cannot be taken at all (see
/// [`UpdateError`]), and [`TakeError::Body`] if the document with it is no body and a
/// repair does not make it one, or its body is past [`UpdateLimits::body_bytes`].
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
    let limits = UpdateLimits {
        update_bytes: limits.state_bytes,
        ..*limits
    };
    take_update(&new_doc(), state, &limits)
}

/// The document an update left, as a body: as it is, or repaired.
fn to_taken(with: Doc, limits: &UpdateLimits) -> Result<Taken, TakeError> {
    let (repair, repaired) = repaired(&with)?;
    if let Some(body) = &repair.body
        && size_of(body) > limits.body_bytes
    {
        return Err(BodyError::TooLarge.into());
    }
    Ok(Taken {
        doc: repaired.unwrap_or(with),
        repair,
    })
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
