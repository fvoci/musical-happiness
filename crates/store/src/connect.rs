//! Turning the server's `dbtype` and `dbconfig` settings into connection options.
//!
//! `dbconfig` keeps the form that existing configuration files use, so they keep
//! working: a file path with optional `?_busy_timeout=...` for
//! SQLite, and a `postgres://` URL or a `key=value` list for PostgreSQL. Parameters this
//! server does not understand are an error rather than silently dropped, so a setting
//! that has no effect is noticed at startup.

use std::str::FromStr;
use std::time::Duration;

use sqlx::postgres::{PgConnectOptions, PgSslMode};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode};

use crate::error::StoreError;

/// The kind of database a store connects to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DbType {
    /// SQLite, the `sqlite3` setting.
    Sqlite,
    /// PostgreSQL, the `postgres` setting.
    Postgres,
}

impl DbType {
    /// Returns the name used in the `dbtype` setting.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Sqlite => "sqlite3",
            Self::Postgres => "postgres",
        }
    }
}

impl FromStr for DbType {
    type Err = StoreError;

    fn from_str(name: &str) -> Result<Self, Self::Err> {
        match name {
            "sqlite3" => Ok(Self::Sqlite),
            "postgres" => Ok(Self::Postgres),
            "mysql" => Err(StoreError::Config(
                "dbtype \"mysql\" is not supported; use \"sqlite3\" or \"postgres\"".to_owned(),
            )),
            other => Err(StoreError::Config(format!(
                "unknown dbtype {other:?}; use \"sqlite3\" or \"postgres\""
            ))),
        }
    }
}

/// Connection settings for SQLite derived from a `dbconfig` string.
#[derive(Debug, Clone)]
pub(crate) struct SqliteSettings {
    pub(crate) options: SqliteConnectOptions,
    /// The database lives in memory and disappears with its last connection.
    pub(crate) in_memory: bool,
}

/// Parses a SQLite `dbconfig`: `[file:]<path>[?<param>=<value>&...]`.
///
/// Recognized parameters are `_busy_timeout` / `_timeout` (milliseconds), `cache`
/// (`shared` or `private`), `mode` (`ro`, `rw`, `rwc` or `memory`), and `_journal_mode` /
/// `_journal`. The path `:memory:` or `mode=memory` selects an in-memory database.
pub(crate) fn sqlite_settings(dbconfig: &str) -> Result<SqliteSettings, StoreError> {
    let (path, query) = dbconfig.split_once('?').unwrap_or((dbconfig, ""));
    let path = path.strip_prefix("file:").unwrap_or(path);
    if path.is_empty() {
        return Err(StoreError::Config(
            "dbconfig names no database file".to_owned(),
        ));
    }

    let mut in_memory = path == ":memory:";
    let mut read_only = false;
    let mut create = true;
    let mut busy_timeout = Duration::from_secs(5);
    let mut journal_mode = None;

    for (key, value) in query_pairs(query) {
        match key {
            "_busy_timeout" | "_timeout" => {
                let millis: u64 = value.parse().map_err(|_| {
                    StoreError::Config(format!(
                        "dbconfig parameter {key}={value:?} is not a number"
                    ))
                })?;
                busy_timeout = Duration::from_millis(millis);
            }
            // Every connection of one store shares one database whatever this says.
            "cache" if matches!(value, "shared" | "private") => {}
            "mode" => match value {
                "memory" => in_memory = true,
                "ro" => (read_only, create) = (true, false),
                "rw" => create = false,
                "rwc" => {}
                _ => return Err(unsupported_value(key, value)),
            },
            "_journal_mode" | "_journal" => {
                journal_mode = Some(
                    SqliteJournalMode::from_str(value)
                        .map_err(|_| unsupported_value(key, value))?,
                );
            }
            _ => return Err(unsupported_parameter(key)),
        }
    }

    let mut options = if in_memory {
        // A fresh, uniquely named in-memory database with a shared cache, so that every
        // connection of this store sees the same data.
        SqliteConnectOptions::from_str("sqlite::memory:")?
    } else {
        SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(create)
            .read_only(read_only)
    };
    // SQLite enforces foreign keys only where a connection asks for it. The schema
    // relies on them: said here, and not left to the driver's default.
    options = options.foreign_keys(true).busy_timeout(busy_timeout);
    if let Some(journal_mode) = journal_mode {
        options = options.journal_mode(journal_mode);
    }
    Ok(SqliteSettings { options, in_memory })
}

/// Connection settings for PostgreSQL derived from a `dbconfig` string.
#[derive(Debug, Clone)]
pub(crate) struct PostgresSettings {
    pub(crate) options: PgConnectOptions,
    /// How long to wait for a connection, from `connect_timeout`.
    pub(crate) connect_timeout: Option<Duration>,
}

/// Parses a PostgreSQL `dbconfig`: a `postgres://` URL or a `key=value` list.
///
/// Recognized parameters are `sslmode`, `sslrootcert`, `sslcert`, `sslkey`,
/// `application_name`, `search_path`, `connect_timeout` (seconds), and
/// `binary_parameters`, which is accepted and ignored. The `key=value` form also takes
/// `host`, `port`, `user`, `password` and `dbname`.
pub(crate) fn postgres_settings(dbconfig: &str) -> Result<PostgresSettings, StoreError> {
    let is_url = dbconfig.starts_with("postgres://") || dbconfig.starts_with("postgresql://");
    let (mut options, pairs) = if is_url {
        let (base, query) = dbconfig.split_once('?').unwrap_or((dbconfig, ""));
        let options = PgConnectOptions::from_str(base)
            .map_err(|error| StoreError::Config(format!("dbconfig is not a valid URL: {error}")))?;
        (options, query_pairs(query).collect::<Vec<_>>())
    } else {
        let pairs = dbconfig
            .split_whitespace()
            .map(|pair| {
                pair.split_once('=').ok_or_else(|| {
                    StoreError::Config(format!("dbconfig item {pair:?} is not key=value"))
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        (PgConnectOptions::new(), pairs)
    };

    let mut connect_timeout = None;
    for (key, value) in pairs {
        options = match key {
            "sslmode" => options
                .ssl_mode(PgSslMode::from_str(value).map_err(|_| unsupported_value(key, value))?),
            "sslrootcert" => options.ssl_root_cert(value),
            "sslcert" => options.ssl_client_cert(value),
            "sslkey" => options.ssl_client_key(value),
            "application_name" => options.application_name(value),
            "search_path" => options.options([("search_path", value)]),
            "connect_timeout" => {
                let seconds: u64 = value.parse().map_err(|_| {
                    StoreError::Config(format!(
                        "dbconfig parameter {key}={value:?} is not a number"
                    ))
                })?;
                connect_timeout = Some(Duration::from_secs(seconds));
                options
            }
            // A setting of the original driver about how it encodes parameters.
            "binary_parameters" => options,
            "host" if !is_url => options.host(value),
            "port" if !is_url => {
                options.port(value.parse().map_err(|_| unsupported_value(key, value))?)
            }
            "user" if !is_url => options.username(value),
            "password" if !is_url => options.password(value),
            "dbname" if !is_url => options.database(value),
            _ => return Err(unsupported_parameter(key)),
        };
    }
    Ok(PostgresSettings {
        options,
        connect_timeout,
    })
}

/// Splits a URL query string into its `key=value` pairs. A key without `=` has an empty
/// value.
fn query_pairs(query: &str) -> impl Iterator<Item = (&str, &str)> {
    query
        .split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| pair.split_once('=').unwrap_or((pair, "")))
}

fn unsupported_parameter(key: &str) -> StoreError {
    StoreError::Config(format!("dbconfig parameter {key:?} is not supported"))
}

fn unsupported_value(key: &str, value: &str) -> StoreError {
    StoreError::Config(format!(
        "dbconfig parameter {key}={value:?} is not supported"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config_error<T: std::fmt::Debug>(result: Result<T, StoreError>) -> String {
        match result {
            Err(StoreError::Config(message)) => message,
            other => panic!("expected a configuration error, got {other:?}"),
        }
    }

    #[test]
    fn db_type_names() {
        assert_eq!("sqlite3".parse::<DbType>().expect("known"), DbType::Sqlite);
        assert_eq!(
            "postgres".parse::<DbType>().expect("known"),
            DbType::Postgres
        );
        assert_eq!(DbType::Sqlite.as_str(), "sqlite3");
        assert_eq!(DbType::Postgres.as_str(), "postgres");
        assert!(config_error("mysql".parse::<DbType>()).contains("not supported"));
        assert!(config_error("sqlite".parse::<DbType>()).contains("unknown dbtype"));
    }

    /// The `dbconfig` values found in the original's configuration files, documentation
    /// and tests.
    #[test]
    fn sqlite_dbconfig_forms_of_the_original() {
        let plain = sqlite_settings("./data/boards.db").expect("a plain path");
        assert!(!plain.in_memory, "a path is a file database");
        assert_eq!(
            plain.options.get_filename().to_str(),
            Some("./data/boards.db")
        );

        let with_timeout = sqlite_settings("./boards.db?_busy_timeout=5000").expect("a timeout");
        assert_eq!(
            with_timeout.options.get_filename().to_str(),
            Some("./boards.db")
        );

        let memory =
            sqlite_settings("file::memory:?cache=shared&_busy_timeout=5000").expect("in memory");
        assert!(memory.in_memory, ":memory: is an in-memory database");

        let by_mode = sqlite_settings("file:test.db?mode=memory&cache=shared").expect("mode");
        assert!(by_mode.in_memory, "mode=memory is an in-memory database");
    }

    #[test]
    fn sqlite_dbconfig_rejects_what_it_does_not_understand() {
        assert!(config_error(sqlite_settings("")).contains("no database file"));
        assert!(config_error(sqlite_settings("a.db?_loc=UTC")).contains("\"_loc\""));
        assert!(config_error(sqlite_settings("a.db?_busy_timeout=soon")).contains("not a number"));
        assert!(config_error(sqlite_settings("a.db?mode=fast")).contains("mode=\"fast\""));
        assert!(config_error(sqlite_settings("a.db?cache=big")).contains("\"cache\""));
    }

    #[test]
    fn postgres_dbconfig_forms_of_the_original() {
        let url = postgres_settings(
            "postgres://boardsuser:secret@db.example/boards?sslmode=disable&connect_timeout=10",
        )
        .expect("the documented URL form");
        assert_eq!(url.options.get_host(), "db.example");
        assert_eq!(url.options.get_username(), "boardsuser");
        assert_eq!(url.options.get_database(), Some("boards"));
        assert_eq!(url.connect_timeout, Some(Duration::from_secs(10)));

        let pairs = postgres_settings("dbname=boards sslmode=disable host=h port=5433 user=u")
            .expect("the key=value form");
        assert_eq!(pairs.options.get_database(), Some("boards"));
        assert_eq!(pairs.options.get_host(), "h");
        assert_eq!(pairs.options.get_port(), 5433);
        assert_eq!(pairs.options.get_username(), "u");
        assert_eq!(pairs.connect_timeout, None);

        postgres_settings("postgres://u@h/db?binary_parameters=yes&search_path=s")
            .expect("binary_parameters is accepted and search_path is supported");
    }

    #[test]
    fn postgres_dbconfig_rejects_what_it_does_not_understand() {
        assert!(
            config_error(postgres_settings("postgres://u@h/db?fallback=1"))
                .contains("\"fallback\"")
        );
        assert!(
            config_error(postgres_settings("postgres://u@h/db?sslmode=maybe")).contains("sslmode")
        );
        assert!(config_error(postgres_settings("dbname")).contains("not key=value"));
        assert!(
            config_error(postgres_settings("postgres://u@h/db?host=other")).contains("\"host\"")
        );
        assert!(
            config_error(postgres_settings("postgres://u@h/db?connect_timeout=x"))
                .contains("not a number")
        );
    }
}
