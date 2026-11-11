//! The body of a card as a Y document, for people to edit it together.
//!
//! A body is stored as JSON (`fvoci_model::Body`). While it is edited by several people
//! at once it is also a Y document, which their editors change through the binding of
//! the web app's editor (`@tiptap/y-tiptap`). This crate is what the server needs to
//! stand between the two, with no input or output of its own:
//!
//! - [`from_body`] makes the Y document of a stored body, in the shape the editors read.
//! - [`to_body`] projects a Y document back to the JSON an editor would store from it,
//!   and checks it as any body is checked.
//! - [`repair`] deletes from a Y document what the allowlist refuses, as the editors
//!   themselves delete what their schema refuses: two edits that are each valid can
//!   merge into a document that is not.
//! - [`with_update`] gives the Y document as it would be with a client's update, and
//!   refuses an update that cannot be taken as it is.
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
mod update;

pub use build::{from_body, from_body_by, new_doc};
pub use project::to_body;
pub use repair::{Repair, repair};
pub use update::{UpdateError, UpdateLimits, copy_of, state_of, with_update};
pub use yrs;

/// The name of the XML fragment at the root of the Y document that holds the body: the
/// one the editor's collaboration extension uses when it is given none.
pub const FIELD: &str = "default";
