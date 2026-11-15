//! The place of each card's document: one at a time works on it, and what is known of
//! it while it is open.

use std::sync::Arc;
use std::time::Duration;

use fvoci_store::Tenant;
use tokio::sync::{Mutex, OwnedMutexGuard, OwnedSemaphorePermit};
use tokio::time::Instant;

use super::versions::Versioned;
use super::worker::CallError;
use super::{Collab, DocChange, DocEvent, refusals, taken_from};
use crate::app::App;
use crate::collab_worker::wire::{Answer, DocNumber, Request, Share};
use crate::error::AppError;

/// How long a document that ended its worker twice, the second within
/// [`DEATHS_REMEMBERED`] of the first, is closed the first time; it doubles with each
/// further time.
const FIRST_CLOSING: Duration = Duration::from_secs(10);

/// The longest a document is closed for ending its worker.
const LONGEST_CLOSING: Duration = Duration::from_mins(5);

/// How long a death of the worker over a document is remembered: the deaths within it
/// of the one before are counted together. Longer than the longest closing, so that a
/// document closed that long is closed again at its next death.
const DEATHS_REMEMBERED: Duration = Duration::from_mins(15);

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
    /// (within [`DEATHS_REMEMBERED`]): a request that ends the worker each time it is
    /// sent again, between which others are answered, is counted, and so is an opening
    /// that ends it. The slot stays in the place while it remembers a death.
    deaths: u32,
    /// When the last of them ended it.
    last_death: Option<Instant>,
    /// Until when it is closed for that.
    closed_until: Option<Instant>,
    /// When it could not be made again from its body last.
    pub(super) remake_failed_at: Option<Instant>,
}

/// A document that a worker holds.
#[derive(Debug)]
pub(super) struct OpenDoc {
    /// Which worker, of which generation, under which number.
    pub(super) worker: usize,
    pub(super) generation: u64,
    pub(super) number: DocNumber,
    /// Which making of the document it is ([`super::Loaded::epoch`]).
    pub(super) epoch: i64,
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
    /// Not before when its body is written again, after a write that failed.
    pub(super) retry_at: Option<Instant>,
    /// Whether its body is kept as a version of its card ([`super::versions`]).
    pub(super) versioned: Versioned,
    /// Who is known to be put down as having written the body since its newest
    /// version: the first update of anybody else puts them down.
    pub(super) editors: Vec<String>,
    /// Opened aside ([`Share::Aside`]), only to write its log to its card: its place
    /// in the server's share aside, given back as it is closed. `None` for a document
    /// counted in its worker's load.
    pub(super) aside: Option<OwnedSemaphorePermit>,
}

impl OpenDoc {
    /// The share of its worker's documents it is counted in.
    pub(super) fn share(&self) -> Share {
        if self.aside.is_some() {
            Share::Aside
        } else {
            Share::Kept
        }
    }
}

impl OpenDoc {
    /// When its body is to be written, if it has updates whose body is not.
    pub(super) fn write_at(&self) -> Option<Instant> {
        let first = self.first_unwritten?;
        let due = (self.last_update + super::WRITE_QUIET).min(first + super::WRITE_LONGEST);
        Some(self.retry_at.map_or(due, |retry_at| due.max(retry_at)))
    }
}

/// The key of an organization's card in the place.
pub(super) fn key_of(tenant: Tenant, card_id: &str) -> String {
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
                        remake_failed_at: None,
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

    /// How many bytes the state of a card's open document is, as its worker counts it
    /// against what it may hold; `None` if it is not open. For the tests that measure.
    #[cfg(feature = "test-util")]
    #[doc(hidden)]
    pub async fn held_state_bytes(&self, app: &App, card_id: &str) -> Option<u64> {
        let slot = self.existing_slot(app, card_id).await?;
        slot.open.as_ref().map(|open| open.state_bytes)
    }

    /// Every slot.
    pub(super) fn all_slots(&self) -> Vec<Arc<Mutex<Slot>>> {
        let slots = self.inner.slots.lock().expect("the slots are not poisoned");
        slots.values().map(Arc::clone).collect()
    }

    /// Takes a slot out of the place, unless its document is open, or it remembers that
    /// its document is closed for a while or that it ended its worker.
    pub(super) fn retire(&self, slot: &mut Held) {
        let now = Instant::now();
        let closed = slot.closed_until.is_some_and(|until| until > now);
        let remembered = slot
            .last_death
            .is_some_and(|last| now.duration_since(last) <= DEATHS_REMEMBERED);
        if slot.retired || slot.open.is_some() || closed || remembered {
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
            if open.aside.is_none() {
                taken_from(&self.inner.loads[open.worker]);
            }
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
                    let aside = open.aside.is_some();
                    self.lost(slot);
                    self.tell(app, card_id, DocChange::Reopened);
                    if aside {
                        // Opened aside for its log alone: not opened again in the kept
                        // share, which may be full; the log stays for the next try.
                        return Err(refusals::unavailable());
                    }
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
        if closed_for_a_while(slot) {
            return Err(refusals::unavailable());
        }
        let error = match self.open_stored(app, card_id, None).await {
            Ok(open) => {
                let past_limit = open.state_bytes > self.inner.limits.state_bytes;
                slot.open = Some(open);
                if past_limit {
                    // Stored or logged under a larger limit: its body is written to the
                    // card, and it is made again from that.
                    // Boxed: the remake asks the open document, through here.
                    Box::pin(self.remake_and_tell(app, card_id, slot)).await;
                    if slot.open.is_none() {
                        // The remake failed and closed it: the store holds it.
                        return Err(refusals::unavailable());
                    }
                }
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

    /// Opens the document aside ([`Share::Aside`]), only to write its log to its card:
    /// within the server's share aside, waiting for a place in it, so that a server
    /// that holds as many documents as it may still writes a log. Closed by the one
    /// that opened it ([`Collab::forget`]) before the slot is let go.
    ///
    /// # Errors
    ///
    /// As [`Collab::ensure_open`].
    pub(super) async fn open_aside(
        &self,
        app: &App,
        card_id: &str,
        slot: &mut Held,
    ) -> Result<(), AppError> {
        if closed_for_a_while(slot) {
            return Err(refusals::unavailable());
        }
        let place = Arc::clone(&self.inner.aside)
            .acquire_owned()
            .await
            .map_err(|_| refusals::unavailable())?;
        match self.open_stored(app, card_id, Some(place)).await {
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

    /// Opens the document if it is not open, and refuses a request made against
    /// another making of it than `epoch` (`update_out_of_step`).
    ///
    /// # Errors
    ///
    /// As [`Collab::ensure_open`], and `update_out_of_step`.
    pub(super) async fn at_epoch(
        &self,
        app: &App,
        card_id: &str,
        slot: &mut Held,
        epoch: i64,
    ) -> Result<(), AppError> {
        self.ensure_open(app, card_id, slot).await?;
        still_at_epoch(slot, epoch)
    }

    /// Forgets a document whose worker is gone.
    pub(super) fn lost(&self, slot: &mut Slot) {
        if let Some(open) = slot.open.take()
            && open.aside.is_none()
        {
            taken_from(&self.inner.loads[open.worker]);
        }
    }

    /// Tells the subscribers what happened to a card's document.
    pub(super) fn tell(&self, app: &App, card_id: &str, change: DocChange) {
        // Nobody may be listening.
        let _ = self.inner.events.send(DocEvent {
            tenant: app.store().tenant(),
            card_id: card_id.to_owned(),
            change,
        });
    }
}

/// Whether the document is closed for a while, after its worker ended over it.
fn closed_for_a_while(slot: &Slot) -> bool {
    slot.closed_until
        .is_some_and(|until| until > Instant::now())
}

/// Counts a death of the worker over the document, and closes the document for a
/// while from the second (see [`Slot`]).
pub(super) fn died_over(slot: &mut Slot) {
    let now = Instant::now();
    if slot
        .last_death
        .is_some_and(|last| now.duration_since(last) > DEATHS_REMEMBERED)
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

/// Refuses a request made against another making of the document than `epoch`: one the
/// document was made again under while the request was asked (a worker that ended,
/// whose document was opened again from a card written past it).
///
/// # Errors
///
/// Returns `update_out_of_step`.
pub(super) fn still_at_epoch(slot: &Slot, epoch: i64) -> Result<(), AppError> {
    if slot.open.as_ref().is_some_and(|open| open.epoch == epoch) {
        Ok(())
    } else {
        Err(refusals::out_of_step())
    }
}
