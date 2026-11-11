//! The body of a card as a Y document, for people to edit it together.
//!
//! A body is stored as JSON (`fvoci_model::Body`). While it is edited by several people
//! at once it is also a Y document, which their editors change through the binding of
//! the web app's editor (`@tiptap/y-tiptap`). This crate is what the server needs to
//! stand between the two, with no input or output of its own:
//!
//! - [`from_body`] makes the Y document of a stored body, in the shape the editors read
//!   ([`new_doc`] the one of a card without a body), and [`state_of`] gives a Y
//!   document as it is stored.
//! - [`take_update`] takes a client's update into a Y document, and [`from_state`]
//!   opens a stored one. Both give a document that is a body or nothing: the update is
//!   applied to a copy, which is read again from its state, projected back to the JSON
//!   an editor would store from it, checked as any body is checked, and repaired where
//!   that fails (two edits that are each valid can merge into a document that is not,
//!   and the editors delete what their schema refuses, as the repair does). The
//!   document an update is given for is never changed. The steps are not given on
//!   their own, so that none is left out or taken in another order.
//!
//! Every function here that is given a document takes its lock for as long as it
//! runs, and waits for it: do not call one while the same thread holds a transaction
//! on that document. The Y library is given on as [`yrs`], so that what holds a
//! document names its types from the version this crate is built with.
//!
//! What the shape is, and what an editor does with shared content, is not assumed here:
//! the web app's tests state it (`apps/web/spikes/collab`) and write it down as vectors
//! (`contracts/ydoc-vectors.json`), and this crate's tests read those.

mod build;
mod project;
mod read;
mod repair;
mod scan;
mod schema;
mod take;
mod update;

pub use build::{from_body, from_body_by, new_doc};
pub use repair::Repair;
pub use take::{TakeError, Taken, from_state, take_update};
pub use update::{UpdateError, UpdateLimits, state_of};
pub use yrs;

/// The steps of [`take_update`] one by one, for this crate's tests: what each does
/// alone is held to the vectors. Built with the feature `test-util` only.
#[cfg(feature = "test-util")]
#[doc(hidden)]
pub mod testing {
    use fvoci_model::{Body, BodyError};
    use yrs::updates::decoder::Decode;
    use yrs::{Doc, Transact, Update};

    use crate::{Repair, UpdateError, UpdateLimits};

    /// Projects a Y document to its body.
    ///
    /// # Errors
    ///
    /// Returns why the document is no body.
    pub fn to_body(doc: &Doc) -> Result<Option<Body>, BodyError> {
        crate::project::to_body(doc)
    }

    /// Repairs a Y document in place: the deletions of a repaired copy are made to it
    /// too, as an editor that holds the document makes them. A repair that fails
    /// leaves it as it was.
    ///
    /// # Errors
    ///
    /// Returns why the document cannot be repaired.
    ///
    /// # Panics
    ///
    /// Panics if the deletions of a copy do not apply to the document they were made
    /// from, which would be a defect.
    pub fn repair(doc: &Doc) -> Result<Repair, BodyError> {
        let (repair, _) = crate::repair::repaired(doc)?;
        if let Some(deletions) = &repair.update {
            let deletions = Update::decode_v1(deletions).expect("the deletions are an update");
            doc.transact_mut()
                .apply_update(deletions)
                .expect("the deletions apply to the document they were made from");
        }
        Ok(repair)
    }

    /// The Y document with an update, not yet projected or repaired.
    ///
    /// # Errors
    ///
    /// Returns why the update cannot be taken at all.
    pub fn with_update(
        doc: &Doc,
        update: &[u8],
        limits: &UpdateLimits,
    ) -> Result<Doc, UpdateError> {
        crate::update::with_update(doc, update, limits)
    }

    /// A copy of a Y document that shares nothing with it.
    ///
    /// # Errors
    ///
    /// Returns why the document cannot be read again from its state.
    pub fn copy_of(doc: &Doc) -> Result<Doc, UpdateError> {
        crate::update::copy_of(doc)
    }
}

/// The name of the XML fragment at the root of the Y document that holds the body: the
/// one the editor's collaboration extension uses when it is given none.
pub const FIELD: &str = "default";
