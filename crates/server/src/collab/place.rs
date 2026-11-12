//! The place of each card's document: one at a time works on it, and what is known of
//! it while it is open.

use std::sync::Arc;
use std::time::Duration;

use fvoci_store::Tenant;
use tokio::sync::{Mutex, OwnedMutexGuard};
use tokio::time::Instant;

use super::worker::CallError;
use super::{Collab, DocChange, DocEvent, refusals, taken_from};
use crate::app::App;
use crate::collab_worker::wire::{Answer, DocNumber, Request};
use crate::error::AppError;

/// How long a document that ended its worker twice, the second within the longest
/// closing of the first, is closed the first time; it doubles with each further time.
const FIRST_CLOSING: Duration = Duration::from_secs(10);

/// The longest a document is closed for ending its worker.
const LONGEST_CLOSING: Duration = Duration::from_mins(5);

/// A slot, held by the one request that works on its document.
pub(super) type Held = OwnedMutexGuard<Slot>;

/// What is known of a card's document. A slot is an organization's card's: the same
/// identifier in another organization is another slot.
#[derive(Debug)]
pub(super) struct Slot {
    /// Its key in the place.
    key: String,
    /// The card.
    pub(super) card_id: String,
    /// The organization of the card.
    pub(super) tenant: Tenant,
    /// The slot is no longer in the place: whoever waited for it looks again.
    retired: bool,
    pub(super) open: Option<OpenDoc>,
    /// How many requests about it ended its worker, each a little after the one before
    /// (within the longest closing): a request that ends the worker each time it is
    /// sent again, between which others are answered, is counted.
    deaths: u32,
    /// When the last of them ended it.
    last_death: Option<Instant>,
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
    /// The size of its state when it was made from its body; 0 if it was not here.
    pub(super) made_bytes: u64,
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

/// The key of an organization's card in the place.
fn key_of(tenant: Tenant, card_id: &str) -> String {
    format!("{tenant:?} {card_id}")
}

impl Collab {
    /// The slot of a card of `app`'s organization, held.
    pub(super) async fn slot(&self, app: &App, card_id: &str) -> Held {
        let tenant = app.store().tenant();
        let key = key_of(tenant, card_id);
        loop {
            let shared = {
                let mut slots = self.inner.slots.lock().expect("the slots are not poisoned");
                Arc::clone(slots.entry(key.clone()).or_insert_with(|| {
                    Arc::new(Mutex::new(Slot {
                        key: key.clone(),
                        card_id: card_id.to_owned(),
                        tenant,
                        retired: false,
                        open: None,
                        deaths: 0,
                        last_death: None,
                        closed_until: None,
                    }))
                }))
            };
            let slot = shared.lock_owned().await;
            if !slot.retired {
                return slot;
            }
        }
    }

    /// The slot of a card of `app`'s organization if it has one, held.
    pub(super) async fn existing_slot(&self, app: &App, card_id: &str) -> Option<Held> {
        let key = key_of(app.store().tenant(), card_id);
        loop {
            let shared = {
                let slots = self.inner.slots.lock().expect("the slots are not poisoned");
                Arc::clone(slots.get(&key)?)
            };
            let slot = shared.lock_owned().await;
            if !slot.retired {
                return Some(slot);
            }
        }
    }

    /// Every slot.
    pub(super) fn all_slots(&self) -> Vec<Arc<Mutex<Slot>>> {
        let slots = self.inner.slots.lock().expect("the slots are not poisoned");
        slots.values().map(Arc::clone).collect()
    }

    /// Takes a slot out of the place, unless its document is open or it remembers that
    /// its document is closed for a while.
    pub(super) fn retire(&self, slot: &mut Held) {
        let closed = slot
            .closed_until
            .is_some_and(|until| until > Instant::now());
        if slot.retired || slot.open.is_some() || closed {
            return;
        }
        slot.retired = true;
        let mut slots = self.inner.slots.lock().expect("the slots are not poisoned");
        if slots
            .get(&slot.key)
            .is_some_and(|there| Arc::ptr_eq(there, OwnedMutexGuard::mutex(slot)))
        {
            slots.remove(&slot.key);
        }
    }

    /// Closes the document in its worker, if it is open, and forgets it.
    pub(super) async fn forget(&self, slot: &mut Held) {
        if let Some(open) = slot.open.take() {
            taken_from(&self.inner.loads[open.worker]);
            self.close_number(open.worker, open.generation, open.number)
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
        slot: &mut Held,
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
                Ok((_, answer)) => return Ok(answer),
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

    /// Opens the document if it is not open, unless it is closed for a while. A slot
    /// whose document did not open leaves the place, unless it is closed for a while.
    ///
    /// # Errors
    ///
    /// Returns `document_unavailable` while it is closed, and the errors of opening it.
    pub(super) async fn ensure_open(
        &self,
        app: &App,
        card_id: &str,
        slot: &mut Held,
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
        let error = match self.open_stored(app, card_id).await {
            Ok(open) => {
                slot.open = Some(open);
                return Ok(());
            }
            Err(Opening::Refused(error)) => error,
            Err(Opening::Died) => {
                died_over(slot);
                refusals::unavailable()
            }
        };
        self.retire(slot);
        Err(error)
    }

    /// Forgets a document whose worker is gone.
    pub(super) fn lost(&self, slot: &mut Slot) {
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
/// while from the second (see [`Slot`]).
pub(super) fn died_over(slot: &mut Slot) {
    let now = Instant::now();
    if slot
        .last_death
        .is_some_and(|last| now.duration_since(last) > LONGEST_CLOSING)
    {
        slot.deaths = 0;
    }
    slot.last_death = Some(now);
    slot.deaths = slot.deaths.saturating_add(1);
    if slot.deaths >= 2 {
        let doublings = (slot.deaths - 2).min(16);
        let closing = FIRST_CLOSING
            .saturating_mul(1 << doublings)
            .min(LONGEST_CLOSING);
        slot.closed_until = Some(now + closing);
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
