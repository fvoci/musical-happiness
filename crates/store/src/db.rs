//! The execution layer: one SQL text and one set of parameters for both databases.
//!
//! Queries are written once with `$n` placeholders, which SQLite and PostgreSQL both
//! accept. [`Sql`] builds the text and collects the parameters, [`Scope`] runs it on one
//! connection, and [`Row`] reads the result. The few places where the two databases
//! differ go through [`Dialect`].

use std::fmt::Write as _;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use fvoci_model::is_id;
use serde_json::Value;
use sqlx::postgres::{PgConnection, PgRow};
use sqlx::sqlite::{SqliteConnection, SqliteRow};
use sqlx::types::Uuid;
use sqlx::{AssertSqlSafe, Row as _};

use crate::Tenant;
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
    /// An identifier, bound to a `uuid` column (`TEXT` on SQLite). `None` binds `NULL`.
    Id(Option<Uuid>),
    /// JSON text for a `jsonb` column (`TEXT` on SQLite) that may be `NULL`. `None`
    /// binds `NULL`.
    OptJsonb(Option<String>),
    /// An integer that may be `NULL`.
    OptInt(Option<i64>),
    /// Text that may be `NULL`.
    OptText(Option<String>),
    /// Bytes, bound to a `bytea` column (`BLOB` on SQLite).
    Bytes(Vec<u8>),
}

/// An identifier as a query parameter.
///
/// The value is whatever a caller was given: text that is not an identifier (see
/// [`fvoci_model::is_id`]) names nothing. A statement that reads then finds no row
/// under it, and one that writes is refused with [`StoreError::InvalidId`].
#[derive(Debug, Clone, Copy)]
pub(crate) struct Id<'a>(pub(crate) &'a str);

/// A reference that may be absent, as a query parameter: the empty string binds `NULL`,
/// anything else is bound as [`Id`] binds it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct OptId<'a>(pub(crate) &'a str);

/// A parameter of a statement, and whether it carried text that is no identifier where
/// one was needed.
#[derive(Debug, Clone)]
pub(crate) struct Bound {
    param: Param,
    invalid_id: Option<String>,
}

impl<T: Into<Param>> From<T> for Bound {
    fn from(value: T) -> Self {
        Self {
            param: value.into(),
            invalid_id: None,
        }
    }
}

impl From<Id<'_>> for Bound {
    fn from(Id(text): Id<'_>) -> Self {
        if is_id(text)
            && let Ok(uuid) = Uuid::try_parse(text)
        {
            return Self {
                param: Param::Id(Some(uuid)),
                invalid_id: None,
            };
        }
        // No row has the nil identifier: it is no identifier itself, so no write stores
        // it. A query bound to it matches nothing.
        Self {
            param: Param::Id(Some(Uuid::nil())),
            invalid_id: Some(text.to_owned()),
        }
    }
}

impl From<&Id<'_>> for Bound {
    fn from(id: &Id<'_>) -> Self {
        Self::from(*id)
    }
}

impl From<OptId<'_>> for Bound {
    fn from(OptId(text): OptId<'_>) -> Self {
        if text.is_empty() {
            Self {
                param: Param::Id(None),
                invalid_id: None,
            }
        } else {
            Self::from(Id(text))
        }
    }
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

    /// A JSON value for a `jsonb` column that may be `NULL`: `None` binds `NULL`.
    pub(crate) fn opt_jsonb<T: serde::Serialize>(value: Option<&T>) -> Result<Self, StoreError> {
        Ok(Self::OptJsonb(value.map(to_json_text).transpose()?))
    }

    /// A JSON value for a `jsonb` column.
    pub(crate) fn jsonb(value: &impl serde::Serialize) -> Result<Self, StoreError> {
        Ok(Self::Jsonb(to_json_text(value)?))
    }
}

pub(crate) fn to_json_text(value: &impl serde::Serialize) -> Result<String, StoreError> {
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
    /// The first bound value that had to be an identifier and was not.
    invalid_id: Option<String>,
}

impl Sql {
    /// Appends SQL text. The text must not contain values; bind those instead.
    pub(crate) fn push(&mut self, text: &str) -> &mut Self {
        self.text.push_str(text);
        self
    }

    /// Appends a placeholder and records its value.
    pub(crate) fn bind(&mut self, bound: impl Into<Bound>) -> &mut Self {
        let Bound { param, invalid_id } = bound.into();
        if self.invalid_id.is_none() {
            self.invalid_id = invalid_id;
        }
        let index = self.params.len() + 1;
        // PostgreSQL needs the type of a JSON parameter spelled out; SQLite stores JSON as
        // plain text and would mangle the value if asked to cast it.
        let cast = match (&param, self.dialect) {
            (Param::Json(_), Dialect::Postgres) => "::json",
            (Param::Jsonb(_) | Param::OptJsonb(_), Dialect::Postgres) => "::jsonb",
            _ => "",
        };
        // Writing to a String cannot fail.
        let _ = write!(self.text, "${index}{cast}");
        self.params.push(param);
        self
    }

    /// Appends `text` followed by a placeholder: `push("id = ").bind(id)` in one call.
    pub(crate) fn push_bind(&mut self, text: &str, param: impl Into<Bound>) -> &mut Self {
        self.push(text).bind(param)
    }

    /// Appends a comma-separated list of placeholders, one per item.
    ///
    /// An empty list produces `NULL`, so that `IN (...)` matches nothing.
    pub(crate) fn bind_list<I>(&mut self, items: I) -> &mut Self
    where
        I: IntoIterator,
        I::Item: Into<Bound>,
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
}

/// A borrowed connection to either database.
#[derive(Debug)]
pub(crate) enum ConnRef<'c> {
    Sqlite(&'c mut SqliteConnection),
    Postgres(&'c mut PgConnection),
}

/// One connection, or one transaction. Every query of an operation runs through the same
/// scope.
#[derive(Debug)]
pub(crate) struct Scope<'c> {
    conn: ConnRef<'c>,
    /// Whose rows the operation reaches. On PostgreSQL the transaction was told so when
    /// it began.
    tenant: Tenant,
    /// Whether a statement ran that may have changed something: anything but a plain
    /// `SELECT`. Set when the statement starts.
    wrote: bool,
    /// Told the same, as the statement starts, if the call is watched: so that a call
    /// given up while such a statement is under way is told as one that wrote
    /// ([`crate::observe::Watch`]).
    writes: Option<&'c AtomicBool>,
    visibility: Option<&'c crate::visibility::Writing<'c>>,
    /// How many versions are kept of a comment and of a card's body.
    versions_kept: VersionsKept,
    /// Whether the store notes changes for the search server
    /// ([`crate::Store::keep_search_outbox`]); `None` for a scope that notes none.
    search_outbox: Option<&'c AtomicBool>,
}

/// How many versions the store keeps, as it is set when a scope begins. Two counts in
/// the room of one `usize`: every call of the store carries a scope.
#[derive(Debug, Clone, Copy)]
pub(crate) struct VersionsKept {
    /// Earlier versions of a comment ([`crate::Store::set_comment_versions_kept`]).
    pub(crate) of_comments: u32,
    /// Versions of a card's body ([`crate::Store::set_body_versions_kept`]).
    pub(crate) of_bodies: u32,
}

/// Binds every parameter of `$sql` to `$query` for one database. `$id` turns an
/// identifier into what that database's identifier columns hold.
macro_rules! bind_params {
    ($query:expr, $params:expr, $id:expr) => {{
        let mut query = $query;
        for param in $params {
            query = match param {
                Param::Text(value) | Param::Json(value) | Param::Jsonb(value) => query.bind(value),
                Param::Int(value) => query.bind(value),
                Param::Bool(value) => query.bind(value),
                Param::Id(value) => query.bind(value.map($id)),
                Param::OptJsonb(value) => query.bind(value),
                Param::OptInt(value) => query.bind(value),
                Param::OptText(value) => query.bind(value),
                Param::Bytes(value) => query.bind(value),
            };
        }
        query
    }};
}

/// SQLite keeps identifiers as their canonical text.
fn sqlite_id(uuid: Uuid) -> String {
    uuid.hyphenated().to_string()
}

/// PostgreSQL has a type for them.
fn postgres_id(uuid: Uuid) -> Uuid {
    uuid
}

impl<'c> Scope<'c> {
    pub(crate) fn new(conn: ConnRef<'c>, tenant: Tenant, versions_kept: VersionsKept) -> Self {
        Self {
            conn,
            tenant,
            wrote: false,
            writes: None,
            visibility: None,
            versions_kept,
            search_outbox: None,
        }
    }

    /// The same scope, which notes changes for the search server while `kept` says so.
    pub(crate) fn noting_for_search(self, kept: &'c AtomicBool) -> Self {
        Self {
            search_outbox: Some(kept),
            ..self
        }
    }

    /// Whether the changes of this scope are noted for the search server.
    pub(crate) fn keeps_search_outbox(&self) -> bool {
        self.search_outbox
            .is_some_and(|kept| kept.load(Ordering::Relaxed))
    }

    /// The same scope, which also sets `writes` when a statement that may write starts.
    pub(crate) fn reporting_writes_to(self, writes: Option<&'c AtomicBool>) -> Self {
        Self { writes, ..self }
    }

    pub(crate) fn tracking_visibility(
        self,
        visibility: &'c crate::visibility::Writing<'c>,
    ) -> Self {
        Self {
            visibility: Some(visibility),
            ..self
        }
    }

    /// Notes that a statement that may write starts.
    fn starts_writing(&mut self, text: &str) {
        if let Some(visibility) = self.visibility {
            visibility.statement(text);
        }
        self.wrote = true;
        if let Some(writes) = self.writes {
            writes.store(true, Ordering::Relaxed);
        }
    }

    /// A structural change affects visibility even though its SQL also writes ordinary fields.
    pub(crate) fn invalidate_visibility(&self) {
        if let Some(visibility) = self.visibility {
            visibility.invalidate();
        }
    }

    /// Whether the operation ran a statement that may have changed something. One that
    /// only read has nothing to commit.
    pub(crate) fn wrote(&self) -> bool {
        self.wrote
    }

    /// The organization whose rows the operation reaches, as a query parameter: for
    /// the statements that name the organization themselves.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NoOrganization`] if the store's handle names none.
    pub(crate) fn org(&self) -> Result<Param, StoreError> {
        if let Tenant::Org(org_id) = self.tenant {
            Ok(Param::Id(Some(org_id)))
        } else {
            Err(StoreError::NoOrganization)
        }
    }

    /// The identifier of the organization the operation reaches, if it names one.
    pub(crate) fn org_id(&self) -> Option<String> {
        self.tenant.org_id()
    }

    /// Whether the store's handle names an organization.
    pub(crate) fn has_org(&self) -> bool {
        matches!(self.tenant, Tenant::Org(_))
    }

    /// The conditions that hold each row of `pairs` to the organization of the row
    /// paired with it: `" AND a.org_id = b.org_id"` for each pair `(a, b)` of table
    /// names or aliases, where rows carry their organization (PostgreSQL), and nothing
    /// on SQLite. For the statements that reach across every organization and match rows
    /// by an identifier that the keys of the schema do not hold to one organization, or
    /// that do not take the keys on trust.
    pub(crate) fn same_organization(&self, pairs: &[(&str, &str)]) -> String {
        if self.dialect() == Dialect::Sqlite {
            return String::new();
        }
        pairs
            .iter()
            .map(|(row, parent)| [" AND ", row, ".org_id = ", parent, ".org_id"].concat())
            .collect()
    }

    /// How many earlier versions of a comment are kept, at least one.
    pub(crate) fn comment_versions_kept(&self) -> usize {
        usize::try_from(self.versions_kept.of_comments).unwrap_or(usize::MAX)
    }

    /// How many versions of a card's body are kept, at least one.
    pub(crate) fn body_versions_kept(&self) -> i64 {
        i64::from(self.versions_kept.of_bodies)
    }

    /// The dialect of the connected database.
    pub(crate) fn dialect(&self) -> Dialect {
        match self.conn {
            ConnRef::Sqlite(_) => Dialect::Sqlite,
            ConnRef::Postgres(_) => Dialect::Postgres,
        }
    }

    /// Starts a statement with the given text.
    pub(crate) fn sql(&self, text: &str) -> Sql {
        Sql {
            dialect: self.dialect(),
            text: text.to_owned(),
            params: Vec::new(),
            invalid_id: None,
        }
    }

    /// Runs a query and returns all its rows.
    ///
    /// A value bound as an identifier that is not one matches no row.
    pub(crate) async fn fetch_all(&mut self, sql: Sql) -> Result<Vec<Row>, StoreError> {
        if !is_plain_select(&sql.text) {
            self.starts_writing(&sql.text);
        }
        // The text is built from literals in this crate; every value is a bound
        // parameter.
        let text = AssertSqlSafe(sql.text);
        match &mut self.conn {
            ConnRef::Sqlite(conn) => {
                let rows = bind_params!(sqlx::query(text), sql.params, sqlite_id)
                    .fetch_all(&mut **conn)
                    .await?;
                Ok(rows.into_iter().map(Row::Sqlite).collect())
            }
            ConnRef::Postgres(conn) => {
                let rows = bind_params!(sqlx::query(text), sql.params, postgres_id)
                    .fetch_all(&mut **conn)
                    .await?;
                Ok(rows.into_iter().map(Row::Postgres).collect())
            }
        }
    }

    /// Runs a plain `SELECT` and returns all its rows, unless it runs past `by`: then
    /// the database is told to stop it, and `None` is returned. On PostgreSQL the
    /// statement runs under a `statement_timeout` of what is left of the time, in the
    /// scope's transaction; the transaction cannot be used after a timeout. On SQLite a
    /// progress handler stops it, and is taken off the connection again before it goes
    /// back to the pool (or left harmless, should the call be given up).
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the query fails otherwise.
    pub(crate) async fn fetch_all_by(
        &mut self,
        sql: Sql,
        by: Instant,
    ) -> Result<Option<Vec<Row>>, StoreError> {
        debug_assert!(is_plain_select(&sql.text), "{}", sql.text);
        let Some(left) = by
            .checked_duration_since(Instant::now())
            .filter(|left| !left.is_zero())
        else {
            return Ok(None);
        };
        match self.dialect() {
            Dialect::Postgres => {
                // 0 would be no limit: a millisecond at least.
                let millis = left.as_millis().max(1);
                let setting = self.sql(&format!(
                    "SELECT set_config('statement_timeout', '{millis}ms', true)"
                ));
                self.fetch_all(setting).await?;
                match self.fetch_all(sql).await {
                    Err(StoreError::Database(error)) if is_query_canceled(&error) => Ok(None),
                    rows => rows.map(Some),
                }
            }
            Dialect::Sqlite => {
                let ConnRef::Sqlite(conn) = &mut self.conn else {
                    unreachable!("a SQLite scope holds a SQLite connection");
                };
                let stop = SqliteStop::set(conn, by).await?;
                let rows = self.fetch_all(sql).await;
                let ConnRef::Sqlite(conn) = &mut self.conn else {
                    unreachable!("a SQLite scope holds a SQLite connection");
                };
                let stopped = stop.take_off(conn).await?;
                match rows {
                    Err(StoreError::Database(_)) if stopped => Ok(None),
                    rows => rows.map(Some),
                }
            }
        }
    }

    /// Runs a query and returns its first row, if any.
    pub(crate) async fn fetch_optional(&mut self, sql: Sql) -> Result<Option<Row>, StoreError> {
        Ok(self.fetch_all(sql).await?.into_iter().next())
    }

    /// Runs a statement that writes and returns the number of rows it affected.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::InvalidId`], without running the statement, if a value
    /// bound as an identifier is not one.
    pub(crate) async fn execute(&mut self, sql: Sql) -> Result<u64, StoreError> {
        if let Some(invalid) = sql.invalid_id {
            return Err(StoreError::InvalidId(invalid));
        }
        self.starts_writing(&sql.text);
        let text = AssertSqlSafe(sql.text);
        match &mut self.conn {
            ConnRef::Sqlite(conn) => Ok(bind_params!(sqlx::query(text), sql.params, sqlite_id)
                .execute(&mut **conn)
                .await?
                .rows_affected()),
            ConnRef::Postgres(conn) => Ok(bind_params!(sqlx::query(text), sql.params, postgres_id)
                .execute(&mut **conn)
                .await?
                .rows_affected()),
        }
    }
}

/// Whether PostgreSQL stopped the statement: its `statement_timeout` ran out
/// (`query_canceled`).
fn is_query_canceled(error: &sqlx::Error) -> bool {
    error
        .as_database_error()
        .and_then(sqlx::error::DatabaseError::code)
        .is_some_and(|code| code == "57014")
}

/// How many virtual machine instructions of SQLite run between two looks at the clock.
const SQLITE_STEPS_A_LOOK: i32 = 1000;

/// The progress handler that stops a statement of SQLite at a time
/// ([`Scope::fetch_all_by`]). Dropped without [`SqliteStop::take_off`] (the call was
/// given up), it disarms the handler it left on the connection: that handler then lets
/// every statement run, until another takes its place.
struct SqliteStop {
    armed: Arc<AtomicBool>,
    stopped: Arc<AtomicBool>,
}

impl SqliteStop {
    async fn set(conn: &mut SqliteConnection, by: Instant) -> Result<Self, StoreError> {
        let armed = Arc::new(AtomicBool::new(true));
        let stopped = Arc::new(AtomicBool::new(false));
        let handler = {
            let armed = Arc::clone(&armed);
            let stopped = Arc::clone(&stopped);
            move || {
                if !armed.load(Ordering::Relaxed) || Instant::now() < by {
                    return true;
                }
                stopped.store(true, Ordering::Relaxed);
                false
            }
        };
        conn.lock_handle()
            .await?
            .set_progress_handler(SQLITE_STEPS_A_LOOK, handler);
        Ok(Self { armed, stopped })
    }

    /// Takes the handler off the connection. Returns whether it stopped the statement.
    async fn take_off(self, conn: &mut SqliteConnection) -> Result<bool, StoreError> {
        conn.lock_handle().await?.remove_progress_handler();
        Ok(self.stopped.load(Ordering::Relaxed))
    }
}

impl Drop for SqliteStop {
    fn drop(&mut self) {
        self.armed.store(false, Ordering::Relaxed);
    }
}

/// Whether a statement is a `SELECT` and nothing else, as this crate writes them: one
/// that changes nothing. A statement that begins otherwise (`WITH ... DELETE`, an
/// `UPDATE ... RETURNING`) counts as one that may write, whatever it does.
fn is_plain_select(text: &str) -> bool {
    text.trim_start().starts_with("SELECT ")
}

/// A result row from either database, read by column position.
///
/// The getters treat SQL `NULL` as the empty value of their type. The columns that may
/// be `NULL` are references that may be absent (a block's parent, a creator), and the
/// model spells "none" as the empty string.
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

    /// Reads a text column that may be `NULL`, as `None`: for where no value and an
    /// empty one are told apart.
    pub(crate) fn opt_text(&self, index: usize) -> Result<Option<String>, StoreError> {
        Ok(match self {
            Self::Sqlite(row) => row.try_get(index)?,
            Self::Postgres(row) => row.try_get(index)?,
        })
    }

    /// Reads a 64-bit integer column that may be `NULL`, as `None`.
    pub(crate) fn opt_int(&self, index: usize) -> Result<Option<i64>, StoreError> {
        Ok(match self {
            Self::Sqlite(row) => row.try_get(index)?,
            Self::Postgres(row) => row.try_get(index)?,
        })
    }

    /// Reads an identifier column: `uuid` on PostgreSQL, text on SQLite. `NULL` reads
    /// as the empty string, which is how the model says "none".
    pub(crate) fn id(&self, index: usize) -> Result<String, StoreError> {
        match self {
            Self::Sqlite(row) => {
                let value: Option<String> = row.try_get(index)?;
                Ok(value.unwrap_or_default())
            }
            Self::Postgres(row) => {
                let value: Option<Uuid> = row.try_get(index)?;
                Ok(value.map(sqlite_id).unwrap_or_default())
            }
        }
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

    /// Reads a column of bytes: `bytea` on PostgreSQL, `BLOB` on SQLite. `NULL` reads
    /// as no bytes.
    pub(crate) fn bytes(&self, index: usize) -> Result<Vec<u8>, StoreError> {
        let value: Option<Vec<u8>> = match self {
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
                // JSON as text, and SQLite keeps what is bound as bytes as a BLOB even
                // in a text column.
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

#[cfg(test)]
mod tests {
    use super::is_plain_select;

    #[test]
    fn only_a_statement_that_begins_with_select_is_taken_for_a_read() {
        assert!(
            is_plain_select("SELECT id FROM boards WHERE id = $1"),
            "a read"
        );
        assert!(is_plain_select("  SELECT 1"), "leading white space");
        for statement in [
            "WITH gone AS (DELETE FROM blocks RETURNING id) SELECT id FROM gone",
            "DELETE FROM data_steps WHERE name = $1 RETURNING name",
            "UPDATE users SET update_at = $1 RETURNING id",
            "INSERT INTO boards (id) VALUES ($1) RETURNING id",
            "select 1",
            "SELECTED",
        ] {
            assert!(!is_plain_select(statement), "{statement}");
        }
    }

    #[tokio::test]
    async fn a_statement_that_runs_past_its_time_is_stopped_and_the_connection_serves_on() {
        use std::time::{Duration, Instant};

        use super::Dialect;
        use crate::Store;
        use crate::testing::TestDatabase;

        let directory =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/deadline-tests");
        let database = TestDatabase::create(&directory).await;
        let store = Store::connect(database.db_type(), database.dbconfig())
            .await
            .expect("test store");
        let counting = |to: u64| {
            format!(
                "SELECT count(*) FROM (WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL \
                 SELECT x + 1 FROM n WHERE x < {to}) SELECT x FROM n) AS counted"
            )
        };
        let endless = match store.dialect() {
            Dialect::Sqlite => counting(u64::MAX / 2),
            Dialect::Postgres => "SELECT pg_sleep(60)".to_owned(),
        };
        let run = async |text: &str, by: Instant| {
            store
                .with_conn(async |scope| {
                    let sql = scope.sql(text);
                    scope.fetch_all_by(sql, by).await
                })
                .await
        };

        let started = Instant::now();
        let stopped = run(&endless, started + Duration::from_millis(300)).await;
        let took = started.elapsed();
        let late = run(&counting(10), started).await;
        // Long past the time of the statement that was stopped: its handler is gone.
        let mut after = Vec::new();
        for _ in 0..4 {
            after.push(run(&counting(300_000), Instant::now() + Duration::from_secs(30)).await);
        }
        let plain = store
            .with_conn(async |scope| {
                let sql = scope.sql(&counting(300_000));
                scope.fetch_all(sql).await
            })
            .await;
        store.close().await;
        database.remove().await;

        assert!(matches!(stopped, Ok(None)), "{stopped:?}");
        assert!(took < Duration::from_secs(5), "stopped in time: {took:?}");
        assert!(
            matches!(late, Ok(None)),
            "nothing asked past the time: {late:?}"
        );
        for rows in after {
            assert!(
                matches!(rows, Ok(Some(ref rows)) if rows.len() == 1),
                "{rows:?}"
            );
        }
        assert!(plain.is_ok(), "{plain:?}");
    }
}
