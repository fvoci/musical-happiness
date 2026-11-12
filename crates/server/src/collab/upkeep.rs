//! Taking an update, and keeping a document and its card in step afterwards: writing
//! the body to the card, folding the log into the state, and making a document that
//! grew too large again from its body.

use fvoci_model::Body;
use fvoci_store::StoreError;
use serde_json::Value;
use sha2::{Digest as _, Sha256};
use tokio::time::{Instant, sleep_until};

use super::open::unexpected;
use super::place::{Held, OpenDoc, Opening, died_over};
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
        user_id: &str,
        update: &[u8],
    ) -> Result<Taken, AppError> {
        let request = |doc| Request::Take {
            doc,
            update: update.to_vec(),
        };
        let answer = match self.ask(app, card_id, slot, request).await {
            Ok(answer) => answer,
            Err(refused) => {
                self.remake_refused(app, card_id, slot, &refused).await;
                return Err(refused);
            }
        };
        let (repair, state_bytes) = match answer {
            Answer::Taken {
                repair,
                state_bytes,
            } => (repair, state_bytes),
            other => return Err(unexpected(&other)),
        };
        let mut entries: Vec<(&[u8], &str)> = vec![(update, user_id)];
        if let Some(repair) = &repair {
            entries.push((repair, ""));
        }
        let seqs = match app.store().append_body_updates(card_id, &entries).await {
            Ok(seqs) => seqs,
            Err(error) => {
                self.forget(slot).await;
                return Err(error.into());
            }
        };
        let open = slot.open.as_mut().expect("the document is open");
        let now = Instant::now();
        open.log_end = seqs.last().copied().unwrap_or(open.log_end);
        open.state_bytes = state_bytes;
        user_id.clone_into(&mut open.last_writer);
        open.last_update = now;
        open.first_unwritten.get_or_insert(now);
        self.schedule_write(app, card_id, open);
        let remade = self.keep_up(app, card_id, slot).await;
        Ok(Taken {
            seq: seqs.first().copied().unwrap_or_default(),
            repair,
            remade,
        })
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
        if slot
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
    async fn remake_and_tell(&self, app: &App, card_id: &str, slot: &mut Held) -> bool {
        if slot
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
                    self.tell(card_id, DocChange::Remade);
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

    /// Stores the document's state in place of its log.
    async fn fold(&self, app: &App, card_id: &str, slot: &mut Held) -> Result<(), AppError> {
        let state = bytes(self.ask(app, card_id, slot, Request::State).await?)?;
        let open = slot.open.as_mut().expect("the document is open");
        let through = open.log_end;
        app.store().fold_body_doc(card_id, &state, through).await?;
        open.state_seq = through;
        Ok(())
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
        self.tell(card_id, DocChange::Replaced);
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
    /// it (its number is the new one's once that is open).
    pub(super) async fn remake_from(
        &self,
        app: &App,
        card_id: &str,
        open: &mut OpenDoc,
        body: Option<&Body>,
    ) -> Result<(), Opening> {
        self.close_number(open.worker, open.generation, open.number)
            .await;
        let number = self.next_number();
        let (generation, state, state_bytes) =
            self.open_from_body(open.worker, number, body).await?;
        open.generation = generation;
        open.number = number;
        let digest = digest_of(body);
        app.store()
            .remake_body_doc(card_id, &state, &digest)
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
    pub(super) async fn write_now(
        &self,
        app: &App,
        card_id: &str,
        slot: &mut Held,
    ) -> Result<(), AppError> {
        self.write_or_replace(app, card_id, slot).await.map(drop)
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
        if digest_of(held.body.as_ref()) != open.body_digest {
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
        let digest = digest_of(body.as_ref());
        let open = slot.open.as_ref().expect("the document is open");
        let through = open.log_end;
        let changed = digest != open.body_digest;
        let last_writer = open.last_writer.clone();
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
                )
                .await;
            let Err(StoreError::Stale { .. }) = projected else {
                projected?;
                break;
            };
            // The card changed since it was read: its body was written past the
            // document, or something else of it was (a title), and the body is written
            // again with the card as it is now.
            let held = app.store().get_block(card_id).await?;
            let open = slot.open.as_ref().expect("the document is open");
            if digest_of(held.body.as_ref()) != open.body_digest {
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
            }
            if let Some(open) = slot.open.as_mut() {
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
        let board = app.store().get_board(&block.board_id).await?;
        Ok::<_, fvoci_store::StoreError>((block, board))
    };
    let (block, board) = match announced.await {
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
            .broadcast_block_change(&board.workspace_id, &block)
            .await;
        app_of_task.webhook().notify_update(&block).await;
    });
}

/// The digest of a body as `body_docs.body_digest` holds it: SHA-256 of its JSON with
/// the members of every object in the order of their names and no space; of nothing for
/// no body.
pub(super) fn digest_of(body: Option<&Body>) -> Vec<u8> {
    let mut json = Vec::new();
    if let Some(body) = body {
        write_canonical(body.document(), &mut json);
    }
    Sha256::digest(&json).to_vec()
}

/// Writes a JSON value with the members of every object in the order of their names.
fn write_canonical(value: &Value, out: &mut Vec<u8>) {
    match value {
        Value::Object(members) => {
            let mut names: Vec<&String> = members.keys().collect();
            names.sort_unstable();
            out.push(b'{');
            for (index, name) in names.into_iter().enumerate() {
                if index > 0 {
                    out.push(b',');
                }
                write_canonical(&Value::String(name.clone()), out);
                out.push(b':');
                write_canonical(&members[name], out);
            }
            out.push(b'}');
        }
        Value::Array(items) => {
            out.push(b'[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(b',');
                }
                write_canonical(item, out);
            }
            out.push(b']');
        }
        // A value with no members is written the same whatever the order of a map.
        other => out.extend_from_slice(other.to_string().as_bytes()),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn the_digest_of_a_body_does_not_depend_on_the_order_of_its_members() {
        let one = Body::from_stored(json!({"type": "doc", "content": [{"type": "paragraph"}]}));
        let other = Body::from_stored(
            serde_json::from_str(r#"{"content":[{"type":"paragraph"}],"type":"doc"}"#)
                .expect("JSON"),
        );
        assert_eq!(digest_of(Some(&one)), digest_of(Some(&other)));
        assert_ne!(digest_of(Some(&one)), digest_of(None), "a body is not none");
        let mut written = Vec::new();
        write_canonical(
            &json!({"b": [1, "x"], "a": {"d": null, "c": true}}),
            &mut written,
        );
        assert_eq!(
            String::from_utf8(written).expect("UTF-8"),
            r#"{"a":{"c":true,"d":null},"b":[1,"x"]}"#
        );
    }
}
