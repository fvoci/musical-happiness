//! Fresh databases for tests, on SQLite or PostgreSQL. Behind the `test-util` feature.
//!
//! Tests run against SQLite by default. Set `FVOCI_STORE_TEST_DB_TYPE=postgres` and
//! `FVOCI_STORE_TEST_CONN_STRING` to a PostgreSQL URL to run them against PostgreSQL;
//! every [`TestDatabase`] is then a schema of its own in that database.
//!
//! The URL must name a role that is no superuser, is not exempt from row-level
//! security (`NOBYPASSRLS`) and may make schemas in the database (`CREATE`): the
//! policies of the schema pass a role that is either, and tests run as one would show
//! nothing of them. A test database is not made for such a role.

use std::path::{Path, PathBuf};

use fvoci_model::new_id;
use sqlx::Connection as _;
use sqlx::postgres::PgConnection;

use crate::error::StoreError;
use crate::{DbType, Store};

/// The environment variable that selects the database kind for tests.
pub const DB_TYPE_ENV: &str = "FVOCI_STORE_TEST_DB_TYPE";

/// The environment variable that holds the PostgreSQL URL for tests.
pub const CONN_STRING_ENV: &str = "FVOCI_STORE_TEST_CONN_STRING";

/// Which database the tests run against, as the environment says ([`DB_TYPE_ENV`]).
///
/// # Panics
///
/// Panics if the variable names no kind of database.
pub fn db_type_of_the_tests() -> DbType {
    match std::env::var(DB_TYPE_ENV) {
        Ok(kind) if !kind.trim().is_empty() => kind
            .trim()
            .parse()
            .unwrap_or_else(|error| panic!("{DB_TYPE_ENV}: {error}")),
        _ => DbType::Sqlite,
    }
}

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
        let db_type = db_type_of_the_tests();
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
                let mut connection = connect(&url).await;
                refuse_a_role_the_policies_pass(&mut connection).await;
                run(&mut connection, format!("CREATE SCHEMA {schema}")).await;
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
                let mut connection = connect(&url).await;
                run(&mut connection, format!("DROP SCHEMA {schema} CASCADE")).await;
            }
        }
    }
}

async fn connect(url: &str) -> PgConnection {
    PgConnection::connect(url)
        .await
        .unwrap_or_else(|error| panic!("connecting to the test PostgreSQL server: {error}"))
}

/// Runs one statement.
async fn run(connection: &mut PgConnection, statement: String) {
    // The statement is built from a generated identifier, not from input.
    sqlx::raw_sql(sqlx::AssertSqlSafe(statement))
        .execute(connection)
        .await
        .unwrap_or_else(|error| panic!("preparing the test database: {error}"));
}

/// Panics if the tests connect as a role that the row-level security policies pass.
async fn refuse_a_role_the_policies_pass(connection: &mut PgConnection) {
    let (role, superuser, bypasses): (String, bool, bool) = sqlx::query_as(
        "SELECT rolname::text, rolsuper, rolbypassrls FROM pg_roles WHERE rolname = current_user",
    )
    .fetch_one(connection)
    .await
    .unwrap_or_else(|error| panic!("reading the role of the test connection: {error}"));
    assert!(
        !superuser && !bypasses,
        "{CONN_STRING_ENV} names the role {role} (superuser: {superuser}, BYPASSRLS: \
         {bypasses}). The tests need a role that is no superuser, has NOBYPASSRLS and has \
         CREATE on the database; `cargo xtask test` makes one in its container"
    );
}

/// The tables that are no organization's; every other table of the schema is behind
/// the wall between organizations.
pub const GLOBAL_TABLES: [&str; 7] = crate::schema::GLOBAL_TABLES;

/// Brings a test database to the schema as a server left it that knew nothing of
/// organizations: every migration before theirs, and none of the changes of the data
/// that follow the migrations.
///
/// # Panics
///
/// Panics if the database cannot be reached or migrated.
pub async fn migrate_to_before_organizations(database: &TestDatabase) {
    let store = Store::open(database.db_type(), database.dbconfig())
        .await
        .unwrap_or_else(|error| panic!("connecting to the test database: {error}"));
    crate::schema::migrate(&store, Some(crate::schema::ORGANIZATIONS_VERSION - 1))
        .await
        .unwrap_or_else(|error| panic!("migrating the test database: {error}"));
    store.close().await;
}

impl Store {
    /// What the idle connections of a PostgreSQL store carry of an organization, read
    /// past the store's own transactions: for each connection the settings `fvoci.org`
    /// and `fvoci.system` as it has them, empty for none. Empty on SQLite.
    ///
    /// # Panics
    ///
    /// Panics if a connection cannot be taken or read.
    pub async fn settings_left_on_the_connections(&self) -> Vec<(String, String)> {
        let crate::Pool::Postgres(pool) = &self.inner.pool else {
            return Vec::new();
        };
        // All of them at once, so that each is looked at.
        let mut connections = Vec::new();
        for _ in 0..pool.size() {
            let connection = pool
                .acquire()
                .await
                .unwrap_or_else(|error| panic!("taking a connection: {error}"));
            connections.push(connection);
        }
        let mut settings = Vec::new();
        for connection in &mut connections {
            let left: (String, String) = sqlx::query_as(
                "SELECT COALESCE(current_setting('fvoci.org', true), ''), \
                 COALESCE(current_setting('fvoci.system', true), '')",
            )
            .fetch_one(&mut **connection)
            .await
            .unwrap_or_else(|error| panic!("reading the settings of a connection: {error}"));
            settings.push(left);
        }
        settings
    }

    /// How many rows of a table this handle reaches, with no condition: for the tests
    /// of the wall between organizations.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the statement fails.
    pub async fn count_rows(&self, table: &str) -> Result<i64, StoreError> {
        self.with_conn(async |scope| {
            let sql = scope.sql(&format!("SELECT count(*) FROM {table}"));
            match scope.fetch_optional(sql).await? {
                Some(row) => row.int(0),
                None => Ok(0),
            }
        })
        .await
    }

    /// Runs one statement as this handle does its own, and returns how many rows it
    /// changed: for the tests of what the database refuses whatever the server asks.
    /// Nothing checks the text.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the statement fails.
    pub async fn execute_unchecked(&self, statement: &str) -> Result<u64, StoreError> {
        self.with_conn(async |scope| {
            let sql = scope.sql(statement);
            scope.execute(sql).await
        })
        .await
    }
}
