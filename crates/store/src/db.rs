//! The execution layer: one SQL text and one set of parameters for both databases.
//!
//! Queries are written once with `$n` placeholders, which SQLite and PostgreSQL both
//! accept. [`Sql`] builds the text and collects the parameters, [`Scope`] runs it on one
//! connection, and [`Row`] reads the result. The few places where the two databases
//! differ go through [`Dialect`].

use std::fmt::Write as _;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

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
    /// How many earlier versions of a comment are kept
    /// ([`crate::Store::set_comment_versions_kept`]).
    comment_versions_kept: usize,
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
    pub(crate) fn new(conn: ConnRef<'c>, tenant: Tenant, comment_versions_kept: usize) -> Self {
        Self {
            conn,
            tenant,
            wrote: false,
            writes: None,
            comment_versions_kept,
        }
    }

    /// The same scope, which also sets `writes` when a statement that may write starts.
    pub(crate) fn reporting_writes_to(self, writes: Option<&'c AtomicBool>) -> Self {
        Self { writes, ..self }
    }

    /// Notes that a statement that may write starts.
    fn starts_writing(&mut self) {
        self.wrote = true;
        if let Some(writes) = self.writes {
            writes.store(true, Ordering::Relaxed);
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
        self.comment_versions_kept
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
            self.starts_writing();
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
        self.starts_writing();
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
}
