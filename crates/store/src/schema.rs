//! Creating the schema of a new database and checking the schema of an existing one.
//!
//! The original server builds its schema with 40 migrations and records them in a
//! bookkeeping table. This server does not carry those migrations. It creates the final
//! schema in one step and writes the same bookkeeping, so either server can open a
//! database the other created. A database that is not at version 40 is refused.

use crate::db::{Dialect, Scope};
use crate::error::StoreError;

/// The schema version this server reads and writes.
pub(crate) const SCHEMA_VERSION: i64 = 40;

/// The names of the original migrations, in order. Version `n` is entry `n - 1`.
const MIGRATION_NAMES: [&str; 40] = [
    "init",
    "system_settings_table",
    "blocks_rootid",
    "auth_table",
    "blocks_modifiedby",
    "sharing_table",
    "workspaces_table",
    "teams",
    "blocks_history",
    "blocks_created_by",
    "match_collation",
    "match_column_collation",
    "millisecond_timestamps",
    "add_not_null_constraint",
    "blocks_history_no_nulls",
    "subscriptions_table",
    "add_file_info",
    "add_teams_and_boards",
    "populate_categories",
    "populate_category_blocks",
    "create_boards_members_history",
    "create_default_board_role",
    "persist_category_collapsed_state",
    "mark_existsing_categories_collapsed",
    "indexes_update",
    "create_preferences_table",
    "migrate_user_props_to_preferences",
    "remove_template_channel_link",
    "add_category_type_field",
    "add_category_sort_order",
    "add_category_boards_sort_order",
    "move_boards_category_to_end",
    "remove_deleted_category_boards",
    "category_boards_remove_unused_delete_at_column",
    "add_hidden_board_column",
    "category_board_add_unique_constraint",
    "hidden_boards_from_preferences",
    "delete_hiddenBoardIDs_from_preferences",
    "add_path_to_file_info",
    "fix_fileinfo_soft_deletes",
];

/// System settings by which the original server remembers that its data migrations have
/// run. A new database has nothing to migrate, so they are set from the start, as the
/// original does.
const COMPLETED_DATA_MIGRATIONS: [&str; 3] = [
    "UniqueIDsMigrationComplete",
    "CategoryUuidIdMigrationComplete",
    "DeDuplicateCategoryBoardTableComplete",
];

/// An arbitrary constant identifying this crate's schema setup to PostgreSQL's advisory
/// locks, so that two servers starting on one empty database do not both create it.
const SETUP_LOCK_KEY: i64 = 0x6676_6f63_6900_0028;

/// Creates the schema if the database is empty, and otherwise checks that it is at
/// [`SCHEMA_VERSION`]. Must run inside a transaction.
pub(crate) async fn ensure(scope: &mut Scope<'_>) -> Result<(), StoreError> {
    if scope.dialect() == Dialect::Postgres {
        let mut lock = scope.sql("SELECT pg_advisory_xact_lock(");
        lock.bind(SETUP_LOCK_KEY).push(")");
        scope.execute(lock).await?;
    }

    let migrations_table = migrations_table(scope);
    if table_exists(scope, &migrations_table).await? {
        return check_version(scope, &migrations_table).await;
    }
    let blocks_table = scope.table("blocks");
    if table_exists(scope, &blocks_table).await? {
        return Err(StoreError::Schema(format!(
            "the database has a {blocks_table} table but no {migrations_table} table; \
             upgrade it to schema version {SCHEMA_VERSION} with the original server first"
        )));
    }
    create(scope, &migrations_table).await
}

/// The name of the migration bookkeeping table. The original's migration engine uses a
/// fixed name on SQLite and a prefixed one on PostgreSQL.
fn migrations_table(scope: &Scope<'_>) -> String {
    match scope.dialect() {
        Dialect::Sqlite => "db_migrations".to_owned(),
        Dialect::Postgres => scope.table("schema_migrations"),
    }
}

async fn table_exists(scope: &mut Scope<'_>, table: &str) -> Result<bool, StoreError> {
    let mut sql = match scope.dialect() {
        Dialect::Sqlite => {
            scope.sql("SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ")
        }
        Dialect::Postgres => scope.sql(
            "SELECT 1 FROM information_schema.tables \
             WHERE table_schema = current_schema() AND table_name = ",
        ),
    };
    sql.bind(table);
    Ok(scope.fetch_optional(sql).await?.is_some())
}

async fn check_version(scope: &mut Scope<'_>, migrations_table: &str) -> Result<(), StoreError> {
    let sql = scope.sql(&format!(
        "SELECT CAST(COALESCE(MAX(version), 0) AS BIGINT) FROM {migrations_table}"
    ));
    let version = match scope.fetch_optional(sql).await? {
        Some(row) => row.int(0)?,
        None => 0,
    };
    match version {
        SCHEMA_VERSION => Ok(()),
        older if older < SCHEMA_VERSION => Err(StoreError::Schema(format!(
            "the database is at schema version {older}; upgrade it to version \
             {SCHEMA_VERSION} with the original server first"
        ))),
        newer => Err(StoreError::Schema(format!(
            "the database is at schema version {newer}, newer than version \
             {SCHEMA_VERSION} that this server supports"
        ))),
    }
}

async fn create(scope: &mut Scope<'_>, migrations_table: &str) -> Result<(), StoreError> {
    let template = match scope.dialect() {
        Dialect::Sqlite => include_str!("../schema/sqlite.sql"),
        Dialect::Postgres => include_str!("../schema/postgres.sql"),
    };
    scope
        .execute_script(template.replace("{p}", &scope.table("")))
        .await?;

    let bookkeeping = match scope.dialect() {
        Dialect::Sqlite => format!(
            "CREATE TABLE {migrations_table} \
             (Version bigint not null primary key, Name varchar not null)"
        ),
        Dialect::Postgres => format!(
            "CREATE TABLE {migrations_table} \
             (version bigint not null primary key, name varchar not null)"
        ),
    };
    scope.execute_script(bookkeeping).await?;

    for (version, name) in (1_i64..).zip(MIGRATION_NAMES) {
        let mut sql = scope.sql(&format!(
            "INSERT INTO {migrations_table} (version, name) VALUES ("
        ));
        sql.bind(version).push(", ").bind(name).push(")");
        scope.execute(sql).await?;
    }

    let settings_table = scope.table("system_settings");
    for key in COMPLETED_DATA_MIGRATIONS {
        let mut sql = scope.sql(&format!(
            "INSERT INTO {settings_table} (id, value) VALUES ("
        ));
        sql.bind(key).push(", 'true')");
        scope.execute(sql).await?;
    }
    Ok(())
}
