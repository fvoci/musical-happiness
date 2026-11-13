//! Revisions for cached authorization reads, spanning the whole writing transaction.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use tokio::sync::watch;

use crate::Store;

#[derive(Debug, Default)]
struct State {
    revision: u64,
    writers: usize,
    uncertain: bool,
}

#[derive(Debug)]
pub(crate) struct Visibility {
    state: Mutex<State>,
    changed: watch::Sender<u64>,
}

impl Default for Visibility {
    fn default() -> Self {
        Self {
            state: Mutex::default(),
            changed: watch::channel(0).0,
        }
    }
}

impl Visibility {
    pub(crate) fn writing(&self) -> Writing<'_> {
        Writing {
            visibility: self,
            touched: AtomicBool::new(false),
            finished: AtomicBool::new(false),
        }
    }

    fn if_current(&self, version: VisibilityVersion, send: impl FnOnce()) -> bool {
        let state = self.state.lock().expect("visibility revision lock");
        if state.uncertain || state.writers != 0 || state.revision != version.revision {
            return false;
        }
        send();
        true
    }

    async fn version(&self) -> VisibilityVersion {
        let mut changes = self.changed.subscribe();
        loop {
            {
                let state = self.state.lock().expect("visibility revision lock");
                if state.writers == 0 {
                    return VisibilityVersion {
                        revision: state.revision,
                        cacheable: !state.uncertain,
                    };
                }
            }
            // Subscribe before checking the state so a commit cannot be missed.
            changes
                .changed()
                .await
                .expect("the store owns the revision sender");
        }
    }
}

/// A committed authorization revision and whether its reads may be cached.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VisibilityVersion {
    revision: u64,
    cacheable: bool,
}

impl VisibilityVersion {
    /// False after an abandoned write whose eventual database outcome is uncertain.
    pub fn cacheable(self) -> bool {
        self.cacheable
    }
}

impl Store {
    /// The revision of authorization data after its pending writers finish.
    /// Cached reads are reusable only at the same cacheable revision. An abandoned
    /// authorization write has no confirmed outcome: dispatch must stop until this
    /// store is reopened, rather than trust a read made before a delayed commit.
    pub async fn visibility_version(&self) -> VisibilityVersion {
        self.inner.visibility.version().await
    }

    /// Runs a dispatch only while its checked revision is current and settled.
    /// The callback must be synchronous and must not call back into the store.
    /// Returns false if authorization must be checked again, or settlement is uncertain.
    pub fn if_visibility_current(&self, version: VisibilityVersion, send: impl FnOnce()) -> bool {
        self.inner.visibility.if_current(version, send)
    }
}

/// Lives outside the connection/transaction future, including its commit or rollback.
/// Abandonment invalidates existing entries and prevents caching a possibly stale read.
#[derive(Debug)]
pub(crate) struct Writing<'a> {
    visibility: &'a Visibility,
    touched: AtomicBool,
    finished: AtomicBool,
}

impl Writing<'_> {
    pub(crate) fn statement(&self, text: &str) {
        if self.touched.load(Ordering::Relaxed) || !affects_visibility(text) {
            return;
        }
        self.touched.store(true, Ordering::Relaxed);
        let mut state = self
            .visibility
            .state
            .lock()
            .expect("visibility revision lock");
        state.writers += 1;
        state.revision = state.revision.wrapping_add(1);
        self.visibility.changed.send_replace(state.revision);
    }

    pub(crate) fn settled(&self) {
        self.finished.store(true, Ordering::Relaxed);
    }
}

impl Drop for Writing<'_> {
    fn drop(&mut self) {
        if !self.touched.load(Ordering::Relaxed) {
            return;
        }
        let mut state = self
            .visibility
            .state
            .lock()
            .expect("visibility revision lock");
        state.writers -= 1;
        state.uncertain |= !self.finished.load(Ordering::Relaxed);
        state.revision = state.revision.wrapping_add(1);
        self.visibility.changed.send_replace(state.revision);
    }
}

/// Every statement is built by the store. Treat any write mentioning these tables
/// conservatively, including writes with subqueries and the test-only raw SQL path.
/// Card/body writes do not mention them and do not invalidate authorization caches.
fn affects_visibility(text: &str) -> bool {
    const TABLES: [&str; 9] = [
        "workspace_members",
        "organization_members",
        "board_members",
        "sharing",
        "workspaces",
        "organizations",
        "boards",
        "users",
        "deleted_boards",
    ];
    text.split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .any(|word| TABLES.iter().any(|table| word.eq_ignore_ascii_case(table)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_dispatch_requires_the_same_settled_revision() {
        let visibility = Visibility::default();
        let old = visibility.version().await;
        let writing = visibility.writing();
        writing.statement("UPDATE workspace_members SET role = 'guest'");
        assert!(!visibility.if_current(old, || panic!("a writer is active")));
        writing.settled();
        drop(writing);
        assert!(!visibility.if_current(old, || panic!("the revision changed")));
        let current = visibility.version().await;
        let mut sent = false;
        assert!(visibility.if_current(current, || sent = true));
        assert!(sent);
    }

    #[tokio::test]
    async fn an_abandoned_authorization_write_cannot_authorize_dispatch() {
        let visibility = Visibility::default();
        let writing = visibility.writing();
        writing.statement("DELETE FROM sharing");
        drop(writing);
        let uncertain = visibility.version().await;
        assert!(!uncertain.cacheable());
        assert!(
            !visibility.if_current(uncertain, || panic!("late commit can still revoke access"))
        );
        let writing = visibility.writing();
        writing.statement("UPDATE boards SET title = 'later'");
        writing.settled();
        drop(writing);
        let later = visibility.version().await;
        assert_ne!(uncertain, later);
        assert!(
            !later.cacheable(),
            "another successful write cannot prove the abandoned write settled"
        );
    }

    #[test]
    fn authorization_writers_cannot_enter_a_checked_dispatch() {
        let visibility = Visibility::default();
        let version = VisibilityVersion {
            revision: 0,
            cacheable: true,
        };
        std::thread::scope(|scope| {
            let (started, starting) = std::sync::mpsc::sync_channel(0);
            let (entered, entering) = std::sync::mpsc::sync_channel(0);
            assert!(visibility.if_current(version, || {
                let visibility = &visibility;
                scope.spawn(move || {
                    let writing = visibility.writing();
                    started.send(()).expect("writer starts");
                    writing.statement("DELETE FROM board_members");
                    entered.send(()).expect("writer entered");
                    writing.settled();
                });
                starting.recv().expect("writer attempted to enter");
                assert!(
                    entering
                        .recv_timeout(std::time::Duration::from_millis(20))
                        .is_err(),
                    "writer cannot enter until enqueue ends"
                );
            }));
            entering
                .recv_timeout(std::time::Duration::from_secs(5))
                .expect("writer enters after enqueue ends");
        });
    }

    #[tokio::test]
    async fn confirmed_rollback_is_settled_but_abandoned_transaction_is_not() {
        use crate::StoreError;
        use crate::testing::TestDatabase;
        let directory =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/visibility-tests");
        let database = TestDatabase::create(&directory).await;
        let store = Store::connect(database.db_type(), database.dbconfig())
            .await
            .expect("test store");
        let error: Result<(), StoreError> = store
            .transaction(async |scope| {
                let write = scope.sql("UPDATE users SET username = username");
                scope.execute(write).await?;
                let invalid = scope.sql("SELECT missing_visibility_test_column FROM users");
                scope.fetch_optional(invalid).await?;
                Ok(())
            })
            .await;
        assert!(matches!(error, Err(StoreError::Database(_))));
        assert!(
            store.visibility_version().await.cacheable(),
            "rollback confirmed the failed write settled"
        );
        let reached = tokio::sync::Notify::new();
        let mut writing = Box::pin(store.transaction(async |scope| {
            let write = scope.sql("UPDATE users SET username = username");
            scope.execute(write).await?;
            reached.notify_one();
            std::future::pending::<Result<(), StoreError>>().await
        }));
        tokio::select! {
            result = &mut writing => panic!("transaction must remain pending: {result:?}"),
            () = reached.notified() => {}
        }
        drop(writing);
        let version = store.inner.visibility.version().await;
        assert!(
            !version.cacheable(),
            "abandoned transaction cannot authorize until settlement is known"
        );
        assert!(!store.if_visibility_current(version, || panic!("must not enqueue")));
        store.close().await;
        database.remove().await;
    }
}
