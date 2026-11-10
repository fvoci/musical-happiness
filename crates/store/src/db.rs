//! The execution layer: one SQL text and one set of parameters for both databases.
//!
//! Queries are written once with `$n` placeholders, which SQLite and PostgreSQL both
//! accept. [`Sql`] builds the text and collects the parameters, [`Scope`] runs it on one
//! connection, and [`Row`] reads the result. The few places where the two databases
//! differ go through [`Dialect`].

use std::fmt::Write as _;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;
use sqlx::postgres::{PgConnection, PgRow};
use sqlx::sqlite::{SqliteConnection, SqliteRow};
use sqlx::{AssertSqlSafe, Row as _};

use crate::error::StoreError;

/// The SQL dialect of the connected database.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
    /// SQLite.
    Sqlite,
    /// PostgreSQL.
    Postgres,
}

/// Returns the current time in milliseconds since the Unix epoch.
pub(crate) fn now_millis() -> i64 {
    let since_epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    i64::try_from(since_epoch.as_millis()).unwrap_or(i64::MAX)
}

/// State shared by every operation of one store.
#[derive(Debug)]
pub(crate) struct Shared {
    /// The last millisecond handed out by [`Shared::next_history_millis`].
    last_history_millis: AtomicI64,
}

impl Shared {
    pub(crate) fn new() -> Self {
        Self {
            last_history_millis: AtomicI64::new(0),
        }
    }

    /// Returns a millisecond timestamp that is later than every one returned before.
    ///
    /// History tables are keyed by `(id, insert_at)` and SQLite's clock has millisecond
    /// resolution, so two history rows for one record written within a millisecond would
    /// collide. Handing out strictly increasing values rules that out within a process.
    fn next_history_millis(&self) -> i64 {
        let now = now_millis();
        let mut last = self.last_history_millis.load(Ordering::Relaxed);
        loop {
            let next = now.max(last + 1);
            match self.last_history_millis.compare_exchange_weak(
                last,
                next,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => return next,
                Err(current) => last = current,
            }
        }
    }
}

/// A query parameter.
#[derive(Debug, Clone)]
pub(crate) enum Param {
    Text(String),
    Int(i64),
    Bool(bool),
    /// JSON text bound to a `json` column (`TEXT` on SQLite).
    Json(String),
    /// JSON text bound to a `jsonb` column (`TEXT` on SQLite).
    Jsonb(String),
}

impl From<&str> for Param {
    fn from(value: &str) -> Self {
        Self::Text(value.to_owned())
    }
}

impl From<&String> for Param {
    fn from(value: &String) -> Self {
        Self::Text(value.clone())
    }
}

impl From<String> for Param {
    fn from(value: String) -> Self {
        Self::Text(value)
    }
}

impl From<i64> for Param {
    fn from(value: i64) -> Self {
        Self::Int(value)
    }
}

impl From<bool> for Param {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}

impl Param {
    /// A JSON value for a `json` column.
    pub(crate) fn json(value: &impl serde::Serialize) -> Result<Self, StoreError> {
        Ok(Self::Json(to_json_text(value)?))
    }

    /// A JSON value for a `jsonb` column.
    pub(crate) fn jsonb(value: &impl serde::Serialize) -> Result<Self, StoreError> {
        Ok(Self::Jsonb(to_json_text(value)?))
    }
}

fn to_json_text(value: &impl serde::Serialize) -> Result<String, StoreError> {
    serde_json::to_string(value).map_err(|error| StoreError::Corrupt {
        column: "(parameter)",
        reason: error.to_string(),
    })
}

/// A SQL statement under construction: text with `$n` placeholders and their values.
#[derive(Debug, Clone)]
pub(crate) struct Sql {
    dialect: Dialect,
    text: String,
    params: Vec<Param>,
}

impl Sql {
    /// Appends SQL text. The text must not contain values; bind those instead.
    pub(crate) fn push(&mut self, text: &str) -> &mut Self {
        self.text.push_str(text);
        self
    }

    /// Appends a placeholder and records its value.
    pub(crate) fn bind(&mut self, param: impl Into<Param>) -> &mut Self {
        let param = param.into();
        let index = self.params.len() + 1;
        // PostgreSQL needs the type of a JSON parameter spelled out; SQLite stores JSON as
        // plain text and would mangle the value if asked to cast it.
        let cast = match (&param, self.dialect) {
            (Param::Json(_), Dialect::Postgres) => "::json",
            (Param::Jsonb(_), Dialect::Postgres) => "::jsonb",
            _ => "",
        };
        // Writing to a String cannot fail.
        let _ = write!(self.text, "${index}{cast}");
        self.params.push(param);
        self
    }

    /// Appends `text` followed by a placeholder: `push("id = ").bind(id)` in one call.
    pub(crate) fn push_bind(&mut self, text: &str, param: impl Into<Param>) -> &mut Self {
        self.push(text).bind(param)
    }

    /// Appends a comma-separated list of placeholders, one per item.
    ///
    /// An empty list produces `NULL`, so that `IN (...)` matches nothing.
    pub(crate) fn bind_list<I>(&mut self, items: I) -> &mut Self
    where
        I: IntoIterator,
        I::Item: Into<Param>,
    {
        let mut first = true;
        for item in items {
            if !first {
                self.text.push_str(", ");
            }
            first = false;
            self.bind(item);
        }
        if first {
            self.text.push_str("NULL");
        }
        self
    }

    /// Appends the expression for the `insert_at` of a new history row.
    pub(crate) fn bind_history_stamp(&mut self, stamp: HistoryStamp) -> &mut Self {
        match stamp {
            HistoryStamp::Millis(millis) => self
                .push("STRFTIME('%Y-%m-%d %H:%M:%f', ")
                .bind(millis)
                .push(" / 1000.0, 'unixepoch')"),
            HistoryStamp::DatabaseClock => self.push("clock_timestamp()"),
        }
    }
}

/// How the `insert_at` of a history row is produced; see [`Scope::history_stamp`].
#[derive(Debug, Clone, Copy)]
pub(crate) enum HistoryStamp {
    /// SQLite: a process-wide strictly increasing millisecond, formatted like the
    /// column's default.
    Millis(i64),
    /// PostgreSQL: the wall clock at statement time, which has microsecond resolution and
    /// unlike `now()` advances within a transaction.
    DatabaseClock,
}

/// A borrowed connection to either database.
#[derive(Debug)]
pub(crate) enum ConnRef<'c> {
    Sqlite(&'c mut SqliteConnection),
    Postgres(&'c mut PgConnection),
}

/// One connection, or one transaction, together with the store-wide context that queries
/// need. Every query of an operation runs through the same scope.
#[derive(Debug)]
pub(crate) struct Scope<'c> {
    conn: ConnRef<'c>,
    shared: &'c Shared,
}

/// Binds every parameter of `$sql` to `$query` for one database.
macro_rules! bind_params {
    ($query:expr, $params:expr) => {{
        let mut query = $query;
        for param in $params {
            query = match param {
                Param::Text(value) | Param::Json(value) | Param::Jsonb(value) => query.bind(value),
                Param::Int(value) => query.bind(value),
                Param::Bool(value) => query.bind(value),
            };
        }
        query
    }};
}

impl<'c> Scope<'c> {
    pub(crate) fn new(conn: ConnRef<'c>, shared: &'c Shared) -> Self {
        Self { conn, shared }
    }

    /// The dialect of the connected database.
    pub(crate) fn dialect(&self) -> Dialect {
        match self.conn {
            ConnRef::Sqlite(_) => Dialect::Sqlite,
            ConnRef::Postgres(_) => Dialect::Postgres,
        }
    }

    /// The name of a table as it goes into SQL text.
    #[expect(
        clippy::unused_self,
        reason = "the statements that call it are rewritten with the identifier columns"
    )]
    pub(crate) fn table(&self, name: &str) -> String {
        name.to_owned()
    }

    /// Starts a statement with the given text.
    pub(crate) fn sql(&self, text: &str) -> Sql {
        Sql {
            dialect: self.dialect(),
            text: text.to_owned(),
            params: Vec::new(),
        }
    }

    /// Returns the `insert_at` source for one new history row.
    pub(crate) fn history_stamp(&self) -> HistoryStamp {
        match self.dialect() {
            Dialect::Sqlite => HistoryStamp::Millis(self.shared.next_history_millis()),
            Dialect::Postgres => HistoryStamp::DatabaseClock,
        }
    }

    /// Runs a query and returns all its rows.
    pub(crate) async fn fetch_all(&mut self, sql: Sql) -> Result<Vec<Row>, StoreError> {
        // The text is built from literals in this crate; every value is a bound
        // parameter.
        let text = AssertSqlSafe(sql.text);
        match &mut self.conn {
            ConnRef::Sqlite(conn) => {
                let rows = bind_params!(sqlx::query(text), sql.params)
                    .fetch_all(&mut **conn)
                    .await?;
                Ok(rows.into_iter().map(Row::Sqlite).collect())
            }
            ConnRef::Postgres(conn) => {
                let rows = bind_params!(sqlx::query(text), sql.params)
                    .fetch_all(&mut **conn)
                    .await?;
                Ok(rows.into_iter().map(Row::Postgres).collect())
            }
        }
    }

    /// Runs a query and returns its first row, if any.
    pub(crate) async fn fetch_optional(&mut self, sql: Sql) -> Result<Option<Row>, StoreError> {
        Ok(self.fetch_all(sql).await?.into_iter().next())
    }

    /// Runs a statement and returns the number of rows it affected.
    pub(crate) async fn execute(&mut self, sql: Sql) -> Result<u64, StoreError> {
        let text = AssertSqlSafe(sql.text);
        match &mut self.conn {
            ConnRef::Sqlite(conn) => Ok(bind_params!(sqlx::query(text), sql.params)
                .execute(&mut **conn)
                .await?
                .rows_affected()),
            ConnRef::Postgres(conn) => Ok(bind_params!(sqlx::query(text), sql.params)
                .execute(&mut **conn)
                .await?
                .rows_affected()),
        }
    }
}

/// A result row from either database, read by column position.
///
/// The getters treat SQL `NULL` as the empty value of their type. The original schema
/// leaves most columns nullable and its own code never writes `NULL` into them, so this
/// only matters for rows written by something else.
#[derive(Debug)]
pub(crate) enum Row {
    Sqlite(SqliteRow),
    Postgres(PgRow),
}

impl Row {
    /// Reads a text column.
    pub(crate) fn text(&self, index: usize) -> Result<String, StoreError> {
        let value: Option<String> = match self {
            Self::Sqlite(row) => row.try_get(index)?,
            Self::Postgres(row) => row.try_get(index)?,
        };
        Ok(value.unwrap_or_default())
    }

    /// Reads a 64-bit integer column. Narrower PostgreSQL integer columns must be cast to
    /// `BIGINT` in the query.
    pub(crate) fn int(&self, index: usize) -> Result<i64, StoreError> {
        let value: Option<i64> = match self {
            Self::Sqlite(row) => row.try_get(index)?,
            Self::Postgres(row) => row.try_get(index)?,
        };
        Ok(value.unwrap_or_default())
    }

    /// Reads a boolean column.
    pub(crate) fn bool(&self, index: usize) -> Result<bool, StoreError> {
        let value: Option<bool> = match self {
            Self::Sqlite(row) => row.try_get(index)?,
            Self::Postgres(row) => row.try_get(index)?,
        };
        Ok(value.unwrap_or_default())
    }

    /// Reads a JSON column: `json` or `jsonb` on PostgreSQL, JSON as text or as a blob on
    /// SQLite.
    ///
    /// `NULL` and empty text read as JSON `null`.
    pub(crate) fn json(&self, index: usize, column: &'static str) -> Result<Value, StoreError> {
        match self {
            Self::Sqlite(row) => {
                // Read as bytes, which accepts both storage classes: this server writes
                // JSON as text, while the original binds it as bytes and SQLite keeps
                // those as a BLOB even in a text column.
                let bytes: Option<Vec<u8>> = row.try_get(index)?;
                match bytes.as_deref() {
                    None | Some([]) => Ok(Value::Null),
                    Some(bytes) => {
                        serde_json::from_slice(bytes).map_err(|error| StoreError::Corrupt {
                            column,
                            reason: error.to_string(),
                        })
                    }
                }
            }
            Self::Postgres(row) => {
                let value: Option<Value> = row.try_get(index)?;
                Ok(value.unwrap_or(Value::Null))
            }
        }
    }

    /// Reads a JSON column that holds an object. Anything else, including `null`, reads
    /// as an empty object.
    pub(crate) fn json_map(
        &self,
        index: usize,
        column: &'static str,
    ) -> Result<fvoci_model::JsonMap, StoreError> {
        match self.json(index, column)? {
            Value::Object(map) => Ok(map),
            _ => Ok(fvoci_model::JsonMap::new()),
        }
    }
}
