//! Revisions for cached authorization reads, spanning the whole writing transaction.
//!
//! Each organization has a revision of its own, which its writes move on, behind a
//! lock of its own: what one organization writes neither empties another's cached
//! reads nor waits for another's dispatch. A reader of no organization takes a
//! revision that every write of an organization moves on, and so does a write of no
//! organization, which reaches none of their rows.
//!
//! A write of the system's reach ([`Tenant::System`]: the housekeeping, the steps
//! after a migration, finding out whose something is) moves no revision: none of it
//! changes who may see what (it throws away what is in the trash or ended, or makes
//! an organization nobody is in yet). What changes an organization's people's access
//! is written through that organization's handle ([`Store::for_org`]).
//!
//! A write abandoned before its outcome was known (its future was dropped between its
//! first authorization statement and its commit or rollback) may still land. For
//! a while after that ([`settle_after`]) nothing of its scope is cached: every check
//! reads the store anew, and a dispatch is still checked against concurrent writers,
//! but none is refused for it.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
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

    fn end(&mut self, unsettled_until: Option<Instant>) {
        self.writers -= 1;
        self.revision = self.revision.wrapping_add(1);
        if unsettled_until.is_some() {
            self.unsettled_until = unsettled_until;
        }
    }

    fn settled_at(self, now: Instant) -> bool {
        self.unsettled_until.is_none_or(|until| now >= until)
    }

    fn version(self, reach: Reach, now: Instant) -> VisibilityVersion {
        VisibilityVersion {
            reach,
            revision: self.revision,
            cacheable: self.settled_at(now),
        }
    }
}

/// One count behind a lock of its own: a dispatch holds it while it queues.
type Guarded = Arc<Mutex<Count>>;

/// Whose rows a reader depends on, or a write moves the revision of.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Reach {
    Org(Uuid),
    Every,
}

impl Reach {
    /// What a reader of `tenant` depends on: one organization's rows, or anyone's.
    fn of_reader(tenant: Tenant) -> Self {
        // No organization, or every one: what it reads may be of any.
        if let Tenant::Org(org) = tenant {
            Self::Org(org)
        } else {
            Self::Every
        }
    }
}

/// What a write moves on: its organization's revision and the one of every write,
/// the latter alone for a write of no organization, nothing for the system's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Moves {
    Org(Uuid),
    Every,
    Nothing,
}

impl From<Tenant> for Moves {
    fn from(tenant: Tenant) -> Self {
        if let Tenant::Org(org) = tenant {
            Self::Org(org)
        } else if tenant == Tenant::Unset {
            Self::Every
        } else {
            // The system's reach (`Store::system`), which changes nobody's access.
            Self::Nothing
        }
    }
}

#[derive(Debug)]
pub(crate) struct Visibility {
    /// Every write of an organization or of none: for readers of no organization.
    every: Guarded,
    /// Each organization's, by its identifier. The map is held only to find one.
    orgs: Mutex<HashMap<Uuid, Guarded>>,
    /// Moves on whenever a writer starts or ends, for whoever waits for one to end.
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
            every: Guarded::default(),
            orgs: Mutex::default(),
            changed: watch::channel(0).0,
            settle,
        }
    }

    /// The count of `reach`, made for an organization the first time it is asked.
    fn count(&self, reach: Reach) -> Guarded {
        match reach {
            Reach::Every => Arc::clone(&self.every),
            Reach::Org(org) => Arc::clone(
                self.orgs
                    .lock()
                    .expect("visibility organizations lock")
                    .entry(org)
                    .or_default(),
            ),
        }
    }

    pub(crate) fn writing(&self, tenant: Tenant) -> Writing<'_> {
        Writing {
            visibility: self,
            moves: tenant.into(),
            touched: AtomicBool::new(false),
            finished: AtomicBool::new(false),
        }
    }

    /// Applies `change` to what a write moves, each count under its own lock, one
    /// after the other: a reader reads one of them.
    fn change(&self, moves: Moves, change: impl Fn(&mut Count)) {
        let reaches: &[Reach] = match moves {
            Moves::Org(org) => &[Reach::Org(org), Reach::Every],
            Moves::Every => &[Reach::Every],
            Moves::Nothing => &[],
        };
        for reach in reaches {
            change(&mut lock(&self.count(*reach)));
        }
        self.changed
            .send_modify(|step| *step = step.wrapping_add(1));
    }

    pub(crate) fn if_current(&self, version: VisibilityVersion, send: impl FnOnce()) -> bool {
        let count = self.count(version.reach);
        let count = lock(&count);
        if count.writers != 0 || count.revision != version.revision {
            return false;
        }
        send();
        true
    }

    pub(crate) async fn version(&self, tenant: Tenant) -> VisibilityVersion {
        let reach = Reach::of_reader(tenant);
        let count = self.count(reach);
        let mut changes = self.changed.subscribe();
        loop {
            {
                let count = lock(&count);
                if count.writers == 0 {
                    return count.version(reach, Instant::now());
                }
            }
            // Subscribed before the count was read, so a commit cannot be missed.
            changes
                .changed()
                .await
                .expect("the store owns the revision sender");
        }
    }
}

fn lock(count: &Mutex<Count>) -> MutexGuard<'_, Count> {
    count.lock().expect("visibility revision lock")
}

/// A committed authorization revision of what one organization reads (or of
/// everything), and whether its reads may be cached.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VisibilityVersion {
    reach: Reach,
    revision: u64,
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
    /// its pending writers finish (of every organization's writes, for a handle of
    /// none). Cached reads are reusable only at the same cacheable revision.
    pub async fn visibility_version(&self) -> VisibilityVersion {
        self.inner.visibility.version(self.tenant).await
    }

    /// Runs a dispatch only while its checked revision is current and settled: no
    /// write of its scope started or ended since. It holds the lock of that scope's
    /// revision alone: writes of another organization go on meanwhile. The callback
    /// must be synchronous and must not call back into the store. Returns false if
    /// authorization must be checked again.
    pub fn if_visibility_current(&self, version: VisibilityVersion, send: impl FnOnce()) -> bool {
        self.inner.visibility.if_current(version, send)
    }
}

/// Lives outside the connection/transaction future, including its commit or rollback.
/// Abandonment moves the revision on and keeps reads from being cached for a while.
#[derive(Debug)]
pub(crate) struct Writing<'a> {
    visibility: &'a Visibility,
    moves: Moves,
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
        if self.moves == Moves::Nothing || self.touched.swap(true, Ordering::Relaxed) {
            return;
        }
        self.visibility.change(self.moves, Count::start);
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
        let unsettled = (!self.finished.load(Ordering::Relaxed))
            .then(|| Instant::now() + self.visibility.settle);
        self.visibility
            .change(self.moves, |count| count.end(unsettled));
    }
}

/// Every statement is built by the store. Treat any write mentioning these tables
/// conservatively, including writes with subqueries and the test-only raw SQL path.
/// Ordinary card/body writes stay cacheable; type changes invalidate explicitly,
/// and deleting or restoring cards changes their visibility through the trash.
/// A user's row is read by no cached check, so changing it (a name, a password) moves
/// nothing; deleting one does, as it takes the user's memberships with it.
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
    let mut cascading = false;
    for word in text.split(|c: char| !c.is_ascii_alphanumeric() && c != '_') {
        if TABLES.iter().any(|table| word.eq_ignore_ascii_case(table)) {
            return true;
        }
        deletes |= word.eq_ignore_ascii_case("delete");
        cascading |= word.eq_ignore_ascii_case("blocks") || word.eq_ignore_ascii_case("users");
    }
    deletes && cascading
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

        assert_ne!(
            visibility.version(second).await.scope(),
            visibility.version(first).await.scope()
        );
    }

    #[tokio::test]
    async fn a_write_of_no_organization_moves_only_what_readers_of_none_read() {
        let visibility = Visibility::default();
        let of_first = visibility.version(org(1)).await;
        let of_everything = visibility.version(Tenant::Unset).await;
        let writing = visibility.writing(Tenant::Unset);
        writing.statement("UPDATE organizations SET delete_at = 1");
        assert!(visibility.if_current(of_first, || {}));
        assert!(!visibility.if_current(of_everything, || panic!("a writer is active")));
        writing.settled();
        drop(writing);
        assert_eq!(visibility.version(org(1)).await, of_first);
        assert_ne!(visibility.version(Tenant::Unset).await, of_everything);
    }

    #[test]
    fn a_dispatch_holds_only_its_own_organizations_writers() {
        let visibility = Visibility::default();
        let version = VisibilityVersion {
            reach: Reach::Org(Uuid::from_u128(1)),
            revision: 0,
            cacheable: true,
        };
        assert!(visibility.if_current(version, || {
            let writing = visibility.writing(org(2));
            writing.statement("DELETE FROM board_members");
            writing.settled();
        }));
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
    }

    #[test]
    fn a_users_row_moves_the_revision_only_as_it_goes() {
        assert!(!affects_visibility(
            "UPDATE users SET props = $1 WHERE id = $2"
        ));
        assert!(!affects_visibility(
            "UPDATE users SET password = $1 WHERE id = $2"
        ));
        assert!(affects_visibility("DELETE FROM users WHERE id = $1"));
        assert!(affects_visibility("DELETE FROM blocks WHERE id = $1"));
        assert!(!affects_visibility("UPDATE blocks SET title = $1"));
    }

    #[tokio::test]
    async fn a_store_settles_for_twice_its_busy_timeout() {
        let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/visibility-tests")
            .join(fvoci_model::new_id());
        std::fs::create_dir_all(&directory).expect("a test directory");
        let path = directory.join("settle.db");
        let store = Store::open(
            crate::DbType::Sqlite,
            &format!("{}?_busy_timeout=120000", path.display()),
        )
        .await
        .expect("a store");
        assert_eq!(store.inner.visibility.settle, Duration::from_secs(240));
        store.close().await;
        std::fs::remove_dir_all(&directory).expect("the test directory goes");
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
            revision: 0,
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
