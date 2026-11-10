//! Fresh databases for tests, on SQLite or PostgreSQL. Behind the `test-util` feature.
//!
//! Tests run against SQLite by default. Set `FVOCI_STORE_TEST_DB_TYPE=postgres` and
//! `FVOCI_STORE_TEST_CONN_STRING` to a PostgreSQL URL to run them against PostgreSQL;
//! every [`TestDatabase`] is then a schema of its own in that database.

use std::path::{Path, PathBuf};

use fvoci_model::new_id;
use sqlx::Connection as _;
use sqlx::postgres::PgConnection;

use crate::DbType;

/// The environment variable that selects the database kind for tests.
pub const DB_TYPE_ENV: &str = "FVOCI_STORE_TEST_DB_TYPE";

/// The environment variable that holds the PostgreSQL URL for tests.
pub const CONN_STRING_ENV: &str = "FVOCI_STORE_TEST_CONN_STRING";

/// An empty database that belongs to one test.
#[derive(Debug)]
pub struct TestDatabase {
    db_type: DbType,
    dbconfig: String,
    cleanup: Cleanup,
}

#[derive(Debug)]
enum Cleanup {
    SqliteFile(PathBuf),
    PostgresSchema { url: String, schema: String },
}

impl TestDatabase {
    /// Creates an empty database: a new SQLite file under `directory`, or a new schema in
    /// the PostgreSQL database that the environment names.
    ///
    /// # Panics
    ///
    /// Panics if the environment asks for PostgreSQL without giving a URL, or the
    /// database cannot be created. This is test support; a broken setup should fail the
    /// test loudly.
    pub async fn create(directory: &Path) -> Self {
        // An identifier has hyphens; a file name takes them, a schema name does not.
        let name = new_id().replace('-', "_");
        let db_type = match std::env::var(DB_TYPE_ENV) {
            Ok(kind) if !kind.trim().is_empty() => kind
                .trim()
                .parse()
                .unwrap_or_else(|error| panic!("{DB_TYPE_ENV}: {error}")),
            _ => DbType::Sqlite,
        };
        match db_type {
            DbType::Sqlite => {
                std::fs::create_dir_all(directory)
                    .unwrap_or_else(|error| panic!("creating {}: {error}", directory.display()));
                let path = directory.join(format!("{name}.db"));
                Self {
                    db_type,
                    dbconfig: format!("{}?_busy_timeout=5000", path.display()),
                    cleanup: Cleanup::SqliteFile(path),
                }
            }
            DbType::Postgres => {
                let url = std::env::var(CONN_STRING_ENV).unwrap_or_else(|_| {
                    panic!("{CONN_STRING_ENV} must hold a PostgreSQL URL for PostgreSQL test runs")
                });
                let schema = format!("t_{name}");
                run_admin(&url, format!("CREATE SCHEMA {schema}")).await;
                let separator = if url.contains('?') { '&' } else { '?' };
                Self {
                    db_type,
                    dbconfig: format!("{url}{separator}search_path={schema}"),
                    cleanup: Cleanup::PostgresSchema { url, schema },
                }
            }
        }
    }

    /// The kind of database this is.
    pub fn db_type(&self) -> DbType {
        self.db_type
    }

    /// The `dbconfig` string that connects to this database.
    pub fn dbconfig(&self) -> &str {
        &self.dbconfig
    }

    /// The path of the database file, if this is an SQLite database.
    pub fn sqlite_path(&self) -> Option<&Path> {
        match &self.cleanup {
            Cleanup::SqliteFile(path) => Some(path),
            Cleanup::PostgresSchema { .. } => None,
        }
    }

    /// Removes the database. Close every store connected to it first.
    ///
    /// # Panics
    ///
    /// Panics if a PostgreSQL schema cannot be dropped.
    pub async fn remove(self) {
        match self.cleanup {
            Cleanup::SqliteFile(path) => {
                // Journal files may or may not exist; a leftover file is harmless.
                let _ = std::fs::remove_file(path);
            }
            Cleanup::PostgresSchema { url, schema } => {
                run_admin(&url, format!("DROP SCHEMA {schema} CASCADE")).await;
            }
        }
    }
}

/// Runs one statement on a connection of its own.
async fn run_admin(url: &str, statement: String) {
    let mut connection = PgConnection::connect(url)
        .await
        .unwrap_or_else(|error| panic!("connecting to the test PostgreSQL server: {error}"));
    // The statement is built from a generated identifier, not from input.
    sqlx::raw_sql(sqlx::AssertSqlSafe(statement))
        .execute(&mut connection)
        .await
        .unwrap_or_else(|error| panic!("preparing the test database: {error}"));
}
