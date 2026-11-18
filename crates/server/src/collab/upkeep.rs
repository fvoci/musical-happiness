//! Taking an update, and keeping a document and its card in step afterwards: writing
//! the body to the card, folding the log into the state, and making a document that
//! grew too large again from its body.

use fvoci_model::Body;
use fvoci_store::{BodyBefore, StoreError, body_digest};
use serde_json::Value;
use tokio::time::{Instant, sleep_until};

use super::open::unexpected;
use super::place::{Held, OpenDoc, Opening, died_over};
use super::versions::Versioned;
use super::{Collab, DocChange, FOLD_AFTER, REMAKE_RETRY, Taken, bytes};
use crate::app::App;
use crate::collab_worker::wire::{Answer, Request};
use crate::error::{AppError, Reason};

/// How many times the body of a document is written to a card that keeps changing
/// meanwhile (its title, its fields) before the write is given up for now.
const PROJECTION_TRIES: u32 = 3;

impl Collab {
    /// Takes an update into the document of a held slot and logs it with what a repair
    /// deleted; a document whose log could not be written is closed, so that the next
    /// request opens it as it is stored.
    pub(super) async fn take_in(
        &self,
        app: &App,
        card_id: &str,
        slot: &mut Held,
        epoch: i64,
        user_id: &str,
        update: &[u8],
    ) -> Result<Taken, AppError> {
        let request = |doc| Request::Take {
            doc,
            update: update.to_vec(),
        };
        self.at_epoch(app, card_id, slot, epoch).await?;
        let answer = match self.ask(app, card_id, slot, request).await {
            Ok(answer) => answer,
            Err(refused) => {
                self.remake_refused(app, card_id, slot, &refused).await;
                return Err(refused);
            }
        };
        let (repair, state_bytes, files) = match answer {
            Answer::Taken {
                repair,
                state_bytes,
                files,
            } => (repair, state_bytes, files),
            other => return Err(unexpected(&other)),
        };
        if super::place::still_at_epoch(slot, epoch).is_err() {
            // Made again while the update was asked (a worker that ended, and a card
            // written past the document): the new document took an update made against
            // the one before, which is not logged, so it is closed.
            self.forget(slot).await;
            return Err(super::refusals::out_of_step());
        }
        let mut entries: Vec<(&[u8], &str)> = vec![(update, user_id)];
        if let Some(repair) = &repair {
            entries.push((repair, ""));
        }
        // Put down as having written since the newest version, unless they are: a
        // body kept first is by those put down before.
        let body_before = self.body_before_update(slot);
        let known = user_id.is_empty()
            || body_before == BodyBefore::Leave
                && slot
                    .open
                    .as_ref()
                    .is_some_and(|open| open.editors.iter().any(|editor| editor == user_id));
        let new_editors: &[&str] = if known { &[] } else { &[user_id] };
        let as_read = slot.open.as_ref().expect("the document is open").as_read();
        #[cfg(feature = "test-util")]
        self.paused_before_log().await;
        let logged = app
            .store()
            .append_body_updates(card_id, &entries, as_read, &files, new_editors, body_before)
            .await;
        let seqs = match logged {
            Ok(seqs) => seqs,
            Err(error) => return Err(self.not_logged(app, card_id, slot, error).await),
        };
        if body_before == BodyBefore::Keep {
            // The store kept the body the card held, by those put down before.
            super::versions::kept(slot);
        }
        let open = slot.open.as_mut().expect("the document is open");
        let now = Instant::now();
        open.log_end = seqs.last().copied().unwrap_or(open.log_end);
        open.state_bytes = state_bytes;
        user_id.clone_into(&mut open.last_writer);
        open.last_update = now;
        open.first_unwritten.get_or_insert(now);
        self.schedule_write(app, card_id, open);
        self.updated_by(slot, user_id);
        let remade = self.keep_up(app, card_id, slot).await;
        Ok(Taken {
            seq: seqs.first().copied().unwrap_or_default(),
            repair,
            remade,
        })
    }

    /// Opens again, as it is stored and at once, a document that holds an update its
    /// worker took and the store did not log, so that what others typed is still
    /// written; what the update is refused with. So is a document another process
    /// wrote since it was read here, and its room is told: what its editors hold
    /// lacks what that one wrote.
    async fn not_logged(
        &self,
        app: &App,
        card_id: &str,
        slot: &mut Held,
        error: StoreError,
    ) -> AppError {
        self.forget(slot).await;
        if let Err(reopened) = self.ensure_open(app, card_id, slot).await {
            tracing::warn!(%reopened, card_id, "cannot open a document again after an update that was not logged");
        }
        match error {
            StoreError::InvalidBlock { .. } => AppError::bad_request(
                "the update names a file (of an image or a file block) the board or wiki \
                 does not have",
            )
            .with_reason(Reason::UpdateRefused),
            StoreError::DocumentWritten { .. } => {
                self.tell(app, card_id, DocChange::Reopened);
                error.into()
            }
            other => other.into(),
        }
    }

    /// Makes the document again from its body if its state is near its limit, and
    /// folds its log into its state if it is long; whether it was made again. The
    /// update before is logged whatever happens here: what fails is logged, and tried
    /// again after a later update.
    async fn keep_up(&self, app: &App, card_id: &str, slot: &mut Held) -> bool {
        let Some(open) = slot.open.as_ref() else {
            return false;
        };
        let limit = self.inner.limits.state_bytes;
        let near_limit = open.state_bytes.saturating_mul(4) >= limit.saturating_mul(3);
        // A document made from a body that is near the limit itself is not made again
        // at every update: only once it grew by a quarter of the limit since.
        let grown = open.state_bytes >= open.made_bytes.saturating_add(limit / 4);
        if near_limit && grown && self.remake_and_tell(app, card_id, slot).await {
            return true;
        }
        // Not by a process that lost the lock of the server, as at closing.
        if app.holds_server_lock()
            && slot
                .open
                .as_ref()
                .is_some_and(|open| open.log_end - open.state_seq >= FOLD_AFTER)
            && let Err(error) = self.fold(app, card_id, slot).await
        {
            tracing::error!(%error, card_id, "cannot fold a document's log into its state");
        }
        false
    }

    /// Makes a document whose update was refused for the size of its state again from
    /// its body, if it grew since it was made (what was deleted is in its state), so
    /// that the editors who load it again can go on; whether it was made again.
    pub(super) async fn remake_refused(
        &self,
        app: &App,
        card_id: &str,
        slot: &mut Held,
        refused: &AppError,
    ) -> bool {
        let limit = self.inner.limits.state_bytes;
        let grown = slot.open.as_ref().is_some_and(|open| {
            open.state_bytes > open.made_bytes
                && open.state_bytes.saturating_mul(4) >= limit.saturating_mul(3)
        });
        refused.reason() == Some(Reason::DocumentTooLarge)
            && grown
            && self.remake_and_tell(app, card_id, slot).await
    }

    /// Makes the document again from its body and tells the room, unless that failed
    /// a little before (it is tried again after [`REMAKE_RETRY`]); whether it was made
    /// again. A document that could not be made again is closed: the store holds it.
    pub(super) async fn remake_and_tell(&self, app: &App, card_id: &str, slot: &mut Held) -> bool {
        // A process that lost the lock of the server would be refused by `remake_from`;
        // it does not try, and so logs no failure.
        if !app.holds_server_lock()
            || slot
                .remake_failed_at
                .is_some_and(|at| at.elapsed() < REMAKE_RETRY)
        {
            return false;
        }
        match self.remake(app, card_id, slot).await {
            Ok(replaced) => {
                // A card written past the document made it again from that, and the
                // room was told so.
                if !replaced {
                    self.tell(app, card_id, DocChange::Remade);
                }
                slot.remake_failed_at = None;
                true
            }
            Err(error) => {
                tracing::error!(%error, card_id, "cannot make a document that grew large again");
                slot.remake_failed_at = Some(Instant::now());
                false
            }
        }
    }

    /// Folds the log of the open document of a slot into its state, if it holds
    /// updates the state does not, for a document that is closed next: what is stored
    /// of it is its state alone. A fold that fails is logged; the log stays, and is
    /// taken again when the document is opened. Not by a process that lost the lock of
    /// the server (it is going down, and the next server may write the same document).
    pub(super) async fn fold_as_it_closes(&self, app: &App, card_id: &str, slot: &mut Held) {
        if app.holds_server_lock()
            && slot
                .open
                .as_ref()
                .is_some_and(|open| open.state_seq < open.log_end)
            && let Err(error) = self.fold(app, card_id, slot).await
        {
            tracing::error!(%error, card_id, "cannot fold a document's log into its state as it closes");
        }
    }

    /// Stores the document's state in place of its log. A document another process
    /// wrote since it was read here is closed: its numbers are the store's no more,
    /// and it is read again when it is next asked for.
    async fn fold(&self, app: &App, card_id: &str, slot: &mut Held) -> Result<(), AppError> {
        let state = bytes(self.ask(app, card_id, slot, Request::State).await?)?;
        let open = slot.open.as_mut().expect("the document is open");
        let through = open.log_end;
        let folded = app
            .store()
            .fold_body_doc(card_id, &state, through, open.as_read())
            .await;
        match folded {
            Ok(()) => {
                open.state_seq = through;
                Ok(())
            }
            Err(error @ StoreError::DocumentWritten { .. }) => {
                self.written_past(app, card_id, slot).await;
                Err(error.into())
            }
            Err(error) => Err(error.into()),
        }
    }

    /// Closes a document another process wrote since it was read here, and tells its
    /// room ([`DocChange::Reopened`]): its numbers are the store's no more, and its
    /// editors may lack what the other wrote. The next request, the room's among them,
    /// reads it again as it is stored.
    async fn written_past(&self, app: &App, card_id: &str, slot: &mut Held) {
        self.forget(slot).await;
        self.tell(app, card_id, DocChange::Reopened);
    }

    /// Writes the document's body to its card, and makes the document again from it;
    /// whether the card was written past the document, and the document was made again
    /// from that instead.
    async fn remake(&self, app: &App, card_id: &str, slot: &mut Held) -> Result<bool, AppError> {
        if self.write_or_replace(app, card_id, slot).await? {
            return Ok(true);
        }
        let body = app.store().get_block(card_id).await?.body;
        self.remake_held(app, card_id, slot, body.as_ref()).await?;
        Ok(false)
    }

    /// Makes the open document of a held slot again from `body`, the body its card was
    /// given whole past the document, and tells the room ([`DocChange::Replaced`]).
    pub(super) async fn replaced(
        &self,
        app: &App,
        card_id: &str,
        slot: &mut Held,
        body: Option<&Body>,
    ) -> Result<(), AppError> {
        self.remake_held(app, card_id, slot, body).await?;
        // The body the card was written is no version until it is kept as one, and
        // who is put down as having written since the newest may be so no more: a
        // version kept with the write names them.
        let due = Versioned::Due(self.version_clock());
        if let Some(open) = slot.open.as_mut() {
            open.versioned = due;
            open.editors.clear();
        }
        self.tell(app, card_id, DocChange::Replaced);
        Ok(())
    }

    /// Makes the open document of a held slot again from `body`; a document that could
    /// not be made again is closed (the store holds it as it was, or as it was made).
    async fn remake_held(
        &self,
        app: &App,
        card_id: &str,
        slot: &mut Held,
        body: Option<&Body>,
    ) -> Result<(), AppError> {
        let Some(open) = slot.open.as_mut() else {
            return Ok(());
        };
        match self.remake_from(app, card_id, open, body).await {
            Ok(()) => Ok(()),
            Err(Opening::Refused(error)) => {
                self.forget(slot).await;
                Err(error)
            }
            Err(Opening::WrittenSince(error)) => {
                self.written_past(app, card_id, slot).await;
                Err(error)
            }
            Err(Opening::Died) => {
                self.lost(slot);
                died_over(slot);
                Err(super::refusals::unavailable())
            }
        }
    }

    /// Makes an open document again from `body`, the body its card holds, in the same
    /// worker, and stores it in place of the one before and its log. The one before is
    /// closed first, so that a worker that holds as many documents as it may has room
    /// for it; whatever fails, `open` is no document to go on with: the caller closes
    /// it (its number is the new one's once that is open). Refused to a process that
    /// lost the lock of the server: making a document again empties its log, as a fold
    /// does, and the log may hold what the server that took the lock wrote since. A
    /// process learns that it lost the lock a while after it did
    /// (`app/server_lock.rs`), so the store is told what was read of the document,
    /// and refuses one that was written since.
    pub(super) async fn remake_from(
        &self,
        app: &App,
        card_id: &str,
        open: &mut OpenDoc,
        body: Option<&Body>,
    ) -> Result<(), Opening> {
        if !app.holds_server_lock() {
            return Err(Opening::Refused(super::refusals::unavailable()));
        }
        self.close_number(open.worker, open.generation, open.number)
            .await;
        let number = self.next_number();
        let (generation, state, state_bytes) = self
            .open_from_body(open.worker, number, body, open.share())
            .await?;
        open.generation = generation;
        open.number = number;
        let digest = body_digest(body);
        open.epoch = app
            .store()
            .remake_body_doc(card_id, &state, &digest, open.as_read())
            .await?;
        open.state_seq = open.log_end;
        open.written_seq = open.log_end;
        open.body_digest = digest;
        open.state_bytes = state_bytes;
        open.made_bytes = state_bytes;
        open.first_unwritten = None;
        Ok(())
    }

    /// Writes the body of the document to its card, if the card does not hold it yet:
    /// as the last writer changed it, with what any change of a body brings (its text,
    /// task counts, the files it shows), and told to the card's listeners and the
    /// webhooks. A body that is the card's already is only marked as written.
    ///
    /// A card whose body is no longer the one the document last matched was written
    /// past the document, whole: that body is not written over; the document is made
    /// again from it ([`DocChange::Replaced`]), and the updates the card did not hold
    /// are gone with the document before.
    ///
    /// The body the card then holds is kept as a version if one is due
    /// ([`Collab::version_if_due`]).
    pub(super) async fn write_now(
        &self,
        app: &App,
        card_id: &str,
        slot: &mut Held,
    ) -> Result<(), AppError> {
        self.write_or_replace(app, card_id, slot).await?;
        self.version_if_due(app, card_id, slot).await;
        Ok(())
    }

    /// As [`Collab::write_now`]; whether the card was written past the document, and the
    /// document was made again from it.
    async fn write_or_replace(
        &self,
        app: &App,
        card_id: &str,
        slot: &mut Held,
    ) -> Result<bool, AppError> {
        let Some(open) = slot.open.as_ref() else {
            return Ok(false);
        };
        if open.written_seq >= open.log_end {
            return Ok(false);
        }
        let held = app.store().get_block(card_id).await?;
        if body_digest(held.body.as_ref()) != open.body_digest {
            self.replaced(app, card_id, slot, held.body.as_ref())
                .await?;
            return Ok(true);
        }
        let json = match self.ask(app, card_id, slot, Request::Body).await? {
            Answer::Body(json) => json,
            other => return Err(unexpected(&other)),
        };
        let body = match json {
            None => None,
            Some(json) => {
                let value: Value = serde_json::from_slice(&json).map_err(AppError::internal)?;
                // The worker's body is held to the allowlist again here, by the server.
                Body::parse(value).map_err(AppError::internal)?
            }
        };
        let digest = body_digest(body.as_ref());
        let open = slot.open.as_ref().expect("the document is open");
        let through = open.log_end;
        let changed = digest != open.body_digest;
        let last_writer = open.last_writer.clone();
        let as_read = open.as_read();
        let mut read_at = held.update_at;
        #[cfg(feature = "test-util")]
        self.paused_before_projection().await;
        let mut tries = 0;
        loop {
            let projected = app
                .store()
                .project_body(
                    card_id,
                    changed.then(|| body.clone()),
                    read_at,
                    through,
                    &digest,
                    &last_writer,
                    as_read,
                )
                .await;
            if let Err(error @ StoreError::DocumentWritten { .. }) = projected {
                self.written_past(app, card_id, slot).await;
                return Err(error.into());
            }
            let Err(StoreError::Stale { .. }) = projected else {
                projected?;
                break;
            };
            // The card changed since it was read: its body was written past the
            // document, or something else of it was (a title), and the body is written
            // again with the card as it is now.
            let held = app.store().get_block(card_id).await?;
            let open = slot.open.as_ref().expect("the document is open");
            if body_digest(held.body.as_ref()) != open.body_digest {
                self.replaced(app, card_id, slot, held.body.as_ref())
                    .await?;
                return Ok(true);
            }
            tries += 1;
            if tries >= PROJECTION_TRIES {
                return Err(AppError::conflict(
                    "the card kept changing while the body of its document was written to it",
                ));
            }
            read_at = held.update_at;
        }
        let open = slot.open.as_mut().expect("the document is open");
        open.written_seq = through;
        open.body_digest = digest;
        open.first_unwritten = None;
        if changed {
            announce(app, card_id).await;
        }
        Ok(false)
    }

    /// Has a task write the body of the document when it is due ([`OpenDoc::write_at`]),
    /// unless one waits to already.
    pub(super) fn schedule_write(&self, app: &App, card_id: &str, open: &mut OpenDoc) {
        if open.writing_scheduled {
            return;
        }
        #[cfg(feature = "test-util")]
        if self
            .inner
            .due_writes_left
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            return;
        }
        open.writing_scheduled = true;
        let collab = self.clone();
        let app_of_task = app.clone();
        let card_id = card_id.to_owned();
        app.spawn_background(async move {
            collab.write_when_due(&app_of_task, &card_id).await;
        });
    }

    /// Waits until the body of a document is due to be written, and writes it.
    async fn write_when_due(&self, app: &App, card_id: &str) {
        loop {
            let due = {
                let _gate = self.reading(app.store().tenant()).await;
                let Some(mut slot) = self.existing_slot(app, card_id).await else {
                    return;
                };
                let Some(open) = slot.open.as_mut() else {
                    return;
                };
                let Some(due) = open.write_at() else {
                    open.writing_scheduled = false;
                    return;
                };
                due
            };
            sleep_until(due).await;
            let _gate = self.reading(app.store().tenant()).await;
            let Some(mut slot) = self.existing_slot(app, card_id).await else {
                return;
            };
            if slot
                .open
                .as_ref()
                .and_then(OpenDoc::write_at)
                .is_some_and(|due| due > Instant::now())
            {
                continue;
            }
            if let Err(error) = self.write_now(app, card_id, &mut slot).await {
                tracing::error!(%error, card_id, "cannot write a card's body from its document");
                if error.is_not_found() {
                    // The card went (to the trash): its document waits in the store
                    // for the card to come back.
                    self.forget(&mut slot).await;
                    self.retire(&mut slot);
                    return;
                }
                // Tried again a little later, whether or not anyone types.
                if let Some(open) = slot.open.as_mut() {
                    open.retry_at = Some(Instant::now() + super::WRITE_QUIET);
                }
                continue;
            }
            if let Some(open) = slot.open.as_mut() {
                open.retry_at = None;
                open.writing_scheduled = false;
            }
            return;
        }
    }
}

/// Tells the card's listeners and the webhooks that its body changed, in the
/// background, as a patch of the card does.
async fn announce(app: &App, card_id: &str) {
    let announced = async {
        let block = app.store().get_block(card_id).await?;
        let container = app.store().get_container(&block.container_id).await?;
        Ok::<_, fvoci_store::StoreError>((block, container))
    };
    let (block, container) = match announced.await {
        Ok(read) => read,
        Err(error) => {
            tracing::error!(%error, card_id, "cannot read a card whose body was written to tell of it");
            return;
        }
    };
    let app_of_task = app.clone();
    app.spawn_background(async move {
        app_of_task
            .ws()
            .broadcast_block_change(&container.workspace_id, &block)
            .await;
        app_of_task.webhook().notify_update(&block).await;
    });
}
