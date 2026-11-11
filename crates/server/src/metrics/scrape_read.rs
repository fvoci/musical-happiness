//! A value a scrape reads by asking the database: one read at a time, and only as long
//! as a scrape may take.

use std::fmt::Display;
use std::time::Duration;

use prometheus::{IntCounter, Opts, Registry};

use super::PREFIX;

/// How long a scrape waits for the database to answer one read. Shorter than
/// Prometheus's own timeout of a scrape (10 s by default), so that a pool that is
/// exhausted, which the pool's metrics are there to show, does not cost the scrape.
const READ_TIMEOUT: Duration = Duration::from_secs(2);

/// The reads of one value by the scrapes, and how many of them failed.
#[derive(Debug)]
pub(super) struct ScrapeRead {
    /// Held while a scrape reads: one at a time, and a scrape that finds it held
    /// answers with the value read before.
    reading: tokio::sync::Mutex<()>,
    failures: IntCounter,
}

impl ScrapeRead {
    /// Makes the counter of the failed reads, `name` with `help`, and registers it.
    pub(super) fn register(
        registry: &Registry,
        name: &str,
        help: &str,
    ) -> prometheus::Result<Self> {
        let failures = IntCounter::with_opts(Opts::new(name, help).namespace(PREFIX))?;
        registry.register(Box::new(failures.clone()))?;
        Ok(Self {
            reading: tokio::sync::Mutex::new(()),
            failures,
        })
    }

    /// What `read` answers in time; `None` if another scrape is reading, and if `read`
    /// fails or does not answer in time, which is counted and logged as a failure to
    /// read `what`. `read` is not polled if another scrape is reading.
    pub(super) async fn read<T, E: Display>(
        &self,
        what: &str,
        read: impl Future<Output = Result<T, E>>,
    ) -> Option<T> {
        let Ok(_reading) = self.reading.try_lock() else {
            return None;
        };
        match tokio::time::timeout(READ_TIMEOUT, read).await {
            Ok(Ok(value)) => Some(value),
            Ok(Err(error)) => {
                self.failures.inc();
                tracing::warn!(%error, "a scrape could not count {what}");
                None
            }
            Err(_) => {
                self.failures.inc();
                tracing::warn!("a scrape could not count {what} in time");
                None
            }
        }
    }
}
