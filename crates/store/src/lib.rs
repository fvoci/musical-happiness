//! SQL storage for the fvoci server, on SQLite or PostgreSQL.
//!
//! [`Store`] is the one entry point. It is cheap to clone and safe to share between
//! tasks. Every method that writes more than one statement runs in a transaction on both
//! databases.
//!
//! The schema is this server's own: a list of migrations applied by sqlx's migrator. A
//! database of the original server is refused.

mod blocks;
mod boards;
mod boards_and_blocks;
mod categories;
mod category_boards;
mod comment_versions;
mod connect;
mod data_steps;
mod db;
mod directory;
mod error;
mod file_info;
mod file_refs;
mod schema;
mod sessions;
mod sharing;
mod subscriptions;
mod teams;
#[cfg(feature = "test-util")]
pub mod testing;
mod trash;
mod uploads;
mod users;

use std::sync::Arc;
use std::time::Duration;

use sqlx::postgres::{PgPool, PgPoolOptions};
use sqlx::sqlite::{SqlitePool, SqlitePoolOptions};

pub use blocks::{BlockFilter, DuplicateAs, Paging};
pub use boards::{Joined, MemberChange, PublicBoards};
pub use category_boards::BoardVisibility;
pub use comment_versions::DEFAULT_COMMENT_VERSIONS_KEPT;
pub use connect::DbType;
pub use db::Dialect;
pub use error::{StoreError, Taken};
pub use sharing::SharingChange;
pub use trash::{DeletedBlock, DeletedBoard, Purged};
pub use uploads::{PartRecord, UploadUsage};
pub use users::{AdminChange, InstanceRole, Preference};

use db::{ConnRef, Scope};
use sqlx::types::Uuid;

/// How many connections a store keeps to a file or server database.
const MAX_CONNECTIONS: u32 = 10;

/// Whose rows the operations of a [`Store`] handle reach.
///
/// The rows of an organization are kept apart from the others' twice. The server
/// decides who may reach what, and that is the rule. Below it, on PostgreSQL, every
/// table that holds what is an organization's has a row-level security policy that
/// lets a connection reach the rows of one organization only, the one the handle
/// names; so an operation that forgot to ask whose rows it wants reaches none of
/// another organization. SQLite has no such policy, and holds one organization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tenant {
    /// No organization: the tables of the organizations read as empty and take no row.
    /// What belongs to no organization (users, sessions) is reached as ever.
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
}

#[derive(Debug)]
enum Pool {
    Sqlite(SqlitePool),
    Postgres(PgPool),
}

impl Store {
    /// Connects to the database described by the server's `dbtype` and `dbconfig`
    /// settings and brings its schema up to date: an empty database gets the whole
    /// schema, an older one the migrations it lacks.
    ///
    /// The tables have fixed names. To keep them apart from other tables in one
    /// PostgreSQL database, give the server a schema of its own with `search_path` in
    /// `dbconfig`.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Config`] if `dbconfig` cannot be used, [`StoreError::Schema`]
    /// if the database is not this server's to open (one of the original server, one
    /// migrated by a newer server), and [`StoreError::Database`] if the database cannot
    /// be reached.
    pub async fn connect(db_type: DbType, dbconfig: &str) -> Result<Self, StoreError> {
        let pool = match db_type {
            DbType::Sqlite => {
                let settings = connect::sqlite_settings(dbconfig)?;
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
                Pool::Sqlite(options.connect_with(settings.options).await?)
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

        let store = Self {
            tenant: Tenant::Unset,
            inner: Arc::new(Inner {
                pool,
                sqlite_writers: tokio::sync::Mutex::new(()),
                comment_versions_kept: std::sync::atomic::AtomicUsize::new(
                    usize::try_from(comment_versions::DEFAULT_COMMENT_VERSIONS_KEPT)
                        .unwrap_or(usize::MAX),
                ),
            }),
        };
        schema::migrate(&store).await?;
        data_steps::run(&store).await?;
        Ok(store)
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
        let kept = self.comment_versions_kept();
        match &self.inner.pool {
            Pool::Sqlite(pool) => {
                let mut conn = pool.acquire().await?;
                operation(&mut Scope::new(ConnRef::Sqlite(&mut conn), kept)).await
            }
            Pool::Postgres(pool) => {
                let mut tx = pool.begin_with(self.tenant.begin_on_postgres()).await?;
                let mut scope = Scope::new(ConnRef::Postgres(&mut tx), kept);
                let result = operation(&mut scope).await;
                if scope.wrote() {
                    finish(result, tx).await
                } else {
                    // Nothing to keep or to undo, so nothing to wait for. The dropped
                    // transaction is rolled back with the check the pool makes of
                    // every connection it takes back, in the one message that check
                    // is, behind the caller: the connection is back as soon as it was
                    // when a read was no transaction.
                    drop(tx);
                    result
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
        let kept = self.comment_versions_kept();
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
                let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
                let result = operation(&mut Scope::new(ConnRef::Sqlite(&mut tx), kept)).await;
                finish(result, tx).await
            }
            Pool::Postgres(pool) => {
                let mut tx = pool.begin_with(self.tenant.begin_on_postgres()).await?;
                let result = operation(&mut Scope::new(ConnRef::Postgres(&mut tx), kept)).await;
                finish(result, tx).await
            }
        }
    }
}

/// Commits `tx` if the operation succeeded and rolls it back otherwise.
async fn finish<T, DB: sqlx::Database>(
    result: Result<T, StoreError>,
    tx: sqlx::Transaction<'static, DB>,
) -> Result<T, StoreError> {
    match result {
        Ok(value) => {
            tx.commit().await?;
            Ok(value)
        }
        Err(error) => {
            if let Err(rollback_error) = tx.rollback().await {
                tracing::error!(error = %rollback_error, "transaction rollback failed");
            }
            Err(error)
        }
    }
}
