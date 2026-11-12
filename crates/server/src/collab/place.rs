//! The place of each card's document: one at a time works on it, and what is known of
//! it while it is open.

use std::sync::Arc;
use std::time::Duration;

use fvoci_store::Tenant;
use tokio::sync::OwnedMutexGuard;
use tokio::time::Instant;

use super::worker::CallError;
use super::{Collab, DocChange, DocEvent, refusals, taken_from};
use crate::app::App;
use crate::collab_worker::wire::{Answer, DocNumber, Request};
use crate::error::AppError;

/// How long a document that ended its worker twice in a row is closed, the first time;
/// it doubles with each further time.
const FIRST_CLOSING: Duration = Duration::from_secs(10);

/// The longest a document is closed for ending its worker.
const LONGEST_CLOSING: Duration = Duration::from_mins(5);

/// What is known of a card's document. Held while one request works on it.
#[derive(Debug, Default)]
pub(super) struct Slot {
    /// The slot is no longer in the place: whoever waited for it looks again.
    retired: bool,
    /// Whose card it is: the organization of the handle that first opened it.
    pub(super) tenant: Option<Tenant>,
    pub(super) open: Option<OpenDoc>,
    /// How many requests about it in a row ended its worker.
    deaths: u32,
    /// Until when it is closed for that.
    closed_until: Option<Instant>,
}

/// A document that a worker holds.
#[derive(Debug)]
pub(super) struct OpenDoc {
    /// Which worker, of which generation, under which number.
    pub(super) worker: usize,
    pub(super) generation: u64,
    pub(super) number: DocNumber,
    /// The last update its stored state holds.
    pub(super) state_seq: i64,
    /// The last update logged.
    pub(super) log_end: i64,
    /// The last update whose body the card holds.
    pub(super) written_seq: i64,
    /// The digest of the body the card holds, as of `written_seq`.
    pub(super) body_digest: Vec<u8>,
    /// The size of its state.
    pub(super) state_bytes: u64,
    /// Who sent the last update; what writes the body writes it as them.
    pub(super) last_writer: String,
    /// When the first update whose body is not written was taken.
    pub(super) first_unwritten: Option<Instant>,
    /// When the last update was taken.
    pub(super) last_update: Instant,
    /// Whether a task waits to write its body.
    pub(super) writing_scheduled: bool,
}

impl OpenDoc {
    /// When its body is to be written, if it has updates whose body is not.
    pub(super) fn write_at(&self) -> Option<Instant> {
        let first = self.first_unwritten?;
        Some((self.last_update + super::WRITE_QUIET).min(first + super::WRITE_LONGEST))
    }
}

impl Collab {
    /// The slot of a card, held, for a request of `app`'s organization.
    ///
    /// # Errors
    ///
    /// Returns a not-found error if the card's document is open for another
    /// organization (no card of that identifier is in this one's reach).
    pub(super) async fn slot(
        &self,
        app: &App,
        card_id: &str,
    ) -> Result<OwnedMutexGuard<Slot>, AppError> {
        loop {
            let shared = {
                let mut slots = self.inner.slots.lock().expect("the slots are not poisoned");
                Arc::clone(slots.entry(card_id.to_owned()).or_default())
            };
            let mut slot = shared.lock_owned().await;
            if slot.retired {
                continue;
            }
            let tenant = app.store().tenant();
            if slot.tenant.is_some_and(|holder| holder != tenant) {
                return Err(AppError::not_found("card"));
            }
            slot.tenant = Some(tenant);
            return Ok(slot);
        }
    }

    /// The slot of a card if it has one, held, for a request of `app`'s organization.
    pub(super) async fn existing_slot(
        &self,
        app: &App,
        card_id: &str,
    ) -> Option<OwnedMutexGuard<Slot>> {
        loop {
            let shared = {
                let slots = self.inner.slots.lock().expect("the slots are not poisoned");
                Arc::clone(slots.get(card_id)?)
            };
            let slot = shared.lock_owned().await;
            if slot.retired {
                continue;
            }
            return (slot.tenant == Some(app.store().tenant())).then_some(slot);
        }
    }

    /// Every slot, by card.
    pub(super) fn all_slots(&self) -> Vec<(String, Arc<tokio::sync::Mutex<Slot>>)> {
        let slots = self.inner.slots.lock().expect("the slots are not poisoned");
        slots
            .iter()
            .map(|(card_id, slot)| (card_id.clone(), Arc::clone(slot)))
            .collect()
    }

    /// Takes a slot out of the place, unless it remembers that its document is closed
    /// for a while.
    pub(super) fn retire(&self, card_id: &str, slot: &mut Slot) {
        if slot.open.is_some()
            || slot
                .closed_until
                .is_some_and(|until| until > Instant::now())
        {
            return;
        }
        slot.retired = true;
        let mut slots = self.inner.slots.lock().expect("the slots are not poisoned");
        slots.remove(card_id);
    }

    /// Closes the document in its worker, if it is open, and forgets it.
    pub(super) async fn forget(&self, slot: &mut Slot) {
        if let Some(open) = slot.open.take() {
            taken_from(&self.inner.loads[open.worker]);
            let worker = &self.inner.workers[open.worker];
            // A worker that is gone took the document with it.
            let _ = worker
                .call(Some(open.generation), &Request::Close(open.number))
                .await;
        }
    }

    /// Asks the worker that holds the document a request made for its number, opening
    /// the document first if it is not open, and again once if its worker is gone.
    ///
    /// # Errors
    ///
    /// Returns a refusal of the worker as the reason it stands for, `document_unavailable`
    /// if the worker ended over it (and the document is closed for a while if that keeps
    /// happening), and the errors of opening the document.
    pub(super) async fn ask(
        &self,
        app: &App,
        card_id: &str,
        slot: &mut Slot,
        request: impl Fn(DocNumber) -> Request,
    ) -> Result<Answer, AppError> {
        for _ in 0..2 {
            self.ensure_open(app, card_id, slot).await?;
            let open = slot.open.as_ref().expect("the document is open");
            let worker = &self.inner.workers[open.worker];
            match worker
                .call(Some(open.generation), &request(open.number))
                .await
            {
                Ok((_, answer)) => {
                    slot.deaths = 0;
                    return Ok(answer);
                }
                Err(CallError::Lost) => {
                    self.lost(slot);
                    self.tell(card_id, DocChange::Reopened);
                }
                Err(CallError::Refused(refused)) => {
                    return Err(refusals::refused(&refused, &self.inner.limits));
                }
                Err(CallError::Died(death)) => {
                    tracing::warn!(%death, card_id, "a collab worker ended over a card's document");
                    self.lost(slot);
                    died_over(slot);
                    return Err(refusals::unavailable());
                }
                Err(CallError::Unavailable(error)) => {
                    tracing::error!(%error, card_id, "no collab worker for a card's document");
                    return Err(refusals::unavailable());
                }
            }
        }
        Err(refusals::unavailable())
    }

    /// Opens the document if it is not open, unless it is closed for a while.
    ///
    /// # Errors
    ///
    /// Returns `document_unavailable` while it is closed, and the errors of opening it.
    pub(super) async fn ensure_open(
        &self,
        app: &App,
        card_id: &str,
        slot: &mut Slot,
    ) -> Result<(), AppError> {
        if slot.open.is_some() {
            return Ok(());
        }
        if slot
            .closed_until
            .is_some_and(|until| until > Instant::now())
        {
            return Err(refusals::unavailable());
        }
        match self.open_stored(app, card_id).await {
            Ok(open) => {
                slot.open = Some(open);
                Ok(())
            }
            Err(Opening::Refused(error)) => Err(error),
            Err(Opening::Died) => {
                died_over(slot);
                Err(refusals::unavailable())
            }
        }
    }

    /// Forgets a document whose worker is gone.
    fn lost(&self, slot: &mut Slot) {
        if let Some(open) = slot.open.take() {
            taken_from(&self.inner.loads[open.worker]);
        }
    }

    /// Tells the subscribers what happened to a card's document.
    pub(super) fn tell(&self, card_id: &str, change: DocChange) {
        // Nobody may be listening.
        let _ = self.inner.events.send(DocEvent {
            card_id: card_id.to_owned(),
            change,
        });
    }
}

/// Counts a death of the worker over the document, and closes the document for a
/// while from the second in a row.
fn died_over(slot: &mut Slot) {
    slot.deaths = slot.deaths.saturating_add(1);
    if slot.deaths >= 2 {
        let doublings = (slot.deaths - 2).min(16);
        let closing = FIRST_CLOSING
            .saturating_mul(1 << doublings)
            .min(LONGEST_CLOSING);
        slot.closed_until = Some(Instant::now() + closing);
    }
}

/// Why a document could not be opened.
#[derive(Debug)]
pub(super) enum Opening {
    /// Refused, or the store failed: the error to answer with.
    Refused(AppError),
    /// The worker ended over it.
    Died,
}

impl From<AppError> for Opening {
    fn from(error: AppError) -> Self {
        Self::Refused(error)
    }
}

impl From<fvoci_store::StoreError> for Opening {
    fn from(error: fvoci_store::StoreError) -> Self {
        Self::Refused(error.into())
    }
}
