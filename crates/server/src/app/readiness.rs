//! Whether the server is ready to serve: the database answers, and the file storage can
//! be used. Asked by the readiness check of the API and by a scrape of the metrics, and
//! kept for a few seconds, so that neither asks the database every time.

use std::time::Duration;

use serde::Serialize;
use tokio::time::Instant;

use super::App;
use crate::metrics::ReadyCheck;

/// How long an answer of the checks stays fresh.
const FRESH_FOR: Duration = Duration::from_secs(5);

/// How long one check may take before it counts as failing: shorter than the timeouts
/// of a probe or a scrape.
const CHECK_TIMEOUT: Duration = Duration::from_secs(2);

/// How one check came out. Its words are the whole answer: no cause of a failure
/// leaves the server, the log has it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReadinessCheck {
    /// It answered in time.
    Ok,
    /// It failed or did not answer in two seconds.
    Failing,
}

/// The checks of the readiness: what the server depends on to serve.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, utoipa::ToSchema)]
pub struct Readiness {
    /// Whether the database answers a statement.
    pub database: ReadinessCheck,
    /// Whether the file storage can be used: on the local disk, the directory; on the
    /// object store, the bucket with the credentials.
    pub files: ReadinessCheck,
}

impl Readiness {
    /// Whether every check is ok.
    pub fn is_ready(&self) -> bool {
        self.failing().next().is_none()
    }

    /// The checks that are failing, by their names.
    pub fn failing(&self) -> impl Iterator<Item = &'static str> {
        [("database", self.database), ("files", self.files)]
            .into_iter()
            .filter(|(_, check)| *check == ReadinessCheck::Failing)
            .map(|(name, _)| name)
    }
}

/// The last answer of the checks, and when they were made.
#[derive(Debug, Default)]
pub(super) struct Kept(tokio::sync::Mutex<Option<(Instant, Readiness)>>);

impl App {
    /// Whether the server is ready, as the checks found it within the last five
    /// seconds; else they are made anew, the two side by side, two seconds each at
    /// most. Callers at the same moment wait for the one making them.
    pub async fn readiness(&self) -> Readiness {
        let mut kept = self.inner.readiness.0.lock().await;
        if let Some((made, readiness)) = *kept
            && made.elapsed() < FRESH_FOR
        {
            return readiness;
        }
        let (database, files) = tokio::join!(
            checked("database", self.store().ping()),
            checked("files", self.files().check()),
        );
        let readiness = Readiness { database, files };
        *kept = Some((Instant::now(), readiness));
        self.metrics().set_ready([
            (
                ReadyCheck::Database,
                readiness.database == ReadinessCheck::Ok,
            ),
            (ReadyCheck::Files, readiness.files == ReadinessCheck::Ok),
        ]);
        readiness
    }
}

/// How the check `name` that is `work` came out; a failure is logged with its cause.
async fn checked<E: std::fmt::Display>(
    name: &'static str,
    work: impl Future<Output = Result<(), E>>,
) -> ReadinessCheck {
    match tokio::time::timeout(CHECK_TIMEOUT, work).await {
        Ok(Ok(())) => ReadinessCheck::Ok,
        Ok(Err(error)) => {
            tracing::warn!(check = name, %error, "a readiness check failed");
            ReadinessCheck::Failing
        }
        Err(_) => {
            tracing::warn!(check = name, "a readiness check did not answer in time");
            ReadinessCheck::Failing
        }
    }
}
