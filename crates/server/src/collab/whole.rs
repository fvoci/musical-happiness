//! Bodies written whole to cards by the routes (a patch, a batch of patches, a block
//! stored again): around the documents of the cards, so that what their editors typed
//! is in the card before, and their documents are that body after.

use std::future::Future;

use fvoci_model::{Block, BlockPatch};
use fvoci_store::body_digest;

use super::place::Held;
use super::{Collab, DocChange, refusals};
use crate::app::App;
use crate::error::{AppError, ErrorKind};

/// Whether writing pending document updates to a card should also keep an automatic
/// version if the version clock says one is due.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DueVersion {
    /// Keep an automatic version if one is due (`write_whole`).
    Keep,
    /// Skip the automatic version because the caller is keeping one by hand
    /// (`keep_by_hand`).
    Skip,
}

/// The cards and pages whose bodies a batch of patches writes: of `blocks`, those
/// read for `block_ids`, the ones whose body has a room and whose patch (at the same
/// place as their identifier) sets a body.
#[must_use]
pub fn cards_written_whole(
    blocks: &[Block],
    block_ids: &[String],
    patches: &[BlockPatch],
) -> Vec<String> {
    block_ids
        .iter()
        .zip(patches)
        .filter(|(block_id, patch)| {
            !patch.body.is_keep()
                && blocks
                    .iter()
                    .any(|block| &block.id == *block_id && block.block_type.has_room())
        })
        .map(|(block_id, _)| block_id.clone())
        .collect()
}

impl Collab {
    /// Runs `write`, a write of the whole bodies of the cards `card_ids` (with the
    /// `updatedAt` its writer read), around their documents, in a task of its own that
    /// holds each card's place (taken in the order of their keys, so that two of them
    /// never wait on each other), so that a caller that stops waiting leaves no half
    /// of it:
    ///
    /// - before, what the editors typed and the card does not hold yet is written to
    ///   it, so that a writer who read the card before is refused (409) and reads it
    ///   again; this includes the log of a document that is not open. If that cannot be
    ///   done, `write` does not run: it would write over updates its writer never saw;
    /// - after, an open document whose card holds another body now is made again from
    ///   that body, and the room is told ([`super::DocChange::Replaced`]). A document
    ///   that is not open is made again from its card's body when it is opened.
    ///
    /// The store keeps the versions of such a write within it
    /// (`fvoci_store::Store::patch_block`): the body before and the body after.
    ///
    /// # Errors
    ///
    /// Returns a not-found error if a card is gone, a conflict if it kept changing
    /// concurrently, `document_unavailable` if what the editors typed cannot otherwise
    /// be written first, and the error of `write`. What fails after `write` is logged:
    /// the cards hold what was written, and a document that was not made again is closed.
    pub async fn write_whole<R, W, F>(
        &self,
        app: &App,
        card_ids: Vec<String>,
        write: W,
    ) -> Result<R, AppError>
    where
        R: Send + 'static,
        W: FnOnce(App) -> F + Send + 'static,
        F: Future<Output = Result<R, AppError>> + Send + 'static,
    {
        let (collab, app_of_task) = (self.clone(), app.clone());
        let task = app.spawn_tracked(async move {
            collab.write_whole_held(&app_of_task, card_ids, write).await
        });
        task.await.map_err(|error| {
            AppError::internal(format_args!("the write of whole bodies ended: {error}"))
        })?
    }

    async fn write_whole_held<R, W, F>(
        &self,
        app: &App,
        mut card_ids: Vec<String>,
        write: W,
    ) -> Result<R, AppError>
    where
        W: FnOnce(App) -> F,
        F: Future<Output = Result<R, AppError>>,
    {
        let _gate = self.reading(app.store().tenant()).await;
        card_ids.sort();
        card_ids.dedup();
        let mut held: Vec<(String, Held)> = Vec::with_capacity(card_ids.len());
        for card_id in card_ids {
            let slot = self.slot(app, &card_id).await;
            held.push((card_id, slot));
        }
        let mut written_first = Ok(());
        for (card_id, slot) in &mut held {
            if let Err(error) = self
                .write_pending(app, card_id, slot, DueVersion::Keep)
                .await
            {
                written_first = Err(error);
                break;
            }
        }
        let written = match written_first {
            Ok(()) => write(app.clone()).await,
            Err(error) => Err(error),
        };
        for (card_id, mut slot) in held {
            if written.is_ok() && slot.open.is_some() {
                self.after_whole(app, &card_id, &mut slot).await;
                self.retire(&mut slot);
                continue;
            }
            // Not open (its worker ended over it, or it was opened for its log alone),
            // with a room all the same: the room holds the document before the write,
            // and is told to open it anew.
            if written.is_ok() && self.room_is_open(app, &card_id) {
                self.tell(app, &card_id, DocChange::Closed);
            }
            self.retire(&mut slot);
        }
        written
    }

    /// Writes to a card what the editors typed and it does not hold yet: from its open
    /// document, or from the document's log, opened aside for that and closed after
    /// ([`Collab::open_aside`]), so that a server that holds as many documents as it
    /// may still writes it.
    ///
    /// # Errors
    ///
    /// Preserves `NotFound` and `Conflict` errors (such as a card deleted or racing
    /// concurrently), returns `document_unavailable` if what was read cannot otherwise
    /// be written, and propagates errors from checking or opening the log.
    pub(super) async fn write_pending(
        &self,
        app: &App,
        card_id: &str,
        slot: &mut Held,
        due: DueVersion,
    ) -> Result<(), AppError> {
        let opened_aside = slot.open.is_none() && app.store().has_unwritten_body(card_id).await?;
        if opened_aside {
            self.open_aside(app, card_id, slot).await?;
        }
        let written = match slot.open {
            Some(_) => match due {
                DueVersion::Keep => self.write_now(app, card_id, slot).await,
                DueVersion::Skip => self.write_or_replace(app, card_id, slot).await.map(drop),
            },
            None => Ok(()),
        };
        if opened_aside {
            // Opened for its log alone: the next request opens it from the card.
            self.forget(slot).await;
        }
        written.map_err(|error| match error.kind() {
            ErrorKind::NotFound | ErrorKind::Conflict => error,
            _ => {
                tracing::error!(%error, card_id, "cannot write what the editors typed to its card");
                refusals::unavailable()
            }
        })
    }

    /// Makes an open document whose card was written whole again from that body.
    async fn after_whole(&self, app: &App, card_id: &str, slot: &mut Held) {
        let body = match app.store().get_block(card_id).await {
            Ok(block) => block.body,
            Err(error) => {
                tracing::error!(%error, card_id, "cannot read a card written whole; its document is closed");
                self.forget(slot).await;
                self.tell(app, card_id, DocChange::Closed);
                return;
            }
        };
        if slot
            .open
            .as_ref()
            .is_some_and(|open| body_digest(body.as_ref()) == open.body_digest)
        {
            return;
        }
        if let Err(error) = self.replaced(app, card_id, slot, body.as_ref()).await {
            // Closed by the failure: the room is told, and opens it from the card.
            tracing::error!(%error, card_id, "cannot make a document again from a body written whole");
            self.tell(app, card_id, DocChange::Closed);
            return;
        }
        // The store kept that body as a version within the write, and named those put
        // down as having written before it.
        super::versions::kept(slot);
    }
}
