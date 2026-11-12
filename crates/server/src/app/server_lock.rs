//! One process is the server of a database (`docs/configuration.md`): it holds the lock
//! of the server (`fvoci_store::ServerLock`) from before the start cleans anything up,
//! and the housekeeping runs only while it holds it. A process that cannot take the
//! lock within `server_lock_wait_seconds` does not start, and says who holds it; one
//! that loses it is not ready, stops its housekeeping, and goes down.

use std::sync::{Arc, Weak};
use std::time::Duration;

use anyhow::bail;
use fvoci_store::{ServerLock, Store};
use tokio::sync::watch;
use tokio::time::Instant;

use super::App;
use crate::config::Configuration;
use crate::error::AppError;

/// How often the lock is looked at while it is held.
const CHECK_EVERY: Duration = Duration::from_secs(5);

/// How long a look at the lock may take before the lock is taken for lost.
const CHECK_TIMEOUT: Duration = Duration::from_secs(10);

/// How often a process that waits for the lock tries to take it.
const TRY_EVERY: Duration = Duration::from_millis(250);

/// Whether this process holds the lock of the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Holding {
    /// Not taken, or let go at shutdown.
    No,
    /// Held.
    Held,
    /// It was held and is lost: the process goes down.
    Lost,
}

/// The holding of the lock, as every handle of the application sees it.
#[derive(Debug)]
pub(super) struct Holder(watch::Sender<Holding>);

impl Default for Holder {
    fn default() -> Self {
        Self(watch::Sender::new(Holding::No))
    }
}

/// The lock of the server, held, and the task that keeps looking at it. Dropped, it
/// lets the lock go, and the task ends.
#[derive(Debug)]
pub(crate) struct HeldLock {
    lock: Arc<ServerLock>,
    watching: tokio::task::JoinHandle<()>,
}

impl App {
    /// Whether this process holds the lock of the server of its database: from when a
    /// server took it at its start until it lets it go or loses it.
    pub fn holds_server_lock(&self) -> bool {
        *self.inner.server_lock.0.borrow() == Holding::Held
    }

    /// Refuses the work of the server of the database (the housekeeping, the clean-up
    /// at the start) unless this process holds the lock.
    ///
    /// # Errors
    ///
    /// Returns a 503 error if the lock is not held.
    pub(crate) fn require_server_lock(&self) -> Result<(), AppError> {
        if self.holds_server_lock() {
            Ok(())
        } else {
            Err(AppError::service_unavailable(
                "this process does not hold the lock of the server of its database",
            ))
        }
    }

    /// Returns once the lock, held, is lost; never if it is not lost.
    pub(crate) async fn server_lock_lost(&self) {
        let mut holding = self.inner.server_lock.0.subscribe();
        // The sender lives as long as the application, which outlives this future.
        let _ = holding.wait_for(|holding| *holding == Holding::Lost).await;
    }

    /// Whether the database answers, and this process holds the lock of the server in
    /// it: what the readiness asks of the database.
    pub(super) async fn ping_as_the_server(&self) -> Result<(), String> {
        if !self.holds_server_lock() {
            return Err("this process does not hold the lock of the server".to_owned());
        }
        self.store().ping().await.map_err(|error| error.to_string())
    }

    /// Holds `lock`, taken with [`take_server_lock`] for this application's database,
    /// and keeps looking at it until it is let go ([`App::release_server_lock`]).
    pub(crate) fn hold_server_lock(&self, lock: ServerLock) -> HeldLock {
        let lock = Arc::new(lock);
        self.inner.server_lock.0.send_replace(Holding::Held);
        let watching = tokio::spawn(watch_lock(self.clone(), Arc::downgrade(&lock)));
        HeldLock { lock, watching }
    }

    /// Lets the lock go, at shutdown, once the housekeeping stopped.
    pub(crate) async fn release_server_lock(&self, held: HeldLock) {
        self.inner.server_lock.0.send_if_modified(|holding| {
            let was_held = *holding == Holding::Held;
            if was_held {
                *holding = Holding::No;
            }
            was_held
        });
        let HeldLock { lock, watching } = held;
        watching.abort();
        // A look under way holds the lock for as long as it takes.
        let _ = watching.await;
        if let Ok(lock) = Arc::try_unwrap(lock) {
            lock.release().await;
        }
    }
}

/// Takes the lock of the server of the database, trying for as long as
/// `server_lock_wait_seconds` says: a server that is restarted waits for the one that
/// is going down to let it go. Taken before the schema is brought up to date, so that a
/// start that is refused changes nothing.
///
/// # Errors
///
/// Fails if the lock cannot be taken in that time, naming the process that holds it,
/// or if the database cannot be reached.
pub(crate) async fn take_server_lock(
    store: &Store,
    config: &Configuration,
) -> anyhow::Result<ServerLock> {
    let holder = format!("fvoci-server process {}", std::process::id());
    let wait = config.server_lock_wait();
    let until = Instant::now() + wait;
    let mut said = false;
    loop {
        if let Some(lock) = store.try_lock_server(&holder).await? {
            return Ok(lock);
        }
        if Instant::now() >= until {
            let holder = store
                .server_lock_holder()
                .await
                .ok()
                .flatten()
                .unwrap_or_else(|| "a process that let it go just now".to_owned());
            bail!(
                "another process is the server of this database: {holder} holds its lock, \
                 and it was not let go in {} seconds (server_lock_wait_seconds). One server \
                 serves a database",
                wait.as_secs()
            );
        }
        if !said {
            tracing::warn!(
                seconds = wait.as_secs(),
                "another process holds the lock of the server of this database; waiting for it"
            );
            said = true;
        }
        tokio::time::sleep(TRY_EVERY.min(until.saturating_duration_since(Instant::now()))).await;
    }
}

/// Looks at the lock every [`CHECK_EVERY`] while it is held, and marks it lost when the
/// database says it is not held or does not answer in [`CHECK_TIMEOUT`]. Ends once the
/// lock is let go.
async fn watch_lock(app: App, lock: Weak<ServerLock>) {
    loop {
        tokio::time::sleep(CHECK_EVERY).await;
        let Some(lock) = lock.upgrade() else {
            return;
        };
        let error = match tokio::time::timeout(CHECK_TIMEOUT, lock.is_held()).await {
            Ok(Ok(true)) => continue,
            Ok(Ok(false)) => "the database says it is not held".to_owned(),
            Ok(Err(error)) => error.to_string(),
            Err(_) => "the database did not answer in time".to_owned(),
        };
        tracing::error!(
            %error,
            "the lock of the server is lost: the housekeeping stops and the server goes down"
        );
        app.inner.server_lock.0.send_replace(Holding::Lost);
        return;
    }
}
