//! SQL storage for the fvoci server, on SQLite or PostgreSQL.
//!
//! [`Store`] is the one entry point. It is cheap to clone and safe to share between
//! tasks. Every method that writes more than one statement runs in a transaction on both
//! databases.
//!
//! The schema is this server's own: a list of migrations applied by sqlx's migrator.

mod account_deletion;
mod account_erasure;
mod accounts;
mod audit;
mod block_derived;
mod blocks;
mod body_digest;
mod body_docs;
mod body_links;
mod body_versions;
mod card_values;
mod categories;
mod category_containers;
mod comment_places;
mod comment_versions;
mod connect;
mod container_kinds;
mod container_membership;
mod container_search;
mod container_team_access;
mod container_team_membership;
mod container_team_trash;
mod container_teams;
mod containers;
mod containers_and_blocks;
mod data_steps;
mod db;
mod directory;
mod error;
mod file_info;
mod file_refs;
mod invitations;
mod invited_signups;
mod joinings;
mod link_locks;
mod linked_blocks;
mod loose_objects;
mod members;
mod memberships_everywhere;
mod observe;
mod oidc;
mod oidc_former;
mod oidc_held;
mod oidc_identities;
mod oidc_pending;
mod operator_records;
mod org_admin;
mod page_copies;
mod page_places;
mod page_trash;
mod pages;
mod password_reset;
mod schema;
mod search_docs;
mod search_outbox;
mod search_text;
mod server_lock;
mod sessions;
mod sharing;
mod signups;
mod subscriptions;
mod teams;
#[cfg(feature = "test-util")]
pub mod testing;
mod trash;
mod uploads;
mod users;
mod visibility;
mod workspace_membership;
mod workspaces;

use std::sync::Arc;
use std::time::Duration;

use sqlx::Connection as _;
use sqlx::postgres::{PgPool, PgPoolOptions};
use sqlx::sqlite::{SqlitePool, SqlitePoolOptions};

pub use account_deletion::{DeletionAsking, DeletionStanding};
pub use account_erasure::{ErasedFrom, Erasing, ErasureEnd, ErasureStart, HandedOver};
pub use accounts::Founding;
pub use audit::{AuditEntry, AuditPlace};
pub use blocks::{BlockFilter, BodyPreview, DuplicateAs, ListedBlock, Paging, Previews};
pub use body_digest::body_digest;
pub use body_docs::{LoggedUpdate, StoredDoc};
pub use body_versions::{BodyBefore, DEFAULT_BODY_VERSIONS_KEPT, KeptVersion};
pub use category_containers::{ContainerPlace, ContainerVisibility};
pub use comment_versions::DEFAULT_COMMENT_VERSIONS_KEPT;
pub use connect::DbType;
pub use container_membership::Standing;
pub use containers::{Joined, MemberChange, PublicContainers};
pub use db::Dialect;
pub use directory::{
    Looking, Reach, SYSTEM_ORG_ID, SYSTEM_WORKSPACE_ID, SharedContainer, Sight, WorkspaceAccess,
};
pub use error::{StoreError, Taken};
pub use file_refs::NewBodyFiles;
pub use invitations::{
    Acceptance, Accepted, Acceptor, Inviter, InviterOfContainer, MailedTo, NewLink,
};
pub use invited_signups::InvitedConfirmation;
pub use loose_objects::{LooseCounts, LooseObject, LooseState};
pub use observe::{Access, Call, Observer, Outcome};
pub use oidc::{
    FlowPurpose, Identity, LoginFlow, OidcAccount, OidcRefusal, Removal, StoredProvider,
};
pub use oidc_former::{FormerEnd, FormerProvider};
pub use oidc_held::{CarriedOver, CarryOver, CarryOverRefusal, HeldSignIn};
pub use oidc_identities::{LinkRefusal, Linked, LinkingSession, TiedIdentity, Unlinked};
pub use oidc_pending::{Applied, Binding, PendingProvider, PendingSettings, Trial, Tried};
pub use org_admin::{Joining, OrgRoleChange, WorkspaceTrashing};
pub use page_copies::PageCopies;
pub use page_trash::PageRestore;
pub use pages::PageWrite;
pub use password_reset::PASSWORD_RESET_LIFETIME_MILLIS;
pub use search_docs::SearchDocument;
pub use search_outbox::{SearchChange, SearchIndexMark, SearchOutboxCounts, SearchTarget};
pub use search_text::{TextFound, TextSearch};
pub use server_lock::ServerLock;
pub use sharing::SharingChange;
pub use signups::{
    Confirmed, Proof, SIGNUP_LIFETIME_MILLIS, SIGNUP_LINK_LIFETIME_MILLIS, Signup, SignupLookup,
    SignupStart,
};
pub use trash::{DeletedBlock, DeletedContainer, Purged};
pub use uploads::{OpenUploads, PartRecord, UploadUsage};
pub use users::{AdminChange, InstanceRole, Preference};
pub use visibility::{VisibilityScope, VisibilityVersion};
pub use workspace_membership::MembershipChange;

use db::{ConnRef, Scope};
use observe::Watch;
use sqlx::types::Uuid;

/// How many connections a store keeps to a file or server database.
const MAX_CONNECTIONS: u32 = 10;

/// How long the read of [`Store::check_ready`] may run on PostgreSQL: less than the two
/// seconds the server's readiness check waits, so that the read ends by itself soon
/// after its caller gives it up, if not before, and does not keep its connection.
const READY_STATEMENT_TIMEOUT: Duration = Duration::from_millis(1500);

/// Whose rows the operations of a [`Store`] handle reach.
///
/// The rows of an organization are kept apart from the others' twice. The server
/// decides who may reach what, and that is the rule. Below it, on PostgreSQL, every
/// table that holds what is an organization's has a row-level security policy that
/// lets a connection reach the rows of one organization only, the one the handle
/// names; so an operation that forgot to ask whose rows it wants reaches none of
/// another organization. SQLite has no such policy, and holds one organization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Tenant {
    /// No organization: the tables of the organizations, and the table of the
    /// organizations itself, read as empty and take no row. What belongs to no
    /// organization (users, sessions) is reached as ever.
    #[default]
    Unset,
    /// The rows of this organization.
    Org(Uuid),
    /// The rows of every organization: for what the server does on nobody's behalf
    /// (purging the trash, the changes of the stored data after a migration) and for
    /// finding out which organization a request is about. A row that is stored under
    /// it must say whose it is; none is taken that does not.
    System,
}

impl Tenant {
    /// The rows of the organization with this identifier.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::InvalidId`] if the text is not an identifier.
    pub fn org(org_id: &str) -> Result<Self, StoreError> {
        if fvoci_model::is_id(org_id)
            && let Ok(uuid) = Uuid::try_parse(org_id)
        {
            Ok(Self::Org(uuid))
        } else {
            Err(StoreError::InvalidId(org_id.to_owned()))
        }
    }

    /// The identifier of the organization whose rows these are; `None` for the rows of
    /// none and of all.
    #[must_use]
    pub fn org_id(self) -> Option<String> {
        match self {
            Self::Org(org_id) => Some(org_id.hyphenated().to_string()),
            Self::Unset | Self::System => None,
        }
    }

    /// The statement that starts a transaction on PostgreSQL and tells its policies
    /// whose rows it reaches. The settings last as long as the transaction (the third
    /// argument of `set_config`), so nothing of them stays on the connection when it
    /// goes back to the pool; and both go to the server with the `BEGIN`, in one
    /// message.
    fn begin_on_postgres(self) -> sqlx::AssertSqlSafe<String> {
        // The identifier is a UUID as this function writes it: nothing of it can end
        // the string it is in.
        let (org, system) = match self {
            Self::Unset => (String::new(), "off"),
            Self::Org(org_id) => (org_id.hyphenated().to_string(), "off"),
            Self::System => (String::new(), "on"),
        };
        sqlx::AssertSqlSafe(format!(
            "BEGIN; SELECT set_config('{ORG_SETTING}', '{org}', true), \
             set_config('{SYSTEM_SETTING}', '{system}', true)"
        ))
    }
}

/// The settings of a PostgreSQL transaction that the policies of the schema read: the
/// organization whose rows it reaches, and `on` if it reaches those of all.
const ORG_SETTING: &str = "fvoci.org";
const SYSTEM_SETTING: &str = "fvoci.system";

/// How the connection pool of a [`Store`] stands, as [`Store::pool_status`] reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PoolStatus {
    /// The connections the pool holds, in use or idle.
    pub connections: u32,
    /// The connections the pool holds that are idle.
    pub idle: u32,
    /// The most connections the pool may hold.
    pub max_connections: u32,
}

/// A handle to the database.
///
/// Cloning is cheap and every clone uses the same connection pool. A handle reaches
/// the rows of its [`Tenant`]; [`Store::for_org`] and [`Store::system`] give handles
/// to the same database that reach other rows.
#[derive(Debug, Clone)]
pub struct Store {
    inner: Arc<Inner>,
    tenant: Tenant,
}

#[derive(Debug)]
struct Inner {
    pool: Pool,
    /// The queue of the transactions on an SQLite database; see [`Store::transaction`].
    sqlite_writers: tokio::sync::Mutex<()>,
    /// See [`Store::set_comment_versions_kept`].
    comment_versions_kept: std::sync::atomic::AtomicUsize,
    /// Whether changes are noted for the search server ([`Store::keep_search_outbox`]).
    search_outbox: std::sync::atomic::AtomicBool,
    /// See [`Store::set_body_versions_kept`].
    body_versions_kept: std::sync::atomic::AtomicU32,
    /// Who is told about the calls; see [`Store::observe`].
    observer: std::sync::OnceLock<Box<dyn Observer>>,
    visibility: visibility::Visibility,
    /// Whether rows name places of the file storage otherwise than the one way, once
    /// asked ([`Store::other_spellings_stored`]).
    other_spellings: tokio::sync::OnceCell<bool>,
    /// The file of the lock of the server of an SQLite database in a file
    /// ([`Store::try_lock_server`]).
    lock_file: Option<std::path::PathBuf>,
}

/// The connections. Nothing takes one but [`Store::with_conn`] and
/// [`Store::transaction`], which begin by telling PostgreSQL whose rows the transaction
/// reaches, and the migrator at the start. A transaction that was left open on a
/// connection (a request that went away while its `BEGIN` was under way) is therefore
/// harmless: the next operation on it sets both settings anew before anything else.
#[derive(Debug)]
enum Pool {
    Sqlite(SqlitePool),
    Postgres(PgPool),
}

impl Store {
    /// Connects to the database described by the server's `dbtype` and `dbconfig`
    /// settings and brings its schema up to date, as [`Store::migrate`] does: an empty
    /// database gets the whole schema, an older one the migrations it lacks. A role that
    /// does not own the tables opens only a schema that is up to date.
    ///
    /// The tables have fixed names. To keep them apart from other tables in one
    /// PostgreSQL database, give the server a schema of its own with `search_path` in
    /// `dbconfig`.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Config`] if `dbconfig` cannot be used, [`StoreError::Schema`]
    /// if the database is not this server's to open (one with this server's tables but
    /// no migration record, one migrated by a newer server) or the role may not read and
    /// write its tables, [`StoreError::SchemaNotOwned`] if the schema is to be changed and
    /// the role does not own the tables, and [`StoreError::Database`] if the database
    /// cannot be reached.
    pub async fn connect(db_type: DbType, dbconfig: &str) -> Result<Self, StoreError> {
        let store = Self::open(db_type, dbconfig).await?;
        store.migrate().await?;
        Ok(store)
    }

    /// Brings the schema of the database up to date, as [`Store::connect`] does: for a
    /// caller that looks at the connection ([`Store::role_past_the_wall`]) before
    /// anything is changed through it.
    ///
    /// A database whose schema is this build's already is left as it is, so that a role
    /// that does not own the tables may serve it once their owner brought them up to
    /// date. Then only the changes of the data the migrations left to the server are
    /// done, if any are still to do.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::SchemaNotOwned`] if the schema is to be changed and the
    /// role may not change it (it does not own the tables), [`StoreError::Schema`] if
    /// the database is not this server's to open or the role may not read and write
    /// its tables, and [`StoreError::Database`] if a statement fails.
    pub async fn migrate(&self) -> Result<(), StoreError> {
        let state = schema::state(self).await?;
        let may_change = schema::may_change(self).await?;
        let reason = match (state, may_change) {
            (schema::SchemaState::Current, true) => None,
            (schema::SchemaState::Current, false) => {
                let ungranted = schema::ungranted_tables(self).await?;
                if !ungranted.is_empty() {
                    let (role, schema) = schema::whereabouts(self).await?;
                    return Err(StoreError::Schema(format!(
                        "the role {role} may not read and write the tables {} of the schema \
                         {schema}: grant it SELECT, INSERT, UPDATE and DELETE on them \
                         (SELECT on _sqlx_migrations)",
                        ungranted.join(", ")
                    )));
                }
                None
            }
            (schema::SchemaState::Behind | schema::SchemaState::Foreign(_), true) => {
                schema::migrate(self, None).await?;
                None
            }
            (schema::SchemaState::Behind, false) => {
                Some("migrations of this server are not applied yet".to_owned())
            }
            (schema::SchemaState::Foreign(reason), false) => Some(reason),
        };
        if let Some(reason) = reason {
            let (role, schema) = schema::whereabouts(self).await?;
            return Err(StoreError::SchemaNotOwned {
                role,
                schema,
                reason,
            });
        }
        data_steps::run(self).await
    }

    /// Connects to the database and leaves its schema as it is. Nothing but
    /// [`Store::role_past_the_wall`] works on such a store before [`Store::migrate`].
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Config`] if `dbconfig` cannot be used, and
    /// [`StoreError::Database`] if the database cannot be reached.
    pub async fn open(db_type: DbType, dbconfig: &str) -> Result<Self, StoreError> {
        let mut lock_file = None;
        // What an abandoned write may still take to land (`visibility::settle_after`).
        let longest_wait = match db_type {
            DbType::Sqlite => connect::sqlite_settings(dbconfig)?.busy_timeout,
            DbType::Postgres => Duration::ZERO,
        };
        let pool = match db_type {
            DbType::Sqlite => {
                let settings = connect::sqlite_settings(dbconfig)?;
                let database =
                    (!settings.in_memory).then(|| settings.options.get_filename().to_path_buf());
                let options = if settings.in_memory {
                    // The database exists only while a connection to it does, and a single
                    // connection sidesteps the table-level locks of SQLite's shared cache.
                    SqlitePoolOptions::new()
                        .max_connections(1)
                        .min_connections(1)
                        .idle_timeout(None)
                        .max_lifetime(None)
                } else {
                    SqlitePoolOptions::new().max_connections(MAX_CONNECTIONS)
                };
                let pool = options.connect_with(settings.options).await?;
                // Once connected, the file of the database is there to be followed to.
                lock_file = database.as_deref().map(server_lock::file_beside);
                Pool::Sqlite(pool)
            }
            DbType::Postgres => {
                let settings = connect::postgres_settings(dbconfig)?;
                let mut options = PgPoolOptions::new().max_connections(MAX_CONNECTIONS);
                if let Some(timeout) = settings.connect_timeout {
                    options = options.acquire_timeout(timeout.max(Duration::from_secs(1)));
                }
                Pool::Postgres(options.connect_with(settings.options).await?)
            }
        };

        Ok(Self {
            tenant: Tenant::Unset,
            inner: Arc::new(Inner {
                pool,
                sqlite_writers: tokio::sync::Mutex::new(()),
                comment_versions_kept: std::sync::atomic::AtomicUsize::new(
                    usize::try_from(comment_versions::DEFAULT_COMMENT_VERSIONS_KEPT)
                        .unwrap_or(usize::MAX),
                ),
                search_outbox: std::sync::atomic::AtomicBool::new(false),
                body_versions_kept: std::sync::atomic::AtomicU32::new(
                    body_versions::DEFAULT_BODY_VERSIONS_KEPT,
                ),
                observer: std::sync::OnceLock::new(),
                visibility: visibility::Visibility::settling_for(visibility::settle_after(
                    longest_wait,
                )),
                other_spellings: tokio::sync::OnceCell::new(),
                lock_file,
            }),
        })
    }

    /// Tells `observer` about every call of this store and of every handle to the same
    /// database, from now on: how long it took, whether it wrote, how it ended, and how
    /// long it waited for a connection. A store has one observer; if it has one already,
    /// that one stays and `false` is returned.
    #[must_use]
    pub fn observe(&self, observer: Box<dyn Observer>) -> bool {
        self.inner.observer.set(observer).is_ok()
    }

    /// A call of `access` watched, if there is an observer.
    fn watch(&self, access: Access) -> Option<Watch<'_>> {
        Watch::start(
            self.inner.observer.get().map(Box::as_ref),
            self.db_type(),
            access,
        )
    }

    /// A handle to the same database that reaches the rows of `tenant`.
    #[must_use]
    pub fn scoped(&self, tenant: Tenant) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
            tenant,
        }
    }

    /// A handle to the same database that reaches the rows of one organization.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::InvalidId`] if the text is not an identifier.
    pub fn for_org(&self, org_id: &str) -> Result<Self, StoreError> {
        Ok(self.scoped(Tenant::org(org_id)?))
    }

    /// A handle to the same database that reaches the rows of every organization
    /// ([`Tenant::System`]). The files that may ask for one are listed in this crate's
    /// tests.
    #[must_use]
    pub fn system(&self) -> Self {
        self.scoped(Tenant::System)
    }

    /// Whose rows this handle reaches.
    pub fn tenant(&self) -> Tenant {
        self.tenant
    }

    /// Returns how the connection pool stands: the connections it holds, those of them
    /// that are idle, and how many it may hold.
    pub fn pool_status(&self) -> PoolStatus {
        let (size, idle, max) = match &self.inner.pool {
            Pool::Sqlite(pool) => (
                pool.size(),
                pool.num_idle(),
                pool.options().get_max_connections(),
            ),
            Pool::Postgres(pool) => (
                pool.size(),
                pool.num_idle(),
                pool.options().get_max_connections(),
            ),
        };
        // Read one after the other, the two can pass each other: no more are idle than
        // there are.
        PoolStatus {
            connections: size,
            idle: u32::try_from(idle).unwrap_or(u32::MAX).min(size),
            max_connections: max,
        }
    }

    /// Returns the kind of database this store is connected to.
    pub fn db_type(&self) -> DbType {
        match self.inner.pool {
            Pool::Sqlite(_) => DbType::Sqlite,
            Pool::Postgres(_) => DbType::Postgres,
        }
    }

    fn dialect(&self) -> Dialect {
        match self.inner.pool {
            Pool::Sqlite(_) => Dialect::Sqlite,
            Pool::Postgres(_) => Dialect::Postgres,
        }
    }

    /// Returns the version string the database reports about itself.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the query fails.
    pub async fn db_version(&self) -> Result<String, StoreError> {
        self.with_conn(async |scope| {
            let query = match scope.dialect() {
                Dialect::Sqlite => "SELECT sqlite_version()",
                Dialect::Postgres => "SELECT current_setting('server_version')",
            };
            let sql = scope.sql(query);
            match scope.fetch_optional(sql).await? {
                Some(row) => row.text(0),
                None => Ok(String::new()),
            }
        })
        .await
    }

    /// Reads one row of the sessions at most, to see that the database answers and holds
    /// the table every signed-in request reads: one connection, one read of the sessions
    /// (on PostgreSQL after setting its statement timeout). A
    /// database that answers but has lost this server's tables is not one it can serve
    /// from; the schema as a whole is not compared. On PostgreSQL the statement gives
    /// up after one and a half seconds, also while it waits for a lock on the table
    /// (a `VACUUM FULL`, a migration): a check its caller gave up does not keep its
    /// connection from the pool until the lock goes.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if no connection comes or a statement fails.
    pub async fn check_ready(&self) -> Result<(), StoreError> {
        self.with_conn(async |scope| {
            if scope.dialect() == Dialect::Postgres {
                // For this transaction only; a plain SELECT, so the call stays a read.
                let sql = scope.sql(&format!(
                    "SELECT set_config('statement_timeout', '{}ms', true)",
                    READY_STATEMENT_TIMEOUT.as_millis()
                ));
                scope.fetch_optional(sql).await?;
            }
            let sql = scope.sql("SELECT 1 FROM sessions LIMIT 1");
            scope.fetch_optional(sql).await.map(|_| ())
        })
        .await
    }

    /// Whether the database's schema is this build's already: every migration recorded
    /// as applied, with this build's text, and no other. Reads only.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Schema`] if the role may not read the record of the
    /// migrations, and [`StoreError::Database`] if it cannot be read.
    pub async fn schema_is_current(&self) -> Result<bool, StoreError> {
        Ok(schema::state(self).await? == schema::SchemaState::Current)
    }

    /// The role the store connects as and the schema it works in, for what a server
    /// says of them; empty on SQLite.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if they cannot be read.
    pub async fn role_and_schema(&self) -> Result<(String, String), StoreError> {
        schema::whereabouts(self).await
    }

    /// The name of the role the store connects as, if the wall between organizations
    /// does not hold it: a PostgreSQL superuser, or a role with `BYPASSRLS`, passes
    /// every row-level security policy. `None` for a role the wall holds, and on
    /// SQLite, which has no wall and one organization.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the query fails.
    pub async fn role_past_the_wall(&self) -> Result<Option<String>, StoreError> {
        if self.dialect() == Dialect::Sqlite {
            return Ok(None);
        }
        self.with_conn(async |scope| {
            let sql = scope.sql(
                "SELECT rolname::text FROM pg_roles \
                 WHERE rolname = current_user AND (rolsuper OR rolbypassrls)",
            );
            match scope.fetch_optional(sql).await? {
                Some(row) => row.text(0).map(Some),
                None => Ok(None),
            }
        })
        .await
    }

    /// Closes every connection. Operations started afterwards fail.
    pub async fn close(&self) {
        match &self.inner.pool {
            Pool::Sqlite(pool) => pool.close().await,
            Pool::Postgres(pool) => pool.close().await,
        }
    }

    /// Runs `operation` on one connection. For reads and for single-statement writes.
    ///
    /// On SQLite it runs outside a transaction. On PostgreSQL it runs in one, as
    /// [`Store::transaction`] does: only a transaction carries whose rows it reaches
    /// ([`Tenant`]) without leaving that on the connection. An operation that only read
    /// (every statement a plain `SELECT`) does not wait for its transaction to end: it
    /// is rolled back, which for a read is what a commit is.
    async fn with_conn<T>(
        &self,
        operation: impl AsyncFnOnce(&mut Scope<'_>) -> Result<T, StoreError>,
    ) -> Result<T, StoreError> {
        let watch = self.watch(Access::Read);
        let visibility = self.inner.visibility.writing(self.tenant);
        let (result, access) = self.on_conn(watch.as_ref(), &visibility, operation).await;
        drop(visibility);
        if let Some(watch) = watch {
            watch.finish(access, &result);
        }
        result
    }

    /// The work of [`Store::with_conn`], and whether a statement wrote.
    async fn on_conn<T>(
        &self,
        watch: Option<&Watch<'_>>,
        visibility: &visibility::Writing<'_>,
        operation: impl AsyncFnOnce(&mut Scope<'_>) -> Result<T, StoreError>,
    ) -> (Result<T, StoreError>, Access) {
        let kept = self.versions_kept();
        match &self.inner.pool {
            Pool::Sqlite(pool) => {
                let mut conn = match Watch::acquire(watch, pool).await {
                    Ok(conn) => conn,
                    Err(error) => return (Err(error.into()), Access::Read),
                };
                let mut scope = Scope::new(ConnRef::Sqlite(&mut conn), self.tenant, kept)
                    .noting_for_search(&self.inner.search_outbox)
                    .reporting_writes_to(watch.map(Watch::writes))
                    .tracking_visibility(visibility);
                let result = operation(&mut scope).await;
                visibility.settled();
                (result, Access::of(&scope))
            }
            Pool::Postgres(pool) => {
                let mut conn = match Watch::acquire(watch, pool).await {
                    Ok(conn) => conn,
                    Err(error) => return (Err(error.into()), Access::Read),
                };
                let mut tx = match conn.begin_with(self.tenant.begin_on_postgres()).await {
                    Ok(tx) => tx,
                    Err(error) => return (Err(error.into()), Access::Read),
                };
                let mut scope = Scope::new(ConnRef::Postgres(&mut tx), self.tenant, kept)
                    .noting_for_search(&self.inner.search_outbox)
                    .reporting_writes_to(watch.map(Watch::writes))
                    .tracking_visibility(visibility);
                let result = operation(&mut scope).await;
                let access = Access::of(&scope);
                if access == Access::Write {
                    (finish(result, tx, visibility).await, access)
                } else {
                    // Nothing to keep or to undo, so nothing to wait for. The dropped
                    // transaction is rolled back with the check the pool makes of
                    // every connection it takes back, in the one message that check
                    // is, behind the caller: the connection is back as soon as it was
                    // when a read was no transaction.
                    drop(tx);
                    (result, access)
                }
            }
        }
    }

    /// Runs `operation` in a transaction, committing if it succeeds and rolling back if
    /// it fails.
    async fn transaction<T>(
        &self,
        operation: impl AsyncFnOnce(&mut Scope<'_>) -> Result<T, StoreError>,
    ) -> Result<T, StoreError> {
        let watch = self.watch(Access::Write);
        let visibility = self.inner.visibility.writing(self.tenant);
        let result = self
            .in_transaction(watch.as_ref(), &visibility, operation)
            .await;
        drop(visibility);
        if let Some(watch) = watch {
            watch.finish(Access::Write, &result);
        }
        result
    }

    /// The work of [`Store::transaction`].
    async fn in_transaction<T>(
        &self,
        watch: Option<&Watch<'_>>,
        visibility: &visibility::Writing<'_>,
        operation: impl AsyncFnOnce(&mut Scope<'_>) -> Result<T, StoreError>,
    ) -> Result<T, StoreError> {
        let kept = self.versions_kept();
        match &self.inner.pool {
            Pool::Sqlite(pool) => {
                // One transaction at a time, in the order they come. SQLite has one
                // writer anyway, and a connection that waits for it only asks again now
                // and then until the busy timeout: under a steady stream of writers one
                // of them can lose every time and fail with "database is locked". The
                // queue is fair, and the busy timeout is left for what is outside it:
                // single statements and other processes.
                let _turn = self.inner.sqlite_writers.lock().await;
                // Take the write lock up front. A transaction that starts by reading and
                // then tries to write fails with SQLITE_BUSY as soon as another one does
                // the same, and no busy timeout can resolve that.
                let mut conn = Watch::acquire(watch, pool).await?;
                let mut tx = conn.begin_with("BEGIN IMMEDIATE").await?;
                let result = operation(
                    &mut Scope::new(ConnRef::Sqlite(&mut tx), self.tenant, kept)
                        .noting_for_search(&self.inner.search_outbox)
                        .tracking_visibility(visibility),
                )
                .await;
                finish(result, tx, visibility).await
            }
            Pool::Postgres(pool) => {
                let mut conn = Watch::acquire(watch, pool).await?;
                let mut tx = conn.begin_with(self.tenant.begin_on_postgres()).await?;
                let result = operation(
                    &mut Scope::new(ConnRef::Postgres(&mut tx), self.tenant, kept)
                        .noting_for_search(&self.inner.search_outbox)
                        .tracking_visibility(visibility),
                )
                .await;
                finish(result, tx, visibility).await
            }
        }
    }
}

/// Commits `tx` if the operation succeeded and rolls it back otherwise.
async fn finish<T, DB: sqlx::Database>(
    result: Result<T, StoreError>,
    tx: sqlx::Transaction<'_, DB>,
    visibility: &visibility::Writing<'_>,
) -> Result<T, StoreError> {
    match result {
        Ok(value) => {
            tx.commit().await?;
            visibility.settled();
            Ok(value)
        }
        Err(error) => {
            if let Err(rollback_error) = tx.rollback().await {
                // The operation's own error says why it failed; the rollback's is
                // logged with it, and leaves the write unsettled.
                tracing::error!(
                    error = %rollback_error,
                    cause = %error,
                    "transaction rollback failed"
                );
                return Err(error);
            }
            visibility.settled();
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What PostgreSQL is told at the start of a transaction, for each kind of handle:
    /// both settings every time, for the transaction only.
    #[test]
    fn a_transaction_begins_by_saying_whose_rows_it_reaches() {
        let begin = |tenant: Tenant| tenant.begin_on_postgres().0;
        assert_eq!(
            begin(Tenant::Unset),
            "BEGIN; SELECT set_config('fvoci.org', '', true), \
             set_config('fvoci.system', 'off', true)"
        );
        assert_eq!(
            begin(Tenant::org("01890000-0000-7000-8000-000000000001").expect("an identifier")),
            "BEGIN; SELECT set_config('fvoci.org', '01890000-0000-7000-8000-000000000001', true), \
             set_config('fvoci.system', 'off', true)"
        );
        assert_eq!(
            begin(Tenant::System),
            "BEGIN; SELECT set_config('fvoci.org', '', true), \
             set_config('fvoci.system', 'on', true)"
        );
    }

    #[test]
    fn only_an_identifier_names_an_organization() {
        for text in [
            "",
            "o1",
            "00000000-0000-0000-0000-000000000000",
            "'; DROP TABLE boards; --",
        ] {
            assert!(
                matches!(Tenant::org(text), Err(StoreError::InvalidId(_))),
                "{text:?}"
            );
        }
    }
}
