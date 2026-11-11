//! Whether the server is ready to serve: the database answers, and the file storage can
//! be used. Asked by the readiness check of the API and by a scrape of the metrics, and
//! kept for a few seconds, so that neither asks the database every time.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use tokio::time::Instant;

use super::App;
use crate::metrics::ReadyCheck;

/// How long an answer of the checks stays fresh.
const FRESH_FOR: Duration = Duration::from_secs(5);

/// How long one check may take before it counts as failing. Only the first caller ever
/// waits for the checks; the others get the last answer while it is made anew.
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
    /// Why the server is not ready, in fixed words that name the failing checks; `None`
    /// if it is ready.
    pub fn refusal(&self) -> Option<&'static str> {
        use ReadinessCheck::{Failing, Ok};
        match (self.database, self.files) {
            (Ok, Ok) => None,
            (Failing, Ok) => Some("not ready: database"),
            (Ok, Failing) => Some("not ready: files"),
            (Failing, Failing) => Some("not ready: database, files"),
        }
    }
}

/// The last answer of the checks, and the turns of making it.
#[derive(Debug)]
pub(super) struct Kept {
    shared: Arc<Shared>,
    /// Held while the file storage is checked, also past the check's timeout: a check
    /// that hangs (a mount that does not answer) holds a thread, and the next is not
    /// started beside it but counted as failing.
    files_turn: Arc<tokio::sync::Semaphore>,
}

/// What the task that makes the answer anew shares with the callers.
#[derive(Debug, Default)]
struct Shared {
    /// The last answer, and when it was made.
    answer: Mutex<Option<(Instant, Readiness)>>,
    /// Held while the answer is made: one at a time.
    making: Arc<tokio::sync::Mutex<()>>,
}

impl Shared {
    fn keep(&self, readiness: Readiness) {
        *self.answer.lock().expect("lock poisoned") = Some((Instant::now(), readiness));
    }

    fn last(&self) -> Option<(Instant, Readiness)> {
        *self.answer.lock().expect("lock poisoned")
    }
}

impl Default for Kept {
    fn default() -> Self {
        Self {
            shared: Arc::default(),
            files_turn: Arc::new(tokio::sync::Semaphore::new(1)),
        }
    }
}

impl Kept {
    /// The answer, made by `make` if there is none fresh: the first caller ever waits
    /// for it, the callers at that moment for the same one; once there is an answer, a
    /// caller that finds it stale gets it as it is, and one task makes it anew.
    async fn get<F>(&self, make: impl FnOnce() -> F) -> Readiness
    where
        F: Future<Output = Readiness> + Send + 'static,
    {
        match self.shared.last() {
            Some((at, readiness)) if at.elapsed() < FRESH_FOR => readiness,
            Some((_, readiness)) => {
                if let Ok(making) = Arc::clone(&self.shared.making).try_lock_owned() {
                    let shared = Arc::clone(&self.shared);
                    let checks = make();
                    tokio::spawn(async move {
                        let _making = making;
                        shared.keep(checks.await);
                    });
                }
                readiness
            }
            None => {
                let _making = self.shared.making.lock().await;
                if let Some((_, readiness)) = self.shared.last() {
                    return readiness;
                }
                let readiness = make().await;
                self.shared.keep(readiness);
                readiness
            }
        }
    }
}

impl App {
    /// Whether the server is ready, as the checks found it within the last five seconds
    /// or so: the database and the file storage, side by side, two seconds each at
    /// most. Only the first caller ever waits for the checks.
    pub async fn readiness(&self) -> Readiness {
        let app = self.clone();
        self.inner
            .readiness
            .get(move || async move { app.check_readiness().await })
            .await
    }

    /// Makes the checks, and sets the gauges of the metrics from them. The database's
    /// check is one call of the store, measured as any other.
    async fn check_readiness(&self) -> Readiness {
        let (database, files) =
            tokio::join!(checked("database", self.store().ping()), self.check_files(),);
        let readiness = Readiness { database, files };
        self.metrics().set_ready([
            (
                ReadyCheck::Database,
                readiness.database == ReadinessCheck::Ok,
            ),
            (ReadyCheck::Files, readiness.files == ReadinessCheck::Ok),
        ]);
        readiness
    }

    /// Checks the file storage in a task of its own that holds the turn of the files
    /// until the check ends, whether or not it ended in time.
    async fn check_files(&self) -> ReadinessCheck {
        let Ok(turn) = Arc::clone(&self.inner.readiness.files_turn).try_acquire_owned() else {
            tracing::warn!(
                check = "files",
                "the last check of the file storage has not ended"
            );
            return ReadinessCheck::Failing;
        };
        let files = self.files().clone();
        let check = tokio::spawn(async move {
            let _turn = turn;
            files.check().await
        });
        checked("files", async move {
            check
                .await
                .map_err(|error| format!("the check did not end: {error}"))?
                .map_err(|error| error.to_string())
        })
        .await
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

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU32, Ordering};

    use super::*;

    const READY: Readiness = Readiness {
        database: ReadinessCheck::Ok,
        files: ReadinessCheck::Ok,
    };
    const NOT_READY: Readiness = Readiness {
        database: ReadinessCheck::Failing,
        files: ReadinessCheck::Ok,
    };

    /// A maker of answers that counts how often it made one, and answers `answer`.
    fn maker(
        made: &Arc<AtomicU32>,
        answer: Readiness,
    ) -> impl FnOnce() -> std::future::Ready<Readiness> {
        let made = Arc::clone(made);
        move || {
            made.fetch_add(1, Ordering::SeqCst);
            std::future::ready(answer)
        }
    }

    #[tokio::test(start_paused = true)]
    async fn an_answer_is_kept_fresh_and_then_given_while_it_is_made_anew() {
        let kept = Kept::default();
        let made = Arc::new(AtomicU32::new(0));
        assert_eq!(
            kept.get(maker(&made, READY)).await,
            READY,
            "the first waits"
        );
        assert_eq!(kept.get(maker(&made, NOT_READY)).await, READY, "fresh");
        assert_eq!(made.load(Ordering::SeqCst), 1);

        tokio::time::advance(FRESH_FOR).await;
        assert_eq!(
            kept.get(maker(&made, NOT_READY)).await,
            READY,
            "stale, given as it is"
        );
        assert_eq!(
            kept.get(maker(&made, NOT_READY)).await,
            READY,
            "while it is made anew, by one task"
        );
        tokio::task::yield_now().await;
        assert_eq!(made.load(Ordering::SeqCst), 2, "made anew once");
        assert_eq!(
            kept.get(maker(&made, READY)).await,
            NOT_READY,
            "the new answer"
        );
    }

    #[tokio::test]
    async fn callers_of_the_first_answer_share_it() {
        let kept = Kept::default();
        let made = Arc::new(AtomicU32::new(0));
        let (first, second) = tokio::join!(
            kept.get(maker(&made, READY)),
            kept.get(maker(&made, NOT_READY)),
        );
        assert_eq!((first, second), (READY, READY));
        assert_eq!(made.load(Ordering::SeqCst), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn a_check_that_does_not_answer_is_failing() {
        let check = checked("x", std::future::pending::<Result<(), String>>()).await;
        assert_eq!(check, ReadinessCheck::Failing);
    }

    #[test]
    fn the_refusal_names_the_failing_checks_in_fixed_words() {
        use ReadinessCheck::{Failing, Ok};
        let refusal = |database, files| Readiness { database, files }.refusal();
        assert_eq!(refusal(Ok, Ok), None);
        assert_eq!(refusal(Failing, Ok), Some("not ready: database"));
        assert_eq!(refusal(Ok, Failing), Some("not ready: files"));
        assert_eq!(
            refusal(Failing, Failing),
            Some("not ready: database, files")
        );
    }
}
