//! The search of what cards and pages say, through a search server that is set up apart
//! (`search_url`): Elasticsearch or `OpenSearch`, with the nori analysis plugin.
//!
//! The browser never reaches the search server. The server keeps one index there; a
//! search asks it for documents of the user's workspace, and then asks the database
//! about every document that comes back (`App::search_workspace`): the index may be
//! behind what is stored, and what it says is never an answer by itself.
//!
//! What changes is sent after it is stored: the store notes every change that a
//! search would find differently, in the transaction that makes it, and the upkeep
//! ([`tend`]) sends what is stored of what was noted, with the reach of the
//! housekeeping, as the one server of its database.
//!
//! Without `search_url` there is no search of bodies, nothing stands in for it, and
//! nothing is noted.

mod client;
mod document;
mod find;
mod index;
mod limit;
mod query;
mod upkeep;
mod worker;

use std::sync::Mutex;
use std::time::{Duration, Instant};

pub use find::{Asked, AskedError, MAX_LIMIT, MAX_TEXT_CHARS};
pub use upkeep::{reindex, tend};
pub(crate) use upkeep::{settle_at_start, start_upkeep, stop_upkeep};

use client::{Client, ClientError};
use limit::Limiter;

use crate::config::search::SearchSettings;

/// How long the upkeep leaves a search server that did not answer alone, at first
/// and at most; doubled with every failure in a row.
const FIRST_PAUSE: Duration = Duration::from_secs(1);
/// See [`FIRST_PAUSE`].
const LONGEST_PAUSE: Duration = Duration::from_secs(60);
/// How often the log says that searches are not answered, while they are not.
const FAILURES_SAID_EVERY: Duration = Duration::from_secs(30);

/// The search of bodies, as the server is configured: on a search server, or none.
#[derive(Debug)]
pub struct Search {
    engine: Option<Engine>,
}

/// The search server and what is known of the index on it.
#[derive(Debug)]
pub(crate) struct Engine {
    client: Client,
    limiter: Limiter,
    index: Mutex<IndexState>,
    /// Held by the upkeep while it runs: one at a time.
    upkeep: tokio::sync::Mutex<Pause>,
    /// When a search that the search server did not answer was last said in the log.
    search_failure_said: Mutex<Option<Instant>>,
}

/// What is known of the index on the search server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IndexState {
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
    /// Why the last run failed.
    last: Option<ClientError>,
    /// How many runs in a row found nothing to send.
    idle_runs: u32,
}

impl Pause {
    fn is_over(&self, now: Instant) -> bool {
        self.until.is_none_or(|until| now >= until)
    }

    /// Notes a failure at `now`. Returns whether it is news: the first in a row, or
    /// another failure than the one before.
    fn failed(&mut self, now: Instant, error: ClientError) -> bool {
        self.wait(now);
        let news = self.last.as_ref() != Some(&error);
        self.last = Some(error);
        news
    }

    /// Leaves the search alone for a while from `now`, longer each time in a row: for
    /// what failed otherwise than by the search server.
    fn wait(&mut self, now: Instant) {
        let pause = FIRST_PAUSE
            .saturating_mul(2_u32.saturating_pow(self.failures))
            .min(LONGEST_PAUSE);
        self.until = Some(now + pause);
        self.failures = self.failures.saturating_add(1);
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
                search_failure_said: Mutex::new(None),
            }),
        })
    }

    /// Whether bodies can be searched on this server: a search server is configured,
    /// and has not turned out to be unable to hold the index.
    #[must_use]
    pub fn enabled(&self) -> bool {
        self.engine().is_some()
    }

    /// Whether the configuration names a search server, whatever became of it.
    pub(crate) fn configured(&self) -> bool {
        self.engine.is_some()
    }

    /// The search server, if bodies can be searched.
    fn engine(&self) -> Option<&Engine> {
        self.engine
            .as_ref()
            .filter(|engine| engine.index_state() != IndexState::Refused)
    }
}

impl Engine {
    pub(super) fn index_state(&self) -> IndexState {
        *self
            .index
            .lock()
            .expect("the lock of the index state is not poisoned")
    }

    /// Whether a search that failed at `now` is to be said in the log: the first, and
    /// then one every [`FAILURES_SAID_EVERY`].
    pub(super) fn search_failure_is_news(&self, now: Instant) -> bool {
        let mut said = self
            .search_failure_said
            .lock()
            .expect("the lock of the failures said is not poisoned");
        let news = said.is_none_or(|last| now.duration_since(last) >= FAILURES_SAID_EVERY);
        if news {
            *said = Some(now);
        }
        news
    }

    /// Notes that the index turned out not to be there: the upkeep looks for it
    /// again. A search that was turned off stays off.
    pub(super) fn lose_index(&self) {
        let mut state = self
            .index
            .lock()
            .expect("the lock of the index state is not poisoned");
        if *state == IndexState::Ready {
            *state = IndexState::Unknown;
        }
    }

    pub(super) fn set_index_state(&self, state: IndexState) {
        *self
            .index
            .lock()
            .expect("the lock of the index state is not poisoned") = state;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_server_that_does_not_answer_is_left_alone_twice_as_long_each_time() {
        let start = Instant::now();
        let down = || ClientError::Unreachable(client::NoAnswer::Connection);
        let mut pause = Pause::default();
        assert!(pause.is_over(start));
        assert!(pause.failed(start, down()), "the first of a row");
        assert!(!pause.is_over(start));
        assert!(pause.is_over(start + FIRST_PAUSE));
        assert!(!pause.failed(start, down()), "the same again is no news");
        assert!(!pause.is_over(start + FIRST_PAUSE));
        assert!(pause.is_over(start + FIRST_PAUSE * 2));
        assert!(
            pause.failed(start, ClientError::Unreadable),
            "another failure"
        );
        for _ in 0..40 {
            pause.failed(start, down());
        }
        assert!(pause.is_over(start + LONGEST_PAUSE), "never longer");
    }
}
