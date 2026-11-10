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
mod connect;
mod db;
mod error;
mod file_info;
mod schema;
mod sessions;
mod sharing;
mod subscriptions;
mod system_settings;
mod teams;
#[cfg(feature = "test-util")]
pub mod testing;
mod users;

use std::sync::Arc;
use std::time::Duration;

use sqlx::postgres::{PgPool, PgPoolOptions};
use sqlx::sqlite::{SqlitePool, SqlitePoolOptions};

pub use blocks::{BlockFilter, DuplicateAs, HistoryOrder, HistoryQuery, Paging, SubtreeQuery};
pub use boards::{BoardMemberHistoryEntry, PublicBoards};
pub use category_boards::BoardVisibility;
pub use connect::DbType;
pub use db::Dialect;
pub use error::StoreError;
pub use users::Preference;

use db::{ConnRef, Scope, Shared};

/// How many connections a store keeps to a file or server database.
const MAX_CONNECTIONS: u32 = 10;

/// A handle to the database.
///
/// Cloning is cheap and every clone uses the same connection pool.
#[derive(Debug, Clone)]
pub struct Store {
    inner: Arc<Inner>,
}

#[derive(Debug)]
struct Inner {
    pool: Pool,
    shared: Shared,
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
            inner: Arc::new(Inner {
                pool,
                shared: Shared::new(),
            }),
        };
        schema::migrate(&store).await?;
        Ok(store)
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

    /// Runs `operation` on one connection, outside a transaction. For reads and for
    /// single-statement writes.
    async fn with_conn<T>(
        &self,
        operation: impl AsyncFnOnce(&mut Scope<'_>) -> Result<T, StoreError>,
    ) -> Result<T, StoreError> {
        let shared = &self.inner.shared;
        match &self.inner.pool {
            Pool::Sqlite(pool) => {
                let mut conn = pool.acquire().await?;
                operation(&mut Scope::new(ConnRef::Sqlite(&mut conn), shared)).await
            }
            Pool::Postgres(pool) => {
                let mut conn = pool.acquire().await?;
                operation(&mut Scope::new(ConnRef::Postgres(&mut conn), shared)).await
            }
        }
    }

    /// Runs `operation` in a transaction, committing if it succeeds and rolling back if
    /// it fails.
    async fn transaction<T>(
        &self,
        operation: impl AsyncFnOnce(&mut Scope<'_>) -> Result<T, StoreError>,
    ) -> Result<T, StoreError> {
        let shared = &self.inner.shared;
        match &self.inner.pool {
            Pool::Sqlite(pool) => {
                // Take the write lock up front. A transaction that starts by reading and
                // then tries to write fails with SQLITE_BUSY as soon as another one does
                // the same, and no busy timeout can resolve that.
                let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
                let result = operation(&mut Scope::new(ConnRef::Sqlite(&mut tx), shared)).await;
                finish(result, tx).await
            }
            Pool::Postgres(pool) => {
                let mut tx = pool.begin().await?;
                let result = operation(&mut Scope::new(ConnRef::Postgres(&mut tx), shared)).await;
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
