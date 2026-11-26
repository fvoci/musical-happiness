//! Markdown an agent adds after the last of a body: read as the importers read it,
//! files named `fvoci-file://{id}` (an image `![alt](…)`, a file `[name](…)`), and
//! written into the body's document as one more editor writes
//! ([`crate::collab::Collab::append`]), never over what its editors typed.

use fvoci_model::{RefusalReason, Refusals};
use serde_json::Value;

use super::App;
use super::caller::{Caller, CallerAuth};
use super::import::markdown::{
    AssetResolver, BodyContext, ResolvedTarget, blocks_to_body, markdown_to_blocks,
};
use crate::collab::Appender;
use crate::error::AppError;

/// The scheme a file of the body's board is named by in the Markdown an agent adds.
const FILE_SCHEME: &str = "fvoci-file://";

/// How deep Markdown an agent adds may nest.
const MAX_DEPTH: usize = 16;

/// Names the files of the body's board by their identifiers; any other target is a
/// link, or text where its scheme is not one a link may have.
struct FilesOfTheBoard;

impl FilesOfTheBoard {
    fn file_id(dest: &str) -> Option<String> {
        let id = dest.trim().strip_prefix(FILE_SCHEME)?;
        (!id.is_empty() && !id.contains(['/', '?', '#'])).then(|| id.to_owned())
    }
}

impl AssetResolver for FilesOfTheBoard {
    fn resolve_image(&mut self, dest: &str) -> ResolvedTarget {
        match Self::file_id(dest) {
            Some(file_id) => ResolvedTarget::LocalImage { file_id },
            None => super::import::markdown::ExternalOnlyResolver.resolve_image(dest),
        }
    }

    fn resolve_link(&mut self, dest: &str) -> ResolvedTarget {
        match Self::file_id(dest) {
            Some(file_id) => ResolvedTarget::LocalFile { file_id },
            None => super::import::markdown::ExternalOnlyResolver.resolve_link(dest),
        }
    }
}

impl App {
    /// Adds `markdown` after the last of the body of the block `block_id` of the board
    /// `container_id` (a card, a page or a unit), by `caller`: read into a body's nodes,
    /// whose files must be the board's, and written into the block's document. The
    /// caller may change the block, which the route checked.
    ///
    /// # Errors
    ///
    /// Returns 400 at `/markdown` for Markdown that adds nothing, nests too deep or
    /// names a file the board does not have; 413 for a body or document that would be
    /// larger than it may; and the errors of [`crate::collab::Collab::append`].
    pub async fn append_markdown(
        &self,
        caller: &Caller,
        block_id: &str,
        container_id: &str,
        markdown: &str,
    ) -> Result<(), AppError> {
        let nodes = markdown_to_blocks(
            markdown,
            BodyContext::Document,
            MAX_DEPTH,
            &mut FilesOfTheBoard,
        )
        .map_err(|error| error.with_field("markdown"))?
        .nodes;
        let Some(added) = blocks_to_body(nodes.clone())? else {
            let mut refusals = Refusals::new();
            refusals.refuse(
                "markdown",
                RefusalReason::Required,
                "the Markdown adds nothing",
            );
            return Err(refusals.into());
        };
        self.check_body_files(container_id, Some(&added))
            .await
            .map_err(|error| error.with_field("markdown"))?;
        let token_id = match &caller.auth {
            CallerAuth::ApiToken(delegation) => Some(delegation.token_id.as_str()),
            CallerAuth::Session(_) => None,
        };
        let nodes: Vec<Value> = nodes;
        self.collab()
            .append(
                self,
                block_id,
                Appender {
                    user_id: &caller.user_id,
                    token_id,
                },
                &nodes,
            )
            .await
    }
}
