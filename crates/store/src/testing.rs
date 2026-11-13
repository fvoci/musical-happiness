//! Fresh databases for tests, on SQLite or PostgreSQL. Behind the `test-util` feature.
//!
//! Tests run against SQLite by default. Set `FVOCI_STORE_TEST_DB_TYPE=postgres` and
//! `FVOCI_STORE_TEST_CONN_STRING` to a PostgreSQL URL to run them against PostgreSQL;
//! every [`TestDatabase`] is then a schema of its own in that database.
//!
//! The URL must name a role that is no superuser, is not exempt from row-level
//! security (`NOBYPASSRLS`) and may make schemas in the database (`CREATE`): the
//! policies of the schema pass a role that is either, and tests run as one would show
//! nothing of them. A test database is not made for such a role. The few tests of
//! what the server makes of such a role need the URL of a superuser of the same
//! database besides, in `FVOCI_STORE_TEST_SUPERUSER_CONN_STRING`.

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

/// The environment variable that holds the PostgreSQL URL of a superuser, for the tests
/// of what the server makes of a role that the wall between organizations does not
/// hold. `cargo xtask test` sets it to the superuser of its container.
pub const SUPERUSER_CONN_STRING_ENV: &str = "FVOCI_STORE_TEST_SUPERUSER_CONN_STRING";

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

/// As whom a test database is made and connected to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    /// The role of [`CONN_STRING_ENV`], which the policies hold.
    HeldByTheWall,
    /// The role of [`SUPERUSER_CONN_STRING_ENV`], which they do not.
    Superuser,
    /// A role made for the one test database, that is no superuser and has
    /// `BYPASSRLS`: the policies do not hold it either.
    BypassRls,
}

#[derive(Debug)]
enum Cleanup {
    SqliteFile(PathBuf),
    PostgresSchema {
        url: String,
        schema: String,
        /// The role that was made with the schema and goes with it.
        role: Option<String>,
    },
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
        Self::create_as(directory, Role::HeldByTheWall).await
    }

    /// Creates an empty database as [`TestDatabase::create`] does, but on PostgreSQL as
    /// a superuser, whom no policy holds: for the tests of what the server makes of
    /// such a role, and for nothing else. On SQLite it is a database like any other.
    ///
    /// # Panics
    ///
    /// Panics as [`TestDatabase::create`] does, and if the environment asks for
    /// PostgreSQL without giving the URL of a superuser
    /// ([`SUPERUSER_CONN_STRING_ENV`]).
    pub async fn create_as_superuser(directory: &Path) -> Self {
        Self::create_as(directory, Role::Superuser).await
    }

    /// Creates an empty database as [`TestDatabase::create`] does, but on PostgreSQL as
    /// a role of its own that is no superuser and has `BYPASSRLS`, the other kind of
    /// role that no policy holds. The role is made by the superuser of
    /// [`SUPERUSER_CONN_STRING_ENV`] and removed with the database.
    ///
    /// # Panics
    ///
    /// Panics as [`TestDatabase::create_as_superuser`] does.
    pub async fn create_as_role_with_bypassrls(directory: &Path) -> Self {
        Self::create_as(directory, Role::BypassRls).await
    }

    async fn create_as(directory: &Path, role: Role) -> Self {
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
                let variable = match role {
                    Role::HeldByTheWall => CONN_STRING_ENV,
                    Role::Superuser | Role::BypassRls => SUPERUSER_CONN_STRING_ENV,
                };
                let url = std::env::var(variable).unwrap_or_else(|_| {
                    panic!("{variable} must hold a PostgreSQL URL for PostgreSQL test runs")
                });
                let schema = format!("t_{name}");
                let mut connection = connect(&url).await;
                let past_the_wall = role_is_past_the_wall(&mut connection).await;
                match role {
                    Role::HeldByTheWall => assert!(
                        !past_the_wall,
                        "{CONN_STRING_ENV} names a superuser or a role with BYPASSRLS. The \
                         tests need a role that is no superuser, has NOBYPASSRLS and has \
                         CREATE on the database; `cargo xtask test` makes one in its container"
                    ),
                    Role::Superuser | Role::BypassRls => assert!(
                        past_the_wall,
                        "{SUPERUSER_CONN_STRING_ENV} names a role the policies hold; the \
                         tests that use it need a superuser"
                    ),
                }
                let (own_role, connect_as) = if role == Role::BypassRls {
                    // The role passes every policy of the cluster: its password is its
                    // own, made here and kept nowhere but in the test's `dbconfig`.
                    let own_role = format!("r_{name}");
                    let password = new_id();
                    run(
                        &mut connection,
                        format!(
                            "CREATE ROLE {own_role} LOGIN PASSWORD '{password}' \
                             NOSUPERUSER BYPASSRLS; \
                             CREATE SCHEMA {schema} AUTHORIZATION {own_role}"
                        ),
                    )
                    .await;
                    let connect_as = url_as_role(&url, &own_role, &password);
                    let mut as_the_role = connect(&connect_as).await;
                    let (superuser, bypasses): (bool, bool) = sqlx::query_as(
                        "SELECT rolsuper, rolbypassrls FROM pg_roles WHERE rolname = current_user",
                    )
                    .fetch_one(&mut as_the_role)
                    .await
                    .unwrap_or_else(|error| panic!("reading the role that was made: {error}"));
                    assert!(
                        !superuser && bypasses,
                        "the role that was made is no superuser and has BYPASSRLS"
                    );
                    (Some(own_role), connect_as)
                } else {
                    run(&mut connection, format!("CREATE SCHEMA {schema}")).await;
                    (None, url.clone())
                };
                let separator = if connect_as.contains('?') { '&' } else { '?' };
                Self {
                    db_type,
                    dbconfig: format!("{connect_as}{separator}search_path={schema}"),
                    cleanup: Cleanup::PostgresSchema {
                        url,
                        schema,
                        role: own_role,
                    },
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
            Cleanup::PostgresSchema { url, schema, role } => {
                let mut connection = connect(&url).await;
                run(&mut connection, format!("DROP SCHEMA {schema} CASCADE")).await;
                if let Some(role) = role {
                    run(&mut connection, format!("DROP ROLE {role}")).await;
                }
            }
        }
    }
}

/// The URL with a role that a test database made for itself and the role's password,
/// in place of those it names.
///
/// # Panics
///
/// Panics if the URL names no role: one of a superuser does.
fn url_as_role(url: &str, role: &str, password: &str) -> String {
    let (scheme, rest) = url
        .split_once("://")
        .unwrap_or_else(|| panic!("{SUPERUSER_CONN_STRING_ENV} is no URL"));
    // The role and its password end at the last `@` before the path or the query.
    let authority_end = rest.find(['/', '?']).unwrap_or(rest.len());
    let at = rest[..authority_end]
        .rfind('@')
        .unwrap_or_else(|| panic!("{SUPERUSER_CONN_STRING_ENV} names no role"));
    format!("{scheme}://{role}:{password}@{}", &rest[at + 1..])
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

/// Whether the connection's role passes the row-level security policies: a superuser,
/// or a role with `BYPASSRLS`.
async fn role_is_past_the_wall(connection: &mut PgConnection) -> bool {
    sqlx::query_scalar("SELECT rolsuper OR rolbypassrls FROM pg_roles WHERE rolname = current_user")
        .fetch_one(connection)
        .await
        .unwrap_or_else(|error| panic!("reading the role of the test connection: {error}"))
}

/// The tables that are no organization's; every other table of the schema is behind
/// the wall between organizations.
pub const GLOBAL_TABLES: [&str; 7] = crate::schema::GLOBAL_TABLES;

/// The tables that hold rows of organizations, and the organizations themselves, each
/// behind the wall on PostgreSQL.
pub const ORG_TABLES: [&str; 26] = crate::schema::ORG_TABLES;

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

/// Brings a test database to the schema as the server of the organizations left it,
/// before the keys that name the organization (migration 15): every migration up to
/// the organizations', and none of the changes of the data that follow the migrations.
///
/// # Panics
///
/// Panics if the database cannot be reached or migrated.
pub async fn migrate_to_the_organizations(database: &TestDatabase) {
    let store = Store::open(database.db_type(), database.dbconfig())
        .await
        .unwrap_or_else(|error| panic!("connecting to the test database: {error}"));
    crate::schema::migrate(&store, Some(crate::schema::ORGANIZATIONS_VERSION))
        .await
        .unwrap_or_else(|error| panic!("migrating the test database: {error}"));
    store.close().await;
}

/// Brings a test database to the schema as a server left it before the workspaces were
/// named so (migration 17): every migration before that one, and none of the changes of
/// the data that follow the migrations.
///
/// # Panics
///
/// Panics if the database cannot be reached or migrated.
pub async fn migrate_to_before_workspaces(database: &TestDatabase) {
    let store = Store::open(database.db_type(), database.dbconfig())
        .await
        .unwrap_or_else(|error| panic!("connecting to the test database: {error}"));
    crate::schema::migrate(&store, Some(crate::schema::WORKSPACES_VERSION - 1))
        .await
        .unwrap_or_else(|error| panic!("migrating the test database: {error}"));
    store.close().await;
}

/// Brings a test database to the schema of the migration that named the workspaces so
/// (migration 17), and none after it.
///
/// # Panics
///
/// Panics if the database cannot be reached or migrated.
pub async fn migrate_to_the_workspaces(database: &TestDatabase) {
    let store = Store::open(database.db_type(), database.dbconfig())
        .await
        .unwrap_or_else(|error| panic!("connecting to the test database: {error}"));
    crate::schema::migrate(&store, Some(crate::schema::WORKSPACES_VERSION))
        .await
        .unwrap_or_else(|error| panic!("migrating the test database: {error}"));
    store.close().await;
}

/// Brings a test database to the schema as a server left it before the organization's
/// management (migration 23): every migration before that one, and none of the changes
/// of the data that follow the migrations.
///
/// # Panics
///
/// Panics if the database cannot be reached or migrated.
pub async fn migrate_to_before_org_management(database: &TestDatabase) {
    let store = Store::open(database.db_type(), database.dbconfig())
        .await
        .unwrap_or_else(|error| panic!("connecting to the test database: {error}"));
    crate::schema::migrate(&store, Some(crate::schema::ORG_MANAGEMENT_VERSION - 1))
        .await
        .unwrap_or_else(|error| panic!("migrating the test database: {error}"));
    store.close().await;
}

/// Migrates a test database to the last schema before operator audit attribution.
///
/// # Panics
///
/// Panics if the database cannot be reached or migrated.
pub async fn migrate_to_before_audit_actors(database: &TestDatabase) {
    let store = Store::open(database.db_type(), database.dbconfig())
        .await
        .unwrap_or_else(|error| panic!("connecting to the test database: {error}"));
    crate::schema::migrate(&store, Some(26))
        .await
        .unwrap_or_else(|error| panic!("migrating the test database: {error}"));
    store.close().await;
}

/// Migrates a test database to the last schema before sessions kept their origin.
///
/// # Panics
///
/// Panics if the database cannot be reached or migrated.
pub async fn migrate_to_before_session_origin(database: &TestDatabase) {
    let store = Store::open(database.db_type(), database.dbconfig())
        .await
        .unwrap_or_else(|error| panic!("connecting to the test database: {error}"));
    crate::schema::migrate(&store, Some(27))
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

    /// Whether the handle's organization is the instance's own, asked in a transaction
    /// as a change of a role asks it, and how many organizations that transaction
    /// reaches afterwards: for the test that the wall is closed again after it.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if a statement fails.
    pub async fn instances_organization_and_reach(&self) -> Result<(bool, i64), StoreError> {
        self.transaction(async |scope| {
            let answer = crate::directory::is_the_instances_organization(scope).await?;
            let sql = scope.sql("SELECT count(*) FROM organizations");
            let reached = scope.fetch_optional(sql).await?;
            Ok((answer, reached.map_or(Ok(0), |row| row.int(0))?))
        })
        .await
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

    /// Runs one query as this handle does its own, and returns the first column of
    /// every row, which is text (`CAST(… AS TEXT)` for one that is not): for the tests
    /// of what is in a table, such as that no token is. Nothing checks the text.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the query fails.
    pub async fn texts_unchecked(&self, query: &str) -> Result<Vec<String>, StoreError> {
        self.with_conn(async |scope| {
            let sql = scope.sql(query);
            scope
                .fetch_all(sql)
                .await?
                .iter()
                .map(|row| row.text(0))
                .collect()
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
