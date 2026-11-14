//! Keeping versions of a card's body (`body_versions`): the body its card holds, each
//! time it is other than the card's newest version, at the times below. A version is
//! the body's JSON, so it is taken from the card after the document's body was written
//! there, by whoever holds the card's place.
//!
//! - When the last of those who wrote since the newest version leaves the room,
//!   whoever stays in it and wrote nothing, when the room closes, and when the server
//!   stops ([`Collab::version_as_left`], [`Collab::version_without_writers`]).
//! - While a body is edited on: `collab_version_minutes` after the first update no
//!   version holds, as the body is next written to the card
//!   ([`Collab::version_if_due`]) or, of a card that holds it already, with the next
//!   update ([`Collab::body_before_update`]). A body nobody edits on is kept when its
//!   writers leave, not by the clock.
//! - When the server starts, of the documents a server that ended left with what no
//!   version holds ([`Collab::recover`]): such a document is opened with a version
//!   due.
//! - Around a body written whole: the store keeps the body before, so that it can be
//!   gone back to, and the body after, by its writer, within that write
//!   (`fvoci_store::Store::patch_block`).
//! - With the first update a document takes after it was opened, if the card's body
//!   is not its newest version ([`Collab::body_before_update`]): one written before
//!   versions were kept, or past its document (an import, a copy), so that what the
//!   editors change can be gone back to. The store keeps it within the write of the
//!   update's log.

use fvoci_store::BodyBefore;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

use super::Collab;
use super::place::Held;
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
        let due = self.version_clock() + self.inner.version_interval;
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

    /// What the log of an update an open document took does with the body its card
    /// holds: kept as a version first if it is not known to be one, or if one is due
    /// (see the module).
    pub(super) fn body_before_update(&self, slot: &Held) -> BodyBefore {
        // Only of a card that holds all its document took.
        let unknown = slot.open.as_ref().is_some_and(|open| {
            open.versioned == Versioned::Unknown && open.written_seq >= open.log_end
        });
        if unknown || self.is_due(slot) {
            BodyBefore::Keep
        } else {
            BodyBefore::Leave
        }
    }

    /// After the body of an open document was written to its card: keeps it as a
    /// version if one is due. What fails is logged, and kept later.
    pub(super) async fn version_if_due(&self, app: &App, card_id: &str, slot: &mut Held) {
        if self.is_due(slot)
            && let Err(error) = self.keep_version(app, card_id, slot).await
        {
            tracing::error!(%error, card_id, "cannot keep a version of a card's body");
        }
    }

    /// Whether a version of the body the card of an open document holds is due now.
    /// Only of a card that holds all its document took: a version of less would take
    /// the names of those whose updates it lacks.
    fn is_due(&self, slot: &Held) -> bool {
        let now = self.version_clock();
        slot.open.as_ref().is_some_and(|open| {
            matches!(open.versioned, Versioned::Due(due) if due <= now)
                && open.written_seq >= open.log_end
        })
    }

    /// The time the versions are due by.
    pub(super) fn version_clock(&self) -> Instant {
        #[cfg(feature = "test-util")]
        let passed = *self
            .inner
            .version_time_passed
            .lock()
            .expect("lock poisoned");
        #[cfg(not(feature = "test-util"))]
        let passed = std::time::Duration::ZERO;
        Instant::now() + passed
    }

    /// Has the versions take it to be `by` later from now on: for the tests of what
    /// is kept as time passes.
    #[cfg(feature = "test-util")]
    #[doc(hidden)]
    pub fn pass_version_time(&self, by: std::time::Duration) {
        *self
            .inner
            .version_time_passed
            .lock()
            .expect("lock poisoned") += by;
    }

    /// As the last of those who wrote leaves a room, and after the document's body was
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

    /// For a room that one who typed has left, or writes in no more: writes the body
    /// of its open document to the card and keeps it as a version, if the document
    /// took updates no version holds and none of them is by one of `writing`, the
    /// users still in the room to write. Whether the room is to ask again when one of
    /// those leaves: one of them wrote it, or it could not be told or kept now (the
    /// document is not open, and is opened with a version due; the store failed,
    /// which is logged). What is not kept by then is when the room closes.
    pub(super) async fn version_without_writers(
        &self,
        app: &App,
        card_id: &str,
        gone: CancellationToken,
        writing: &[String],
    ) -> bool {
        let writing = writing.to_vec();
        let left = self
            .on_slot_while(
                app,
                card_id,
                Some(gone),
                move |collab, app, card_id, mut slot| async move {
                    let Some(open) = slot.open.as_ref() else {
                        collab.retire(&mut slot);
                        return Ok(true);
                    };
                    if !matches!(open.versioned, Versioned::Due(_)) {
                        return Ok(false);
                    }
                    if open.editors.iter().any(|editor| writing.contains(editor)) {
                        return Ok(true);
                    }
                    let written = collab.write_now(&app, &card_id, &mut slot).await;
                    if written.is_ok() {
                        collab.version_as_left(&app, &card_id, &mut slot).await;
                    }
                    collab.retire(&mut slot);
                    written.map(|()| false)
                },
            )
            .await;
        match left {
            Ok(ask_again) => ask_again,
            Err(error) if error.is_not_found() => false,
            Err(error) => {
                tracing::error!(%error, card_id, "cannot keep a version of a body its writers left");
                true
            }
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
        app.store().keep_body_version(card_id).await?;
        kept(slot);
        Ok(())
    }
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
