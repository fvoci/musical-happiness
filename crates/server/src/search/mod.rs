//! The search of what cards say, through a search server that is set up apart
//! (`search_url`): Elasticsearch or `OpenSearch`, with the nori analysis plugin.
//!
//! The browser never reaches the search server. The server keeps one index there; a
//! search asks it for documents of the user's workspace, and then asks the database
//! about every document that comes back ([`App::search_workspace`]): the index may be
//! behind what is stored, and what it says is never an answer by itself.
//!
//! Without `search_url` there is no search of bodies, and nothing stands in for it.

mod client;
mod document;
mod find;
mod index;
mod limit;
mod query;

use std::sync::Mutex;
use std::time::{Duration, Instant};

pub use find::{Asked, MAX_LIMIT, MAX_TEXT_CHARS};

use client::{Client, ClientError, IndexFound};
use limit::Limiter;

use crate::app::App;
use crate::config::search::SearchSettings;
use crate::scheduler::RecurringTask;

/// How often the upkeep of the search runs.
const UPKEEP_INTERVAL: Duration = Duration::from_secs(1);
/// How long the upkeep leaves a search server that did not answer alone, at first
/// and at most; doubled with every failure in a row.
const FIRST_PAUSE: Duration = Duration::from_secs(1);
/// See [`FIRST_PAUSE`].
const LONGEST_PAUSE: Duration = Duration::from_secs(60);

/// The search of bodies, as the server is configured: on a search server, or none.
#[derive(Debug)]
pub struct Search {
    engine: Option<Engine>,
}

/// The search server and what is known of the index on it.
#[derive(Debug)]
struct Engine {
    client: Client,
    limiter: Limiter,
    index: Mutex<IndexState>,
    /// Held by the upkeep while it runs: one at a time.
    upkeep: tokio::sync::Mutex<Pause>,
}

/// What is known of the index on the search server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IndexState {
    /// Nothing yet: the search server has not answered.
    Unknown,
    /// It is there, made from this server's definition.
    Ready,
    /// The search server cannot hold it. Bodies are not searched until the server is
    /// started again.
    Refused,
}

/// How long the upkeep leaves the search server alone after it failed to answer.
#[derive(Debug, Default)]
struct Pause {
    until: Option<Instant>,
    failures: u32,
}

impl Pause {
    fn is_over(&self, now: Instant) -> bool {
        self.until.is_none_or(|until| now >= until)
    }

    /// Notes a failure at `now`. Returns whether it is the first in a row.
    fn failed(&mut self, now: Instant) -> bool {
        let pause = FIRST_PAUSE
            .saturating_mul(2_u32.saturating_pow(self.failures))
            .min(LONGEST_PAUSE);
        self.until = Some(now + pause);
        self.failures = self.failures.saturating_add(1);
        self.failures == 1
    }
}

impl Search {
    /// The search the configuration asks for.
    ///
    /// # Errors
    ///
    /// Returns an error if the keys of the search server do not hold
    /// ([`SearchSettings::server`]) or its client cannot be set up.
    pub fn from_config(settings: &SearchSettings) -> anyhow::Result<Self> {
        let Some(server) = settings.server()? else {
            return Ok(Self { engine: None });
        };
        Ok(Self {
            engine: Some(Engine {
                client: Client::new(&server)?,
                limiter: Limiter::new(server.requests_per_second),
                index: Mutex::new(IndexState::Unknown),
                upkeep: tokio::sync::Mutex::default(),
            }),
        })
    }

    /// Whether bodies can be searched on this server: a search server is configured,
    /// and has not turned out to be unable to hold the index.
    #[must_use]
    pub fn enabled(&self) -> bool {
        self.engine().is_some()
    }

    /// The search server, if bodies can be searched.
    fn engine(&self) -> Option<&Engine> {
        self.engine
            .as_ref()
            .filter(|engine| engine.index_state() != IndexState::Refused)
    }
}

impl Engine {
    fn index_state(&self) -> IndexState {
        *self
            .index
            .lock()
            .expect("the lock of the index state is not poisoned")
    }

    fn set_index_state(&self, state: IndexState) {
        *self
            .index
            .lock()
            .expect("the lock of the index state is not poisoned") = state;
    }

    /// Sees that the index is on the search server, made from this server's
    /// definition: makes it where there is none, and makes it again where it was made
    /// from another version.
    async fn ensure_index(&self) -> Result<IndexState, ClientError> {
        match self.client.describe_index().await? {
            IndexFound::Ours { version } if version == index::VERSION => Ok(IndexState::Ready),
            IndexFound::Ours { version } => {
                tracing::info!(
                    from = version,
                    to = index::VERSION,
                    "the search index was made from another definition; making it again"
                );
                self.client.delete_index().await?;
                self.client.create_index().await?;
                Ok(IndexState::Ready)
            }
            IndexFound::Absent => {
                self.client.create_index().await?;
                tracing::info!("made the search index");
                Ok(IndexState::Ready)
            }
            IndexFound::Foreign => {
                tracing::error!(
                    "search_index names an index this server did not make; it is left as \
                     it is, and bodies are not searched: name another index, or remove it"
                );
                Ok(IndexState::Refused)
            }
        }
    }
}

/// Does the upkeep of the search once, now: what its task does every interval. Until
/// the index is known to be there, it asks the search server for it, leaving a server
/// that does not answer alone for a while. The server does it by itself while it
/// serves; this is for its tests.
pub async fn tend(app: &App) {
    let Some(engine) = app.search().engine() else {
        return;
    };
    let mut pause = engine.upkeep.lock().await;
    if engine.index_state() != IndexState::Unknown || !pause.is_over(Instant::now()) {
        return;
    }
    if let Err(error) = app.require_server_lock() {
        tracing::warn!(%error, "the search index is not looked after");
        return;
    }
    match engine.ensure_index().await {
        Ok(state) => {
            *pause = Pause::default();
            engine.set_index_state(state);
        }
        Err(ClientError::NoNori) => {
            tracing::error!(
                "the search server has no nori analysis plugin (analysis-nori): bodies \
                 are not searched until it is installed there and this server started again"
            );
            engine.set_index_state(IndexState::Refused);
        }
        Err(error) => {
            // Said once for a row of failures: the next ones say the same.
            if pause.failed(Instant::now()) {
                tracing::warn!(%error, "the search index could not be looked after; trying again");
            }
        }
    }
}

/// Starts the task of the search's upkeep ([`tend`]); `None` where no search server
/// is configured.
pub(crate) fn start_upkeep(app: &App) -> Option<RecurringTask> {
    app.search().engine.as_ref()?;
    let app = app.clone();
    Some(RecurringTask::spawn_starting_now(
        "searchUpkeep",
        UPKEEP_INTERVAL,
        move || {
            let app = app.clone();
            async move { tend(&app).await }
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_server_that_does_not_answer_is_left_alone_twice_as_long_each_time() {
        let start = Instant::now();
        let mut pause = Pause::default();
        assert!(pause.is_over(start));
        assert!(pause.failed(start), "the first of a row");
        assert!(!pause.is_over(start));
        assert!(pause.is_over(start + FIRST_PAUSE));
        assert!(!pause.failed(start));
        assert!(!pause.is_over(start + FIRST_PAUSE));
        assert!(pause.is_over(start + FIRST_PAUSE * 2));
        for _ in 0..40 {
            pause.failed(start);
        }
        assert!(pause.is_over(start + LONGEST_PAUSE), "never longer");
    }
}
