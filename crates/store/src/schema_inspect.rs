//! Database catalog and privilege inspection helpers for [`super`].

use crate::Store;
use crate::db::{Dialect, Scope};
use crate::error::StoreError;

/// The table in which sqlx's migrator records what it applied.
pub(super) const MIGRATIONS_TABLE: &str = "_sqlx_migrations";

/// Whether the record of the migrations is in the schema the role works in. On
/// PostgreSQL it is found whatever the role was granted, and a record the role may not
/// read is an error that says what to grant.
pub(super) async fn has_record(scope: &mut Scope<'_>) -> Result<bool, StoreError> {
    if scope.dialect() == Dialect::Sqlite {
        return Ok(table_names(scope)
            .await?
            .iter()
            .any(|table| table == MIGRATIONS_TABLE));
    }
    let sql = scope.sql(
        "SELECT record IS NOT NULL, COALESCE(has_table_privilege(record, 'SELECT'), false), \
           current_user::text, COALESCE(current_schema()::text, '') \
         FROM (SELECT to_regclass(quote_ident(current_schema()) || '._sqlx_migrations') \
           AS record) AS found",
    );
    let Some(row) = scope.fetch_optional(sql).await? else {
        return Ok(false);
    };
    let (found, readable) = (row.bool(0)?, row.bool(1)?);
    if found && !readable {
        return Err(StoreError::Schema(format!(
            "the role {} may not read the tables of the schema {}: grant it SELECT, INSERT, \
             UPDATE and DELETE on them, and USAGE and SELECT on the sequences, also by default \
             for the tables the role that migrates makes later",
            row.text(2)?,
            row.text(3)?
        )));
    }
    Ok(found)
}

/// Whether the role the store connects as may change the schema: on PostgreSQL, the
/// role that owns the record of the migrations (and so the tables the migrations made),
/// a superuser, or, on an empty schema, a role that may make tables in it. A member of
/// the owner's role is not taken for it: what it made would be its own. SQLite has no
/// roles.
///
/// # Errors
///
/// Returns [`StoreError::Database`] if the catalog cannot be read.
pub(crate) async fn may_change(store: &Store) -> Result<bool, StoreError> {
    if store.dialect() == Dialect::Sqlite {
        return Ok(true);
    }
    store
        .with_conn(async |scope| {
            let sql = scope.sql(
                "SELECT COALESCE(( \
                   SELECT CASE WHEN record IS NULL \
                     THEN has_schema_privilege(current_schema(), 'CREATE') \
                     ELSE (SELECT rolsuper FROM pg_roles WHERE rolname = current_user) \
                       OR (SELECT relowner FROM pg_class WHERE oid = record) \
                         = (SELECT oid FROM pg_roles WHERE rolname = current_user) END \
                   FROM (SELECT to_regclass(quote_ident(current_schema()) \
                     || '._sqlx_migrations') AS record) AS found), false)",
            );
            match scope.fetch_optional(sql).await? {
                Some(row) => row.bool(0),
                None => Ok(false),
            }
        })
        .await
}

/// The role and the schema the store works as and in, for what it says: on SQLite,
/// none.
pub(crate) async fn whereabouts(store: &Store) -> Result<(String, String), StoreError> {
    if store.dialect() == Dialect::Sqlite {
        return Ok((String::new(), String::new()));
    }
    store
        .with_conn(async |scope| {
            let sql =
                scope.sql("SELECT current_user::text, COALESCE(current_schema()::text, '(none)')");
            match scope.fetch_optional(sql).await? {
                Some(row) => Ok((row.text(0)?, row.text(1)?)),
                None => Ok((String::new(), String::new())),
            }
        })
        .await
}

/// The tables of the schema the role may not read and write all of (of the record of
/// the migrations, read): a role that does not own them, on a schema that is up to
/// date, would otherwise start and fail its requests (the default privileges cover only
/// the tables made after them).
///
/// # Errors
///
/// Returns [`StoreError::Database`] if the catalog cannot be read.
pub(crate) async fn ungranted_tables(store: &Store) -> Result<Vec<String>, StoreError> {
    if store.dialect() == Dialect::Sqlite {
        return Ok(Vec::new());
    }
    store
        .with_conn(async |scope| {
            let sql = scope.sql(
                "SELECT c.relname::text FROM pg_class c \
                 WHERE c.relnamespace = (SELECT oid FROM pg_namespace \
                   WHERE nspname = current_schema()) \
                 AND c.relkind IN ('r', 'p') \
                 AND NOT (has_table_privilege(c.oid, 'SELECT') \
                   AND (c.relname = '_sqlx_migrations' \
                     OR (has_table_privilege(c.oid, 'INSERT') \
                       AND has_table_privilege(c.oid, 'UPDATE') \
                       AND has_table_privilege(c.oid, 'DELETE')))) \
                 ORDER BY c.relname",
            );
            scope
                .fetch_all(sql)
                .await?
                .iter()
                .map(|row| row.text(0))
                .collect()
        })
        .await
}

/// The names of the tables in the database (on PostgreSQL: in the current schema).
pub(super) async fn table_names(scope: &mut Scope<'_>) -> Result<Vec<String>, StoreError> {
    let sql = match scope.dialect() {
        Dialect::Sqlite => scope.sql("SELECT name FROM sqlite_master WHERE type = 'table'"),
        Dialect::Postgres => scope.sql(
            "SELECT table_name::text FROM information_schema.tables \
             WHERE table_schema = current_schema()",
        ),
    };
    scope
        .fetch_all(sql)
        .await?
        .iter()
        .map(|row| row.text(0))
        .collect()
}
