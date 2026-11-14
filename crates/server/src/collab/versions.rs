//! Keeping versions of a card's body (`body_versions`): the body its card holds, each
//! time it is other than the card's newest version, at the times below. A version is
//! the body's JSON, so it is taken from the card after the document's body was written
//! there, by whoever holds the card's place.
//!
//! - When the last of a room's writers leaves it, and when the room closes
//!   ([`Collab::version_as_left`], [`Collab::version_without_writers`]).
//! - Around a body written whole ([`super::whole`]): the body before, so that it can be
//!   gone back to, and the body after, by its writer.
//! - Before the first update a document takes after it was opened, if the card's body
//!   is not its newest version ([`Collab::version_before_update`]): one written before
//!   versions were kept, or past its document (an import, a copy), so that what the
//!   editors change can be gone back to.

use std::collections::HashMap;

use fvoci_model::{Block, BlockType};
use fvoci_store::BodyToKeep;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

use super::Collab;
use super::place::Held;
use super::upkeep::digest_of;
use crate::app::App;
use crate::error::AppError;

/// Whether the body of an open document is kept as a version of its card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Versioned {
    /// Not known: the document was opened, and took no update since.
    Unknown,
    /// The card's newest version is the document's body.
    Kept,
    /// The document took updates no version holds; the next version is due then.
    Due(Instant),
}

impl Collab {
    /// Notes that an open document took an update by `user_id`: a version is due
    /// `collab_version_minutes` after the first one no version holds, and the user
    /// is put down as having written since the newest.
    pub(super) fn updated_by(&self, slot: &mut Held, user_id: &str) {
        let due = Instant::now() + self.inner.version_interval;
        let Some(open) = slot.open.as_mut() else {
            return;
        };
        if !matches!(open.versioned, Versioned::Due(_)) {
            open.versioned = Versioned::Due(due);
        }
        if !user_id.is_empty() && !open.editors.iter().any(|editor| editor == user_id) {
            open.editors.push(user_id.to_owned());
        }
    }

    /// Before an open document takes an update: keeps the card's body as a version if
    /// it is not known to be one (see the module). What fails is logged: the update
    /// is taken all the same.
    pub(super) async fn version_before_update(&self, app: &App, card_id: &str, slot: &mut Held) {
        let unknown = slot
            .open
            .as_ref()
            .is_some_and(|open| open.versioned == Versioned::Unknown);
        if unknown && let Err(error) = self.keep_version(app, card_id, slot).await {
            tracing::error!(%error, card_id, "cannot keep a version of a card's body");
        }
    }

    /// As the last of a room's writers leaves, and after the document's body was
    /// written to its card: keeps it as a version if the document took updates no
    /// version holds. What fails is logged: who wrote stays put down, and the version
    /// is kept later.
    pub(super) async fn version_as_left(&self, app: &App, card_id: &str, slot: &mut Held) {
        let unkept = slot
            .open
            .as_ref()
            .is_some_and(|open| matches!(open.versioned, Versioned::Due(_)));
        if unkept && let Err(error) = self.keep_version(app, card_id, slot).await {
            tracing::error!(%error, card_id, "cannot keep a version of a card's body");
        }
    }

    /// For a room that has readers and no writer any more: writes the body of its
    /// open document to the card and keeps it as a version, if the document took
    /// updates no version holds. A document that is not open is left: it has none.
    /// What fails is logged, and done when the room closes.
    pub(super) async fn version_without_writers(
        &self,
        app: &App,
        card_id: &str,
        gone: CancellationToken,
    ) {
        let left = self
            .on_slot_while(
                app,
                card_id,
                Some(gone),
                move |collab, app, card_id, mut slot| async move {
                    let unkept = slot
                        .open
                        .as_ref()
                        .is_some_and(|open| matches!(open.versioned, Versioned::Due(_)));
                    let written = if unkept {
                        collab.write_now(&app, &card_id, &mut slot).await
                    } else {
                        Ok(())
                    };
                    if written.is_ok() {
                        collab.version_as_left(&app, &card_id, &mut slot).await;
                    }
                    collab.retire(&mut slot);
                    written
                },
            )
            .await;
        if let Err(error) = left
            && !error.is_not_found()
        {
            tracing::error!(%error, card_id, "cannot keep a version of a body its writers left");
        }
    }

    /// Keeps the body a card holds as a version, if it is not the card's newest. With
    /// an open document whose body the card holds, the document's body is kept from
    /// then on.
    async fn keep_version(
        &self,
        app: &App,
        card_id: &str,
        slot: &mut Held,
    ) -> Result<(), AppError> {
        let cards = stored_cards(app, &[card_id.to_owned()]).await?;
        keep_versions(app, &cards, "").await?;
        kept(slot);
        Ok(())
    }
}

/// Keeps the bodies cards hold as versions, each if it is not its card's newest; by
/// `written_by` if they were written whole.
///
/// # Errors
///
/// Returns the store's error.
pub(super) async fn keep_versions(
    app: &App,
    cards: &[Block],
    written_by: &str,
) -> Result<(), AppError> {
    if cards.is_empty() {
        return Ok(());
    }
    let digests: Vec<Vec<u8>> = cards
        .iter()
        .map(|card| digest_of(card.body.as_ref()))
        .collect();
    let bodies: Vec<BodyToKeep<'_>> = cards
        .iter()
        .zip(&digests)
        .map(|(block, digest)| BodyToKeep {
            block,
            digest,
            written_by,
        })
        .collect();
    app.store().keep_body_versions(&bodies).await?;
    Ok(())
}

/// Notes on a held slot that its card's body was kept as a version, or is its newest
/// already: nobody is put down as having written since, and an open document whose
/// body the card holds is kept.
pub(super) fn kept(slot: &mut Held) {
    if let Some(open) = slot.open.as_mut() {
        open.editors.clear();
        if open.written_seq >= open.log_end {
            open.versioned = Versioned::Kept;
        }
    }
}

/// Those of `card_ids` that are cards as `app` reaches them, not in the trash.
///
/// # Errors
///
/// Returns the store's error.
pub(super) async fn stored_cards(app: &App, card_ids: &[String]) -> Result<Vec<Block>, AppError> {
    let mut cards = Vec::with_capacity(card_ids.len());
    for card_id in card_ids {
        match app.store().get_block(card_id).await {
            Ok(block) if block.block_type == BlockType::Card => cards.push(block),
            Ok(_) => {}
            Err(error) if error.is_not_found() => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(cards)
}

/// The digests of the bodies of cards, by card.
pub(super) fn digests_of(cards: &[Block]) -> HashMap<String, Vec<u8>> {
    cards
        .iter()
        .map(|card| (card.id.clone(), digest_of(card.body.as_ref())))
        .collect()
}
