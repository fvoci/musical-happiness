//! Bringing a database to this server's schema, and refusing one it cannot use.
//!
//! The schema is a list of migrations per database kind, compiled into the binary and
//! applied in order by sqlx's migrator, which records them in `_sqlx_migrations`. It
//! checks the checksum of every applied migration, so an edited history is noticed; it
//! refuses a database that holds a migration this binary does not know; and on
//! PostgreSQL it takes an advisory lock, so two servers starting on one empty database
//! do not both create it. Migrations only go forward.
//!
//! The two files of a migration describe one change and differ in the column types
//! only, as the tests of the baselines hold them to. The migration of the organizations
//! is the exception on two counts, and a later table of an organization on the first.
//! PostgreSQL keeps the rows of the organizations apart with row-level security, which
//! SQLite does not have: there the tables whose statements do not name the organization
//! get the column `org_id`, with the organization of the transaction as its default,
//! and every table of an organization gets its policy; SQLite holds one organization
//! and gets neither. And
//! SQLite adds no column that both takes no NULL and refers to another table, so where
//! PostgreSQL alters `teams` (the table of the workspaces before migration 17 named it
//! so), SQLite makes the table again; and SQLite alone has the
//! triggers that hold it to one organization. The migration after it, of the keys that
//! name the organization, is PostgreSQL's alone: SQLite's file says why it is empty.

mod steps;

use sqlx::SqlSafeStr as _;
use sqlx::migrate::{MigrateError, Migration, MigrationType, Migrator};

use self::steps::STEPS;
use crate::db::{Dialect, Scope};
use crate::error::StoreError;
use crate::{Pool, Store};

/// The migration that makes the organizations and, on PostgreSQL, puts up the wall
/// between them.
#[cfg(any(test, feature = "test-util"))]
pub(crate) const ORGANIZATIONS_VERSION: i64 = 14;

/// The migration that renames the table of the workspaces and the columns that name one.
#[cfg(any(test, feature = "test-util"))]
pub(crate) const WORKSPACES_VERSION: i64 = 17;

/// The migration that makes a workspace's name its own in its organization, among much
/// else of the organization's management.
#[cfg(any(test, feature = "test-util"))]
pub(crate) const ORG_MANAGEMENT_VERSION: i64 = 23;

/// The migration that renames the tables of the boards and wikis, the containers, and
/// the columns that name one.
#[cfg(any(test, feature = "test-util"))]
pub(crate) const CONTAINERS_VERSION: i64 = 41;

/// The migration before the one of the diaries, which widens the kinds of a container.
#[cfg(any(test, feature = "test-util"))]
pub(crate) const BEFORE_DIARIES_VERSION: i64 = 54;

/// The migration before the one of the courses, which widens the kinds of a container.
#[cfg(any(test, feature = "test-util"))]
pub(crate) const BEFORE_COURSES_VERSION: i64 = 60;

/// The migration before unit progress (0063).
#[cfg(any(test, feature = "test-util"))]
pub(crate) const BEFORE_UNIT_PROGRESS_VERSION: i64 = 62;

/// The migration version right before `time_entries` (64).
#[cfg(any(test, feature = "test-util"))]
pub(crate) const BEFORE_TIME_ENTRIES_VERSION: i64 = 63;

/// The migration before unit reviews and notes (0067).
#[cfg(any(test, feature = "test-util"))]
pub(crate) const BEFORE_UNIT_REVIEWS_VERSION: i64 = 66;

/// The tables that are no organization's: the accounts and what is true of the
/// installation. Every other table of the schema is one of [`ORG_TABLES`].
#[cfg(any(test, feature = "test-util"))]
pub(crate) const GLOBAL_TABLES: [&str; 12] = [
    "_sqlx_migrations",
    "data_steps",
    "email_changes",
    "email_reverts",
    "instance",
    "preferences",
    "password_resets",
    "search_index_state",
    "sessions",
    "signups",
    "user_identities",
    "users",
];

/// The tables that hold rows of organizations, and the organizations themselves. On
/// PostgreSQL each is behind the wall: row-level security enabled and forced, with the
/// policy `org_wall`. A test of the store reads the catalog to see that every table of
/// the schema is in this list or in [`GLOBAL_TABLES`], and that it is so.
#[cfg(any(test, feature = "test-util"))]
pub(crate) const ORG_TABLES: [&str; 43] = [
    "audit_log",
    "blocks",
    "body_docs",
    "body_links",
    "body_updates",
    "body_version_editors",
    "body_versions",
    "categories",
    "category_containers",
    "comment_versions",
    "container_members",
    "container_teams",
    "containers",
    "deleted_blocks",
    "deleted_containers",
    "diary_days",
    "file_info",
    "file_refs",
    "file_versions",
    "invitations",
    "loose_objects",
    "notification_outbox",
    "notifications",
    "oidc_former_providers",
    "oidc_held_sign_ins",
    "oidc_login_flows",
    "oidc_pending_providers",
    "oidc_providers",
    "organization_members",
    "organizations",
    "search_outbox",
    "sharing",
    "subscriptions",
    "team_members",
    "teams",
    "time_entries",
    "unit_notes",
    "unit_progress",
    "unit_reviews",
    "upload_parts",
    "uploads",
    "workspace_members",
    "workspaces",
];

/// The table in which sqlx's migrator records what it applied.
const MIGRATIONS_TABLE: &str = "_sqlx_migrations";

/// A table of this server's schema, by which a database that has tables but no
/// migration record is told from an empty one.
const A_SCHEMA_TABLE: &str = "blocks";

/// Applies the migrations the database does not have yet, all of them or, for a test,
/// those up to a version.
///
/// # Errors
///
/// Returns [`StoreError::Schema`] with the reason if the database holds this server's
/// tables without their migration record, was migrated by a newer server, or has a
/// history that differs from this binary's; and
/// [`StoreError::Database`] if a statement fails.
pub(crate) async fn migrate(store: &Store, up_to: Option<i64>) -> Result<(), StoreError> {
    store.with_conn(refuse_unrecorded_schemas).await?;

    let migrator = migrator(store.dialect(), up_to);
    let result = match &store.inner.pool {
        Pool::Sqlite(pool) => migrator.run(pool).await,
        Pool::Postgres(pool) => migrator.run(pool).await,
    };
    result.map_err(|error| match error {
        MigrateError::Execute(error) | MigrateError::ExecuteMigration(error, _) => {
            StoreError::Database(error)
        }
        MigrateError::VersionMissing(version) => StoreError::Schema(format!(
            "the database holds schema migration {version}, which this server does not \
             know: it was last opened by a newer server"
        )),
        MigrateError::VersionMismatch(version) => StoreError::Schema(format!(
            "schema migration {version} of the database differs from this server's: the \
             database was created by a different build"
        )),
        other => StoreError::Schema(other.to_string()),
    })
}

/// The version of the newest migration.
#[cfg(any(test, feature = "test-util"))]
pub(crate) fn latest_version() -> i64 {
    STEPS.last().map_or(0, |step| step.version)
}

/// Where a database's schema stands against this build's migrations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SchemaState {
    /// Every migration of this build is recorded as applied, with this build's text,
    /// and no other: the database is opened without a statement that changes it.
    Current,
    /// Some of this build's migrations are not recorded yet (none, if the database is
    /// empty); what is recorded is this build's.
    Behind,
    /// The record is not this build's: why, as the migrator would say it.
    Foreign(String),
}

/// Reads where the database's schema stands, from the record of the migrations.
///
/// # Errors
///
/// Returns [`StoreError::Schema`] if the record is there but the role may not read it
/// (it was not granted the tables), and [`StoreError::Database`] if it cannot be read.
pub(crate) async fn state(store: &Store) -> Result<SchemaState, StoreError> {
    let migrator = migrator(store.dialect(), None);
    let known: Vec<(i64, &[u8])> = migrator
        .iter()
        .map(|migration| (migration.version, migration.checksum.as_ref()))
        .collect();
    store
        .with_conn(async |scope| {
            if !has_record(scope).await? {
                return Ok(SchemaState::Behind);
            }
            let sql = scope
                .sql("SELECT version, checksum, success FROM _sqlx_migrations ORDER BY version");
            let recorded = scope
                .fetch_all(sql)
                .await?
                .iter()
                .map(|row| Ok((row.int(0)?, row.bytes(1)?, row.bool(2)?)))
                .collect::<Result<Vec<_>, StoreError>>()?;
            for (version, checksum, applied) in &recorded {
                let reason = match known.iter().find(|(known, _)| known == version) {
                    None => format!(
                        "the database holds schema migration {version}, which this server \
                         does not know: it was last opened by a newer server"
                    ),
                    Some((_, known)) if known != checksum => format!(
                        "schema migration {version} of the database differs from this \
                         server's: the database was created by a different build"
                    ),
                    Some(_) if !applied => {
                        format!("schema migration {version} failed when it was applied")
                    }
                    Some(_) => continue,
                };
                return Ok(SchemaState::Foreign(reason));
            }
            Ok(if recorded.len() == known.len() {
                SchemaState::Current
            } else {
                SchemaState::Behind
            })
        })
        .await
}

/// Whether the record of the migrations is in the schema the role works in. On
/// PostgreSQL it is found whatever the role was granted, and a record the role may not
/// read is an error that says what to grant.
async fn has_record(scope: &mut Scope<'_>) -> Result<bool, StoreError> {
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

/// The migrator of every migration, or of those up to a version: a database as an
/// older server left it, for the tests of what a migration makes of one.
fn migrator(dialect: Dialect, up_to: Option<i64>) -> Migrator {
    let migrations = STEPS
        .iter()
        .filter(|step| up_to.is_none_or(|version| step.version <= version))
        .map(|step| {
            let sql = match dialect {
                Dialect::Sqlite => step.sqlite,
                Dialect::Postgres => step.postgres,
            };
            Migration::new(
                step.version,
                step.description.into(),
                MigrationType::Simple,
                sql.into_sql_str(),
                false,
            )
        })
        .collect();
    Migrator::with_migrations(migrations)
}

/// Refuses a database that has this server's tables but no record of the migrations
/// that made them, which the migrator would try to create again.
async fn refuse_unrecorded_schemas(scope: &mut Scope<'_>) -> Result<(), StoreError> {
    let tables = table_names(scope).await?;
    let has = |name: &str| tables.iter().any(|table| table == name);
    if has(A_SCHEMA_TABLE) && !has(MIGRATIONS_TABLE) {
        return Err(StoreError::Schema(format!(
            "the database has a {A_SCHEMA_TABLE} table but no record of schema migrations \
             ({MIGRATIONS_TABLE}); start with an empty database"
        )));
    }
    Ok(())
}

/// The names of the tables in the database (on PostgreSQL: in the current schema).
async fn table_names(scope: &mut Scope<'_>) -> Result<Vec<String>, StoreError> {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// What a PostgreSQL migration after the wall begins with if it reads or changes
    /// rows of an organization's table: the migrator's transaction names no
    /// organization, so without it the statements find no row, and say nothing.
    const SYSTEM_SCOPE: &str = "SELECT set_config('fvoci.system', 'on', true);";

    /// The lines of a migration without their comments, the blank ones left out. (No
    /// string in the schema holds `--`.)
    fn statement_lines(sql: &str) -> impl Iterator<Item = &str> {
        sql.lines()
            .map(|line| line.split("--").next().unwrap_or_default().trim())
            .filter(|line| !line.is_empty())
    }

    /// The words of a migration: its runs of letters, digits and underscores, in lower
    /// case, comments left out.
    fn words(sql: &str) -> Vec<String> {
        statement_lines(sql)
            .flat_map(|line| line.split(|c: char| !c.is_ascii_alphanumeric() && c != '_'))
            .filter(|word| !word.is_empty())
            .map(str::to_ascii_lowercase)
            .collect()
    }

    /// The tables of organizations whose rows a migration may read or change: every
    /// one it names anywhere, if it has a statement that reads or changes rows at all.
    /// More than it must be: a migration that changes the rows of one table and the
    /// shape of another names both. A line too many at its head costs nothing; a
    /// statement that finds no row costs the data.
    fn walled_tables(sql: &str) -> Vec<String> {
        let words = words(sql);
        let touches_rows = words
            .iter()
            .any(|word| ["select", "insert", "update", "delete"].contains(&word.as_str()));
        if !touches_rows {
            return Vec::new();
        }
        let mut walled: Vec<String> = words
            .into_iter()
            .filter(|word| ORG_TABLES.contains(&word.as_str()))
            .collect();
        walled.sort_unstable();
        walled.dedup();
        walled
    }

    fn begins_in_the_system_scope(sql: &str) -> bool {
        statement_lines(sql).next() == Some(SYSTEM_SCOPE)
    }

    /// After the wall is up, a PostgreSQL migration that reads or changes rows of an
    /// organization's table begins by reaching the rows of all: forced on the owner,
    /// the policies would otherwise leave its `UPDATE` and `DELETE` without a row and
    /// its `INSERT ... SELECT` without a source, with no error.
    #[test]
    fn a_migration_after_the_wall_that_touches_an_organizations_rows_begins_in_the_system_scope() {
        for step in STEPS
            .iter()
            .filter(|step| step.version > ORGANIZATIONS_VERSION)
        {
            let walled = walled_tables(step.postgres);
            assert!(
                walled.is_empty() || begins_in_the_system_scope(step.postgres),
                "migration {} ({}) reads or changes rows of {walled:?}; its PostgreSQL file \
                 must begin with the line: {SYSTEM_SCOPE}",
                step.version,
                step.description
            );
        }
    }

    /// The check sees what it is there to see: statements of every kind that touch a
    /// table of an organization, also in a subquery, and not those that touch none or
    /// only change the shape of a table.
    #[test]
    fn the_check_of_later_migrations_sees_what_it_is_there_to_see() {
        for touching in [
            "UPDATE organizations SET name = 'ours'",
            "INSERT INTO data_steps (name) SELECT b.title FROM users AS u, containers AS b;",
            "DELETE FROM sessions USING containers WHERE sessions.user_id = containers.created_by",
            "ALTER TABLE containers ADD COLUMN color TEXT; UPDATE users SET email = ''",
            // More than it must be, and meant: `DELETE` is a word here, not a statement.
            "CREATE TABLE notes (org_id UUID REFERENCES organizations (id) ON DELETE CASCADE)",
            "UPDATE containers SET title = ''",
            "DELETE FROM deleted_blocks WHERE deleted_at < 5;",
            "INSERT INTO file_refs (block_id) VALUES ('x')",
            "INSERT INTO data_steps (name) SELECT title FROM containers;",
            "update users set display_name = (select title from containers limit 1)",
            "CREATE TABLE copies AS SELECT b.id FROM blocks AS b JOIN workspaces t ON true",
            "-- a comment\nUPDATE users SET email = '' WHERE id IN (SELECT user_id FROM container_members)",
        ] {
            assert!(!walled_tables(touching).is_empty(), "{touching}");
            assert!(!begins_in_the_system_scope(touching), "{touching}");
        }
        for not_touching in [
            "ALTER TABLE containers ADD COLUMN color TEXT NOT NULL DEFAULT '';",
            "CREATE INDEX blocks_title_idx ON blocks (title);",
            "CREATE TABLE notes (id UUID PRIMARY KEY, container_id UUID REFERENCES containers (id));",
            "INSERT INTO data_steps (name) VALUES ('later');",
            "UPDATE users SET display_name = '' -- not from containers",
            "UPDATE users SET update_at = (EXTRACT(EPOCH FROM now()) * 1000)::BIGINT;",
            "DELETE FROM sessions WHERE user_id IN (SELECT id FROM (SELECT id FROM users) AS u)",
        ] {
            assert_eq!(
                walled_tables(not_touching),
                Vec::<String>::new(),
                "{not_touching}"
            );
        }
        let scoped = format!("-- why\n{SYSTEM_SCOPE}\nUPDATE containers SET title = '';");
        assert!(begins_in_the_system_scope(&scoped), "comments come first");
        assert!(
            !begins_in_the_system_scope(&format!(
                "UPDATE containers SET title = '';\n{SYSTEM_SCOPE}"
            )),
            "the first statement, not a later one"
        );
    }

    /// No table is both an organization's and nobody's.
    #[test]
    fn the_two_lists_of_tables_share_no_table() {
        for table in ORG_TABLES {
            assert!(!GLOBAL_TABLES.contains(&table), "{table} is in both lists");
        }
    }

    /// The migration of the organizations is in the list under the version the rest of
    /// the crate knows it by.
    #[test]
    fn the_migration_of_the_organizations_is_the_one_the_crate_names() {
        let step = STEPS
            .iter()
            .find(|step| step.version == ORGANIZATIONS_VERSION)
            .expect("the migration is in the list");
        assert_eq!(step.description, "organizations");
        assert!(
            step.postgres.contains("CREATE POLICY org_wall"),
            "it puts up the wall"
        );
    }

    /// The migration right after `BEFORE_DIARIES_VERSION` is the one of the diaries.
    #[test]
    fn the_migration_after_before_diaries_is_the_diaries_one() {
        let step = STEPS
            .iter()
            .find(|step| step.version == BEFORE_DIARIES_VERSION + 1)
            .expect("the migration after BEFORE_DIARIES_VERSION is in the list");
        assert_eq!(step.description, "diaries");
    }

    /// The migration right after `BEFORE_COURSES_VERSION` is the one of the courses.
    #[test]
    fn the_migration_after_before_courses_is_the_courses_one() {
        let step = STEPS
            .iter()
            .find(|step| step.version == BEFORE_COURSES_VERSION + 1)
            .expect("the migration after BEFORE_COURSES_VERSION is in the list");
        assert_eq!(step.description, "courses");
    }

    /// Migration 63 is unit progress, following `BEFORE_UNIT_PROGRESS_VERSION`.
    #[test]
    fn migration_63_is_unit_progress() {
        let step = STEPS
            .iter()
            .find(|step| step.version == 63)
            .expect("migration 63 is in the list");
        assert_eq!(step.description, "unit progress");
        assert!(BEFORE_UNIT_PROGRESS_VERSION < step.version);
    }

    /// The migration right after `BEFORE_TIME_ENTRIES_VERSION` is the one of time entries.
    #[test]
    fn the_migration_after_before_time_entries_is_the_time_entries_one() {
        let step = STEPS
            .iter()
            .find(|step| step.version == BEFORE_TIME_ENTRIES_VERSION + 1)
            .expect("the migration after BEFORE_TIME_ENTRIES_VERSION is in the list");
        assert_eq!(step.description, "time entries");
    }

    /// Migration 67 is unit reviews and notes, following `BEFORE_UNIT_REVIEWS_VERSION`.
    #[test]
    fn migration_67_is_unit_reviews_and_notes() {
        let step = STEPS
            .iter()
            .find(|step| step.version == 67)
            .expect("migration 67 is in the list");
        assert_eq!(step.description, "unit reviews and notes");
        assert!(BEFORE_UNIT_REVIEWS_VERSION < step.version);
    }
}
