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
mod file_refs;
mod schema;
mod sessions;
mod sharing;
mod subscriptions;
mod teams;
#[cfg(feature = "test-util")]
pub mod testing;
mod trash;
mod users;

use std::sync::Arc;
use std::time::Duration;

use sqlx::postgres::{PgPool, PgPoolOptions};
use sqlx::sqlite::{SqlitePool, SqlitePoolOptions};

pub use blocks::{BlockFilter, DuplicateAs, Paging};
pub use boards::{Joined, MemberChange, PublicBoards};
pub use category_boards::BoardVisibility;
pub use connect::DbType;
pub use db::Dialect;
pub use error::{StoreError, Taken};
pub use sharing::SharingChange;
pub use trash::{DeletedBlock, DeletedBoard, Purged};
pub use users::{AdminChange, InstanceRole, Preference};

use db::{ConnRef, Scope};

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
    /// The queue of the transactions on an SQLite database; see [`Store::transaction`].
    sqlite_writers: tokio::sync::Mutex<()>,
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
                sqlite_writers: tokio::sync::Mutex::new(()),
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
        match &self.inner.pool {
            Pool::Sqlite(pool) => {
                let mut conn = pool.acquire().await?;
                operation(&mut Scope::new(ConnRef::Sqlite(&mut conn))).await
            }
            Pool::Postgres(pool) => {
                let mut conn = pool.acquire().await?;
                operation(&mut Scope::new(ConnRef::Postgres(&mut conn))).await
            }
        }
    }

    /// Runs `operation` in a transaction, committing if it succeeds and rolling back if
    /// it fails.
    async fn transaction<T>(
        &self,
        operation: impl AsyncFnOnce(&mut Scope<'_>) -> Result<T, StoreError>,
    ) -> Result<T, StoreError> {
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
                let result = operation(&mut Scope::new(ConnRef::Sqlite(&mut tx))).await;
                finish(result, tx).await
            }
            Pool::Postgres(pool) => {
                let mut tx = pool.begin().await?;
                let result = operation(&mut Scope::new(ConnRef::Postgres(&mut tx))).await;
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
