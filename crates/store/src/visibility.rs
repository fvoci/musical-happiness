//! Revisions for cached authorization reads, spanning the whole writing transaction.
//!
//! Each organization has a revision of its own, which its writes move on; a write of
//! no organization or of every organization (the directory, users, the system's
//! work) moves on a shared one that every organization's revision includes. So what
//! one organization writes does not empty another's cached reads. A reader that is of
//! no organization takes a revision that every write moves on.
//!
//! A write abandoned before its outcome was known (its future was dropped between its
//! first authorization statement and its commit or rollback) may still land. For
//! a while after that ([`settle_after`]) nothing of its scope is cached: every check
//! reads the store anew, and a dispatch is still checked against concurrent writers,
//! but none is refused for it.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use sqlx::types::Uuid;
use tokio::sync::watch;

use crate::{Store, Tenant};

/// The least time after an abandoned authorization write that reads are not cached:
/// longer than a commit that was already sent takes on PostgreSQL with asynchronous
/// or local commit (a synchronous standby that stalls can hold it longer).
const SETTLE: Duration = Duration::from_secs(60);

/// How long after an abandoned authorization write reads are not cached, for a store
/// whose statements may wait up to `longest_wait` for another writer (SQLite's busy
/// timeout): by then the write has landed or never will. A commit later than that
/// is not seen until the next write of the same scope moves its revision on.
pub(crate) fn settle_after(longest_wait: Duration) -> Duration {
    SETTLE.max(longest_wait.saturating_mul(2))
}

/// Writers of a part of the data, its revision, and until when its reads are not
/// cached after one of its writes was abandoned.
#[derive(Debug, Default, Clone, Copy)]
struct Count {
    revision: u64,
    writers: usize,
    unsettled_until: Option<Instant>,
}

impl Count {
    fn start(&mut self) {
        self.writers += 1;
        self.revision = self.revision.wrapping_add(1);
    }

    fn end(&mut self) {
        self.writers -= 1;
        self.revision = self.revision.wrapping_add(1);
    }

    fn settled_at(self, now: Instant) -> bool {
        self.unsettled_until.is_none_or(|until| now >= until)
    }
}

#[derive(Debug, Default)]
struct State {
    /// Writes of no organization or of every organization.
    shared: Count,
    /// Every write, for readers of no organization.
    every: Count,
    /// Writes of one organization, by its identifier. Only written ones are kept: a
    /// reader of an organization nobody wrote takes the default.
    orgs: HashMap<Uuid, Count>,
}

impl State {
    fn counts(&self, reach: Reach) -> [Count; 2] {
        match reach {
            Reach::Org(org) => [
                self.shared,
                self.orgs.get(&org).copied().unwrap_or_default(),
            ],
            Reach::Every => [self.every, self.every],
        }
    }

    fn writers_of(&self, reach: Reach) -> usize {
        self.counts(reach).iter().map(|count| count.writers).sum()
    }

    fn version(&self, reach: Reach, now: Instant) -> VisibilityVersion {
        let [shared, own] = self.counts(reach);
        VisibilityVersion {
            reach,
            shared: shared.revision,
            own: own.revision,
            cacheable: shared.settled_at(now) && own.settled_at(now),
        }
    }

    /// Applies `change` to what a write of `reach` moves: its organization's count
    /// (the shared one for a write of no or every organization), and the count of
    /// every write.
    fn change(&mut self, reach: Reach, change: impl Fn(&mut Count)) {
        change(&mut self.every);
        match reach {
            Reach::Org(org) => change(self.orgs.entry(org).or_default()),
            Reach::Every => change(&mut self.shared),
        }
    }
}

/// Whose rows a write may reach, or a reader depends on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Reach {
    Org(Uuid),
    Every,
}

impl From<Tenant> for Reach {
    fn from(tenant: Tenant) -> Self {
        match tenant {
            Tenant::Org(org) => Self::Org(org),
            Tenant::Unset | Tenant::System => Self::Every,
        }
    }
}

#[derive(Debug)]
pub(crate) struct Visibility {
    state: Mutex<State>,
    changed: watch::Sender<u64>,
    settle: Duration,
}

impl Default for Visibility {
    fn default() -> Self {
        Self::settling_for(SETTLE)
    }
}

impl Visibility {
    pub(crate) fn settling_for(settle: Duration) -> Self {
        Self {
            state: Mutex::default(),
            changed: watch::channel(0).0,
            settle,
        }
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().expect("visibility revision lock")
    }

    pub(crate) fn writing(&self, tenant: Tenant) -> Writing<'_> {
        Writing {
            visibility: self,
            reach: tenant.into(),
            touched: AtomicBool::new(false),
            finished: AtomicBool::new(false),
        }
    }

    fn if_current(&self, version: VisibilityVersion, send: impl FnOnce()) -> bool {
        let state = self.state();
        let now = state.version(version.reach, Instant::now());
        if state.writers_of(version.reach) != 0
            || (now.shared, now.own) != (version.shared, version.own)
        {
            return false;
        }
        send();
        true
    }

    async fn version(&self, tenant: Tenant) -> VisibilityVersion {
        let reach = Reach::from(tenant);
        let mut changes = self.changed.subscribe();
        loop {
            {
                let state = self.state();
                if state.writers_of(reach) == 0 {
                    return state.version(reach, Instant::now());
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

/// A committed authorization revision of what one organization reads (or of
/// everything), and whether its reads may be cached.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VisibilityVersion {
    reach: Reach,
    shared: u64,
    own: u64,
    cacheable: bool,
}

impl VisibilityVersion {
    /// False for a while after an abandoned write whose outcome was not known: reads
    /// are made anew each time then.
    pub fn cacheable(self) -> bool {
        self.cacheable
    }

    /// What the version is of: two versions of one scope may share a cache slot.
    pub fn scope(self) -> VisibilityScope {
        VisibilityScope(self.reach)
    }
}

/// What a [`VisibilityVersion`] is of: one organization, or everything.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VisibilityScope(Reach);

impl Store {
    /// The revision of the authorization data this handle's organization reads, after
    /// its pending writers finish (every organization's, for a handle of none). Cached
    /// reads are reusable only at the same cacheable revision.
    pub async fn visibility_version(&self) -> VisibilityVersion {
        self.inner.visibility.version(self.tenant).await
    }

    /// Runs a dispatch only while its checked revision is current and settled: no
    /// write of its scope started or ended since. The callback must be synchronous and
    /// must not call back into the store. Returns false if authorization must be
    /// checked again.
    pub fn if_visibility_current(&self, version: VisibilityVersion, send: impl FnOnce()) -> bool {
        self.inner.visibility.if_current(version, send)
    }
}

/// Lives outside the connection/transaction future, including its commit or rollback.
/// Abandonment moves the revision on and keeps reads from being cached for a while.
#[derive(Debug)]
pub(crate) struct Writing<'a> {
    visibility: &'a Visibility,
    reach: Reach,
    touched: AtomicBool,
    finished: AtomicBool,
}

impl Writing<'_> {
    pub(crate) fn statement(&self, text: &str) {
        if !self.touched.load(Ordering::Relaxed) && affects_visibility(text) {
            self.invalidate();
        }
    }

    pub(crate) fn invalidate(&self) {
        if self.touched.swap(true, Ordering::Relaxed) {
            return;
        }
        let mut state = self.visibility.state();
        state.change(self.reach, Count::start);
        self.visibility.changed.send_replace(state.every.revision);
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
        let mut state = self.visibility.state();
        let unsettled = (!self.finished.load(Ordering::Relaxed))
            .then(|| Instant::now() + self.visibility.settle);
        state.change(self.reach, |count| {
            count.end();
            if unsettled.is_some() {
                count.unsettled_until = unsettled;
            }
        });
        self.visibility.changed.send_replace(state.every.revision);
    }
}

/// Every statement is built by the store. `users` is not among the tables: no cached
/// read depends on a user's row, and a user who goes has their connections closed.
/// Treat any write mentioning these tables conservatively, including writes with subqueries and the test-only raw SQL path.
/// Ordinary card/body writes stay cacheable; type changes invalidate explicitly,
/// and deleting or restoring cards changes their visibility through the trash.
fn affects_visibility(text: &str) -> bool {
    const TABLES: [&str; 10] = [
        "workspace_members",
        "organization_members",
        "board_members",
        "sharing",
        "workspaces",
        "organizations",
        "boards",
        "deleted_boards",
        "deleted_blocks",
        // Which provider an organization takes sessions of (`sso_required`).
        "oidc_providers",
    ];
    let mut deletes = false;
    let mut blocks = false;
    for word in text.split(|c: char| !c.is_ascii_alphanumeric() && c != '_') {
        if TABLES.iter().any(|table| word.eq_ignore_ascii_case(table)) {
            return true;
        }
        deletes |= word.eq_ignore_ascii_case("delete");
        blocks |= word.eq_ignore_ascii_case("blocks");
    }
    deletes && blocks
}

#[cfg(test)]
mod tests {
    use super::*;

    fn org(n: u128) -> Tenant {
        Tenant::Org(Uuid::from_u128(n))
    }

    #[tokio::test]
    async fn a_dispatch_requires_the_same_settled_revision() {
        let visibility = Visibility::default();
        let old = visibility.version(Tenant::Unset).await;
        let writing = visibility.writing(Tenant::Unset);
        writing.statement("UPDATE workspace_members SET role = 'guest'");
        assert!(!visibility.if_current(old, || panic!("a writer is active")));
        writing.settled();
        drop(writing);
        assert!(!visibility.if_current(old, || panic!("the revision changed")));
        let current = visibility.version(Tenant::Unset).await;
        let mut sent = false;
        assert!(visibility.if_current(current, || sent = true));
        assert!(sent);
    }

    #[tokio::test]
    async fn an_abandoned_write_stops_caching_for_a_while_but_not_dispatch() {
        let visibility = Visibility::settling_for(Duration::from_millis(50));
        let writing = visibility.writing(Tenant::Unset);
        writing.statement("DELETE FROM sharing");
        drop(writing);
        let unsettled = visibility.version(Tenant::Unset).await;
        assert!(!unsettled.cacheable(), "its outcome may still land");
        let mut sent = false;
        assert!(
            visibility.if_current(unsettled, || sent = true),
            "a dispatch checked against the store anew goes on"
        );
        assert!(sent);
        let writing = visibility.writing(Tenant::Unset);
        writing.statement("UPDATE boards SET title = 'later'");
        writing.settled();
        drop(writing);
        assert!(
            !visibility.version(Tenant::Unset).await.cacheable(),
            "another successful write does not prove the abandoned one settled"
        );
        tokio::time::sleep(Duration::from_millis(60)).await;
        assert!(
            visibility.version(Tenant::Unset).await.cacheable(),
            "after the settle time reads are cached again"
        );
    }

    #[tokio::test]
    async fn one_organizations_writes_leave_anothers_revision() {
        let visibility = Visibility::default();
        let (first, second) = (org(1), org(2));
        let of_second = visibility.version(second).await;
        let of_everything = visibility.version(Tenant::Unset).await;
        let writing = visibility.writing(first);
        writing.statement("UPDATE board_members SET scheme_admin = true");
        assert!(
            visibility.if_current(of_second, || {}),
            "a writer of the first organization does not hold up the second's dispatch"
        );
        writing.settled();
        drop(writing);
        assert_eq!(visibility.version(second).await, of_second);
        assert_ne!(visibility.version(Tenant::Unset).await, of_everything);
        assert!(!visibility.if_current(of_everything, || panic!("everything moved on")));

        let writing = visibility.writing(Tenant::System);
        writing.statement("UPDATE organizations SET delete_at = 1");
        assert!(
            !visibility.if_current(of_second, || panic!("a shared writer is active")),
            "a write of every organization holds up each one's dispatch"
        );
        writing.settled();
        drop(writing);
        assert_ne!(visibility.version(second).await, of_second);
        assert_ne!(
            visibility.version(second).await.scope(),
            visibility.version(first).await.scope()
        );
    }

    #[tokio::test]
    async fn an_abandoned_write_unsettles_its_own_organization_only() {
        let visibility = Visibility::settling_for(Duration::from_secs(60));
        let writing = visibility.writing(org(1));
        writing.statement("DELETE FROM board_members");
        drop(writing);
        assert!(!visibility.version(org(1)).await.cacheable());
        assert!(!visibility.version(Tenant::Unset).await.cacheable());
        assert!(
            visibility.version(org(2)).await.cacheable(),
            "another organization keeps its cache"
        );
        let writing = visibility.writing(Tenant::System);
        writing.statement("UPDATE organizations SET delete_at = 1");
        drop(writing);
        assert!(
            !visibility.version(org(2)).await.cacheable(),
            "an abandoned write of every organization unsettles each"
        );
    }

    #[test]
    fn the_settle_time_outlasts_the_longest_wait() {
        assert_eq!(settle_after(Duration::from_secs(5)), SETTLE);
        assert_eq!(
            settle_after(Duration::from_secs(120)),
            Duration::from_secs(240)
        );
    }

    #[test]
    fn authorization_writers_cannot_enter_a_checked_dispatch() {
        let visibility = Visibility::default();
        let version = VisibilityVersion {
            reach: Reach::Every,
            shared: 0,
            own: 0,
            cacheable: true,
        };
        std::thread::scope(|scope| {
            let (started, starting) = std::sync::mpsc::sync_channel(0);
            let (entered, entering) = std::sync::mpsc::sync_channel(0);
            assert!(visibility.if_current(version, || {
                let visibility = &visibility;
                scope.spawn(move || {
                    let writing = visibility.writing(Tenant::Unset);
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
                let write = scope.sql("UPDATE boards SET title = title");
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
            let write = scope.sql("UPDATE boards SET title = title");
            scope.execute(write).await?;
            reached.notify_one();
            std::future::pending::<Result<(), StoreError>>().await
        }));
        tokio::select! {
            result = &mut writing => panic!("transaction must remain pending: {result:?}"),
            () = reached.notified() => {}
        }
        drop(writing);
        let version = store.inner.visibility.version(store.tenant).await;
        assert!(
            !version.cacheable(),
            "an abandoned transaction's reads are not cached until it settles"
        );
        let mut sent = false;
        assert!(store.if_visibility_current(version, || sent = true));
        assert!(sent, "a dispatch checked anew is not refused for it");
        store.close().await;
        database.remove().await;
    }
}
