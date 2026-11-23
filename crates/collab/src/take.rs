//! What the server does with an update from a client, or with a stored state, in the
//! one order that keeps its document whole.

use fvoci_model::{Body, BodyError};
use thiserror::Error;
use yrs::Doc;

use crate::build::new_doc;
use crate::project::to_body;
use crate::repair::{Repair, repaired};
use crate::update::{
    Outcome, Settled, UpdateError, UpdateLimits, settled_sized, with_update_sized,
    within_reading_steps,
};

/// A document that took an update, with its body.
#[derive(Debug)]
#[non_exhaustive]
pub struct Taken {
    /// The document with the update, and with what a repair changed. It shares nothing
    /// with the document the update was given for, which is as it was.
    pub doc: Doc,
    /// The body of `doc`, and what a repair changed to make it one: the changes, as
    /// an update, are for the editors that hold the document, which did not make them.
    pub repair: Repair,
    /// The size of the state of `doc` ([`crate::state_of`]), in bytes.
    pub state_bytes: usize,
    /// The most changes one client has in `doc`, deleted ones too
    /// ([`UpdateLimits::client_changes`]).
    pub client_changes: u64,
    /// The steps the Y library takes to read `doc` from its state
    /// ([`UpdateLimits::reading_steps`]); 0 where the limits have no count of them.
    pub reading_steps: u64,
    /// The most steps the library may take to read `doc` once one more client has
    /// written into it ([`crate::steps_ahead`]); 0 where the limits have no count.
    pub steps_ahead: u64,
    /// Whether `doc` must be made again from its body ([`crate::from_body`]) before
    /// its state is stored or another update is taken: reading its state would take
    /// more steps than [`UpdateLimits::reading_steps`], so `doc` was not read back
    /// from its state.
    pub remake: bool,
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
/// and the reading again each read the whole state (a paragraph typed into 50,000
/// nodes is taken in 0.15 s, and a repair there in up to 1.3 s, release build), and
/// the walk refuses the known ways to make that reading cost the square of the state
/// (a member written over and over). And the Y library places a change among the
/// others of its list by walking past them, so that an update made for it has every
/// change walk a long list (59 kB of changes that each named the root of 50,000
/// children took 12 seconds): [`UpdateLimits::placing_steps`] is what an update may
/// be made to walk. The library walks so again every time it reads the document from
/// its state, here and as the document is opened, so that updates within that limit
/// added up to a document no worker opened in its time:
/// [`UpdateLimits::reading_steps`] is what the document an update leaves may take to
/// read, counted from its state before the library reads it. Not every way to make
/// the library slow may be counted: what calls this also gives it a time
/// to take an update in, and holds back the sender of one that takes longer. It keeps
/// [`UpdateLimits::update_bytes`] and [`UpdateLimits::state_bytes`] small and does not
/// hold anything others wait for while it runs.
///
/// # Errors
///
/// Returns [`TakeError::Update`] if the update cannot be taken at all (see
/// [`UpdateError`]: among them one whose changes would take more steps to place than
/// [`UpdateLimits::placing_steps`], and one that leaves a document that takes more
/// steps to read than [`UpdateLimits::reading_steps`]), [`TakeError::Body`] if the
/// document with it is no body and a
/// repair does not make it one, and [`TakeError::BodyTooLarge`] if its body is past
/// [`UpdateLimits::body_bytes`].
///
/// A document keeps what was deleted from it, so it grows with every edit: typing and
/// deleting 10 kB a hundred times leaves a body of two letters in a state of 1 MB. Once
/// it is near [`UpdateLimits::state_bytes`], every update is refused with
/// [`UpdateError::DocumentTooLarge`], deletions too. Changes that make the state slow
/// to read stay in it as well: once an update would take the document past
/// [`UpdateLimits::reading_steps`], if the copy with the update is already a valid
/// body (needs no repair), it is still returned with [`Taken::remake`] set so what
/// holds the document writes its body and makes it again from that ([`crate::from_body`])
/// without losing the update; only an update that also needs a repair is refused with
/// [`UpdateError::TooManyReadingSteps`]. [`Taken::steps_ahead`] tells what holds the
/// document how near it may be after one more client wrote, so that it is made again
/// beforehand; making it again starts without what was deleted and in no reading steps,
/// and has the editors that hold the old one load the new one: an update made against
/// the old document is refused by the new one ([`UpdateError::MissingChanges`]).
pub fn take_update(doc: &Doc, update: &[u8], limits: &UpdateLimits) -> Result<Taken, TakeError> {
    let with = with_update_sized(doc, update, limits)?;
    to_taken(with, limits)
}

/// Opens the document of a stored state ([`crate::state_of`]), as an update to an
/// empty document is taken: with the same checks and repair, and the update's own
/// limits left out, as a state may be as large as [`UpdateLimits::state_bytes`] and
/// its changes were placed when they were taken. The steps its reading takes are
/// counted first ([`UpdateLimits::reading_steps`]): the library does not read a state
/// past them.
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
    within_reading_steps(state, limits)?;
    let limits = UpdateLimits {
        update_bytes: state.len(),
        placing_steps: None,
        clients: None,
        client_changes: None,
        ..*limits
    };
    let taken = take_update(&new_doc(), state, &limits)?;
    if taken.remake {
        return Err(UpdateError::TooManyReadingSteps {
            steps: taken.reading_steps,
            limit: limits.reading_steps.unwrap_or_default(),
        }
        .into());
    }
    Ok(taken)
}

/// The document an update left, as a body: as it is, or repaired.
///
/// A repaired copy is held to the limit of size and read again from its state, as the
/// document an update leaves is ([`with_update`]): with changes no editor makes (origins that do not lie where the
/// change does), the library can place letters otherwise when it reads a whole state
/// than when it deletes in a document it holds. What is given back is what a server
/// that stored it reads, with the body of that; if that is no body, the update is
/// refused.
///
/// Where `with` was not settled because reading its state would exceed
/// [`UpdateLimits::reading_steps`] ([`Outcome::Remake`]), it is projected without
/// repairing: if it is already a body within [`UpdateLimits::body_bytes`] it comes
/// back with [`Taken::remake`] set; if it needs a repair it is refused with
/// [`UpdateError::TooManyReadingSteps`], because repair copies the document by reading
/// its state again.
fn to_taken(with: Settled, limits: &UpdateLimits) -> Result<Taken, TakeError> {
    if let Outcome::Remake { steps, limit } = with.outcome {
        let body = match to_body(&with.doc) {
            Ok(body) => body,
            Err(
                limit_err @ (BodyError::TooLarge
                | BodyError::TooDeep
                | BodyError::TooManyNodes
                | BodyError::TableTooLarge { .. }),
            ) => return Err(limit_err.into()),
            Err(_) => return Err(UpdateError::TooManyReadingSteps { steps, limit }.into()),
        };
        let repair = Repair {
            deleted: 0,
            mended: 0,
            update: None,
            body,
        };
        return finish_taken(with, repair, limits);
    }
    let (mut repair, repaired) = repaired(&with.doc)?;
    let settled = match repaired {
        None => with,
        Some(copy) => {
            let again = settled_sized(copy, limits)?;
            if let Outcome::Remake { steps, limit } = again.outcome {
                return Err(UpdateError::TooManyReadingSteps { steps, limit }.into());
            }
            repair.body = to_body(&again.doc)?;
            again
        }
    };
    finish_taken(settled, repair, limits)
}

/// Checks `repair.body` against [`UpdateLimits::body_bytes`] and builds [`Taken`].
fn finish_taken(
    settled: Settled,
    repair: Repair,
    limits: &UpdateLimits,
) -> Result<Taken, TakeError> {
    if let Some(body) = &repair.body {
        let size = size_of(body);
        if size > limits.body_bytes {
            return Err(TakeError::BodyTooLarge {
                size,
                limit: limits.body_bytes,
            });
        }
    }
    Ok(Taken {
        doc: settled.doc,
        repair,
        state_bytes: settled.state_bytes,
        client_changes: settled.client_changes,
        reading_steps: settled.reading_steps,
        steps_ahead: settled.steps_ahead,
        remake: matches!(settled.outcome, Outcome::Remake { .. }),
    })
}

/// The size of a body as JSON without whitespace, as its limit is measured.
pub(crate) fn size_of(body: &Body) -> usize {
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
