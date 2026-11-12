//! Editing a card's body together: the one place that holds the Y document of each
//! body being edited, in a worker process ([`worker`]), and stores it.
//!
//! What a room of editors (the websocket, a later piece of work) calls:
//!
//! - [`Collab::open`]: the whole document, opened from what is stored, or made from the
//!   card's body the first time (by the server, never by a client).
//! - [`Collab::take`]: takes an editor's update, logs it, and only then gives it back,
//!   with what a repair deleted, to be sent to the other editors.
//! - [`Collab::state_vector`], [`Collab::diff`]: what an editor that comes back is
//!   missing.
//! - [`Collab::write_body`], [`Collab::close`]: writes the document's body to the
//!   card now; closes the document after that.
//! - [`Collab::subscribe`]: what happens to documents that editors have to know of
//!   ([`DocChange`]).
//!
//! An update is taken by the worker into a copy of the document, logged in
//! `body_updates`, and then given back: nothing is sent to anybody that the log does
//! not have. The body of the document is written to the card (`blocks.body`, which
//! everything else reads) two seconds after the last update, ten seconds after the
//! first one that is not written at the latest, and when the document is closed or the
//! server stops. The log is folded into the stored state every [`FOLD_AFTER`] updates;
//! a document whose state comes near its limit is made again from its body. A server
//! that ended before it wrote a body writes it when it starts again
//! ([`Collab::recover`]).

#[cfg(unix)]
mod child;
mod open;
mod place;
mod refusals;
mod upkeep;
mod whole;
pub mod worker;

pub use whole::cards_written_whole;

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use fvoci_store::Tenant;
use tokio::sync::broadcast;

use crate::app::App;
use crate::collab_worker::wire::{Answer, Limits, Refusal, Refused, Request};
use crate::config::CollabSettings;
use crate::error::AppError;
use place::{Held, Slot};
use worker::{Worker, WorkerCommand, WorkerSettings};

/// How many updates the log of a document holds before they are folded into its state.
pub const FOLD_AFTER: i64 = 200;

/// How long after a document could not be made again from its body it is tried again.
const REMAKE_RETRY: Duration = Duration::from_mins(1);

/// How long after the last update the body of a document is written to its card.
pub const WRITE_QUIET: Duration = Duration::from_secs(2);

/// How long after the first update that is not written the body is written at the
/// latest, whatever comes after it.
pub const WRITE_LONGEST: Duration = Duration::from_secs(10);

/// How many events a subscriber may fall behind before it misses some.
const EVENTS: usize = 256;

/// Runs the executable as the worker ([`crate::collab_worker`]) if its first argument
/// says it was started as one: `None` if it was not, else how the worker ended. The
/// executable does nothing else then: no configuration, runtime or log.
#[must_use]
pub fn run_as_worker() -> Option<anyhow::Result<()>> {
    crate::collab_worker::run_if_started_as_worker()
        .map(|ended| ended.map_err(|error| anyhow::anyhow!("the collab worker ended: {error}")))
}

/// The place of the documents of the bodies being edited. Cloning is cheap; every clone
/// is the same place.
#[derive(Debug, Clone)]
pub struct Collab {
    inner: Arc<Inner>,
}

#[derive(Debug)]
struct Inner {
    workers: Vec<Worker>,
    /// How many documents each worker was given, of those that are open.
    loads: Vec<AtomicUsize>,
    /// The documents that are open, or were and are closed for a while, by card.
    slots: Mutex<HashMap<String, Arc<tokio::sync::Mutex<Slot>>>>,
    /// The next number a document is opened under in a worker.
    numbers: AtomicU64,
    limits: Limits,
    events: broadcast::Sender<DocEvent>,
    /// What the next write of a body waits for, between reading its card and writing it:
    /// for the tests of what is written to the card meanwhile.
    #[cfg(feature = "test-util")]
    projection_pause: Mutex<Option<(Arc<tokio::sync::Notify>, Arc<tokio::sync::Notify>)>>,
}

/// An update a document took.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Taken {
    /// Its number in the document's log.
    pub seq: i64,
    /// What a repair deleted so that the document stays a body, as an update for every
    /// editor, the one that sent the update too; logged after it.
    pub repair: Option<Vec<u8>>,
    /// Whether the document was made again from its body after it (its state came near
    /// its limit): every editor loads it anew ([`DocChange::Remade`] says so as well),
    /// and the update and its repair, made against the document before, are not sent
    /// to anyone.
    pub remade: bool,
}

/// Something that happened to a document that the editors who hold it have to know.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct DocEvent {
    /// The organization of the card: the same identifier in another organization is
    /// another card.
    pub tenant: Tenant,
    /// The card.
    pub card_id: String,
    /// What happened.
    pub change: DocChange,
}

/// What happened to a document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum DocChange {
    /// It was made again from its body: an update made against the one before is
    /// refused (reason `update_out_of_step`). Editors load it anew.
    Remade,
    /// Its body was written whole over HTTP, and the document made again from it.
    /// Editors load it anew.
    Replaced,
    /// The worker that held it ended, and it was opened again from what was stored.
    /// What an editor sent and was refused for that is not in it.
    Reopened,
}

impl Collab {
    /// The place, with workers started as `settings` say when they are first needed.
    ///
    /// # Errors
    ///
    /// Returns why the executable to start the workers from is not known.
    pub fn new(settings: &CollabSettings) -> anyhow::Result<Self> {
        let command = match &settings.worker_command {
            Some(command) => command.clone(),
            None => WorkerCommand::this_executable()?,
        };
        let limits = Limits {
            update_bytes: settings.collab_update_bytes,
            state_bytes: settings.collab_state_bytes,
            body_bytes: u64::try_from(fvoci_model::BODY_MAX_BYTES).unwrap_or(u64::MAX),
            documents: u64::from(settings.collab_documents_per_worker),
            held_bytes: settings.held_bytes(),
        };
        let worker_settings = WorkerSettings {
            command,
            limits,
            timeout: Duration::from_millis(settings.collab_worker_timeout_ms),
            memory_bytes: settings.collab_worker_memory_bytes,
        };
        let count = usize::try_from(settings.collab_workers).unwrap_or(1).max(1);
        Ok(Self {
            inner: Arc::new(Inner {
                workers: (0..count)
                    .map(|_| Worker::new(worker_settings.clone()))
                    .collect(),
                loads: (0..count).map(|_| AtomicUsize::new(0)).collect(),
                slots: Mutex::new(HashMap::new()),
                numbers: AtomicU64::new(1),
                limits,
                events: broadcast::channel(EVENTS).0,
                #[cfg(feature = "test-util")]
                projection_pause: Mutex::new(None),
            }),
        })
    }

    /// What happens to documents from now on.
    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<DocEvent> {
        self.inner.events.subscribe()
    }

    /// Runs `work` on a card's slot, held, in a task of its own: what a caller that
    /// stops waiting leaves is a step done whole, never half of one (an update taken
    /// and not logged, a document made and not counted). Shutdown waits for it.
    async fn on_slot<T, W, F>(&self, app: &App, card_id: &str, work: W) -> Result<T, AppError>
    where
        T: Send + 'static,
        W: FnOnce(Self, App, String, Held) -> F + Send + 'static,
        F: Future<Output = Result<T, AppError>> + Send + 'static,
    {
        let (collab, app_of_task, card_id) = (self.clone(), app.clone(), card_id.to_owned());
        let task = app.spawn_tracked(async move {
            let slot = collab.slot(&app_of_task, &card_id).await;
            work(collab, app_of_task, card_id, slot).await
        });
        task.await.map_err(|error| {
            AppError::internal(format_args!("the work on a document ended: {error}"))
        })?
    }

    /// The whole document of a card's body, as a version 1 update, opened if it is not
    /// open: from what is stored, or made from the card's body the first time.
    ///
    /// # Errors
    ///
    /// Returns a not-found error if `app` reaches no card of that identifier, the
    /// reasons of [`crate::error::Reason`] for the document (too many open,
    /// unavailable), and the store's errors.
    pub async fn open(&self, app: &App, card_id: &str) -> Result<Vec<u8>, AppError> {
        self.on_slot(
            app,
            card_id,
            move |collab, app, card_id, mut slot| async move {
                let (collab, app, card_id, slot) = (&collab, &app, card_id.as_str(), &mut slot);
                bytes(collab.ask(app, card_id, slot, Request::State).await?)
            },
        )
        .await
    }

    /// The state vector of a card's document, version 1, opened if it is not open.
    ///
    /// # Errors
    ///
    /// As [`Collab::open`].
    pub async fn state_vector(&self, app: &App, card_id: &str) -> Result<Vec<u8>, AppError> {
        self.on_slot(
            app,
            card_id,
            move |collab, app, card_id, mut slot| async move {
                let (collab, app, card_id, slot) = (&collab, &app, card_id.as_str(), &mut slot);
                bytes(collab.ask(app, card_id, slot, Request::StateVector).await?)
            },
        )
        .await
    }

    /// What a card's document has that a holder of `state_vector` has not, as a version
    /// 1 update.
    ///
    /// # Errors
    ///
    /// As [`Collab::open`], and a refusal (`update_refused`) for bytes that are no state
    /// vector.
    pub async fn diff(
        &self,
        app: &App,
        card_id: &str,
        state_vector: &[u8],
    ) -> Result<Vec<u8>, AppError> {
        self.held_to_update_bytes(state_vector)?;
        let state_vector = state_vector.to_vec();
        self.on_slot(
            app,
            card_id,
            move |collab, app, card_id, mut slot| async move {
                let (collab, app, card_id, slot) = (&collab, &app, card_id.as_str(), &mut slot);
                let request = |doc| Request::Diff {
                    doc,
                    state_vector: state_vector.clone(),
                };
                bytes(collab.ask(app, card_id, slot, request).await?)
            },
        )
        .await
    }

    /// Takes an editor's update into a card's document, by `user_id`, and logs it; what
    /// comes back may be sent to the other editors, the update first and then the
    /// repair. Opens the document if it is not open.
    ///
    /// # Errors
    ///
    /// As [`Collab::open`], and the refusals of an update (`update_refused`,
    /// `update_out_of_step`, `update_too_large`, `document_too_large`). A refused update
    /// is in neither the document nor the log. So is one under way when the worker
    /// ended (`document_unavailable`).
    pub async fn take(
        &self,
        app: &App,
        card_id: &str,
        user_id: &str,
        update: &[u8],
    ) -> Result<Taken, AppError> {
        self.held_to_update_bytes(update)?;
        let (user_id, update) = (user_id.to_owned(), update.to_vec());
        self.on_slot(
            app,
            card_id,
            move |collab, app, card_id, mut slot| async move {
                let (collab, app, card_id, slot) = (&collab, &app, card_id.as_str(), &mut slot);
                collab.take_in(app, card_id, slot, &user_id, &update).await
            },
        )
        .await
    }

    /// Writes the body of a card's document to the card now, if it has updates whose
    /// body the card does not hold. Nothing if the document is not open.
    ///
    /// # Errors
    ///
    /// Returns the errors of reading the body from the worker and of writing it.
    pub async fn write_body(&self, app: &App, card_id: &str) -> Result<(), AppError> {
        self.on_slot(
            app,
            card_id,
            move |collab, app, card_id, mut slot| async move {
                let (collab, app, card_id, slot) = (&collab, &app, card_id.as_str(), &mut slot);
                let written = collab.write_now(app, card_id, slot).await;
                collab.retire(slot);
                written
            },
        )
        .await
    }

    /// Writes the body of a card's document to the card and closes the document. The
    /// next request opens it again from what is stored. A body that cannot be written
    /// now stays in the log and is written when the document is opened again, or when
    /// the server starts: the document is closed all the same.
    ///
    /// # Errors
    ///
    /// Returns why the body could not be written.
    pub async fn close(&self, app: &App, card_id: &str) -> Result<(), AppError> {
        self.on_slot(
            app,
            card_id,
            move |collab, app, card_id, mut slot| async move {
                let (collab, app, card_id, slot) = (&collab, &app, card_id.as_str(), &mut slot);
                let written = collab.write_now(app, card_id, slot).await;
                collab.forget(slot).await;
                collab.retire(slot);
                written
            },
        )
        .await
    }

    /// Writes the body of every open document and closes them, and ends the workers:
    /// for a server that stops. `app` is any handle of the server: each document is
    /// written as its card's organization.
    pub async fn end(&self, app: &App) {
        for slot in self.all_slots() {
            let mut slot = slot.lock_owned().await;
            let app = app.scoped(slot.tenant);
            if let Some(card_id) = slot.open.is_some().then(|| slot.card_id.clone()) {
                if let Err(error) = self.write_now(&app, &card_id, &mut slot).await {
                    tracing::error!(%error, card_id, "cannot write a card's body as the server stops");
                }
                self.forget(&mut slot).await;
            }
            self.retire(&mut slot);
        }
        for worker in &self.inner.workers {
            worker.end().await;
        }
    }

    /// Writes the bodies of the documents that a server that ended did not write, each
    /// as its organization: for a server that starts. `app` reaches every organization.
    /// What cannot be written is logged and left for when the document is opened.
    pub async fn recover(&self, app: &App) {
        // Writes to cards: only the server of the database does.
        if let Err(error) = app.require_server_lock() {
            tracing::warn!(%error, "the bodies a run that ended did not write are not written");
            return;
        }
        let unwritten = match app.store().unwritten_body_docs().await {
            Ok(unwritten) => unwritten,
            Err(error) => {
                tracing::error!(%error, "cannot read which bodies are not written");
                return;
            }
        };
        for (card_id, org_id) in unwritten {
            let tenant = match Tenant::org(&org_id) {
                Ok(tenant) => tenant,
                Err(error) => {
                    tracing::error!(%error, card_id, "the organization of a document is no identifier");
                    continue;
                }
            };
            match self.close_after_open(&app.scoped(tenant), &card_id).await {
                Ok(()) => tracing::info!(card_id, "wrote a body the server had not written"),
                Err(error) => {
                    tracing::error!(%error, card_id, "cannot write a body the server had not written");
                }
            }
        }
    }

    /// Opens a card's document, writes its body and closes it.
    async fn close_after_open(&self, app: &App, card_id: &str) -> Result<(), AppError> {
        self.on_slot(
            app,
            card_id,
            move |collab, app, card_id, mut slot| async move {
                let (collab, app, card_id, slot) = (&collab, &app, card_id.as_str(), &mut slot);
                collab.ensure_open(app, card_id, slot).await?;
                let written = collab.write_now(app, card_id, slot).await;
                collab.forget(slot).await;
                collab.retire(slot);
                written
            },
        )
        .await
    }

    /// The process identifiers of the running workers, for the tests of their deaths.
    #[cfg(feature = "test-util")]
    #[doc(hidden)]
    pub async fn worker_pids(&self) -> Vec<Option<u32>> {
        let mut pids = Vec::new();
        for worker in &self.inner.workers {
            pids.push(worker.pid().await);
        }
        pids
    }

    /// Has the first worker take `millis` milliseconds over a request, for the tests
    /// of a worker that does not answer in time.
    ///
    /// # Errors
    ///
    /// Returns how the request ended.
    #[cfg(feature = "test-util")]
    #[doc(hidden)]
    pub async fn stall_worker(&self, millis: u64) -> Result<(), worker::CallError> {
        self.inner.workers[0]
            .call(None, &Request::Stall(millis))
            .await
            .map(|_| ())
    }

    /// Has the worker that holds a card's document take `millis` milliseconds over a
    /// request about it, opening the document first: for the tests of a document whose
    /// requests end its worker.
    ///
    /// # Errors
    ///
    /// As a request about the document.
    #[cfg(feature = "test-util")]
    #[doc(hidden)]
    pub async fn stall_document(
        &self,
        app: &App,
        card_id: &str,
        millis: u64,
    ) -> Result<(), AppError> {
        self.on_slot(
            app,
            card_id,
            move |collab, app, card_id, mut slot| async move {
                let (collab, app, card_id, slot) = (&collab, &app, card_id.as_str(), &mut slot);
                collab
                    .ask(app, card_id, slot, |_| Request::Stall(millis))
                    .await
                    .map(|_| ())
            },
        )
        .await
    }

    /// Refuses what an editor sends that is larger than an update may be, before it is
    /// copied into a frame for the worker (which refuses it as well).
    fn held_to_update_bytes(&self, sent: &[u8]) -> Result<(), AppError> {
        let limit = usize::try_from(self.inner.limits.update_bytes).unwrap_or(usize::MAX);
        if sent.len() <= limit {
            return Ok(());
        }
        let refused = Refused {
            refusal: Refusal::UpdateTooLarge,
            message: format!("{} bytes, before the worker", sent.len()),
        };
        Err(refusals::refused(&refused, &self.inner.limits))
    }

    /// Has the next write of a document's body to its card wait, after it read the card
    /// and before it writes it, until the second is notified; the first is notified when
    /// it waits. For the tests of what is written to the card meanwhile.
    #[cfg(feature = "test-util")]
    #[doc(hidden)]
    #[must_use]
    pub fn pause_next_projection(&self) -> (Arc<tokio::sync::Notify>, Arc<tokio::sync::Notify>) {
        let pause = (
            Arc::new(tokio::sync::Notify::new()),
            Arc::new(tokio::sync::Notify::new()),
        );
        *self
            .inner
            .projection_pause
            .lock()
            .expect("the pause is not poisoned") = Some(pause.clone());
        pause
    }

    /// Waits where [`Collab::pause_next_projection`] asked.
    #[cfg(feature = "test-util")]
    async fn paused_before_projection(&self) {
        let pause = self
            .inner
            .projection_pause
            .lock()
            .expect("the pause is not poisoned")
            .take();
        if let Some((waiting, go)) = pause {
            waiting.notify_one();
            go.notified().await;
        }
    }

    /// Forgets every open document without writing its body, and ends the workers: as
    /// a server that is killed leaves them, for the tests of what a server that starts
    /// again makes of that.
    #[cfg(feature = "test-util")]
    #[doc(hidden)]
    pub async fn abandon(&self) {
        for slot in self.all_slots() {
            let mut slot = slot.lock_owned().await;
            self.lost(&mut slot);
            self.retire(&mut slot);
        }
        for worker in &self.inner.workers {
            worker.end().await;
        }
    }
}

/// The bytes of an answer that carries them.
fn bytes(answer: Answer) -> Result<Vec<u8>, AppError> {
    match answer {
        Answer::Bytes(bytes) => Ok(bytes),
        other => Err(AppError::internal(format_args!(
            "the worker answered with no bytes: {other:?}"
        ))),
    }
}

/// Counts a document given to a worker.
fn given(load: &AtomicUsize) {
    load.fetch_add(1, Ordering::Relaxed);
}

/// Counts a document taken from a worker.
fn taken_from(load: &AtomicUsize) {
    let _ = load.try_update(Ordering::Relaxed, Ordering::Relaxed, |count| {
        count.checked_sub(1)
    });
}
