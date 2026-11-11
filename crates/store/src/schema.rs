//! Bringing a database to this server's schema, and refusing one it cannot use.
//!
//! The schema is a list of migrations per database kind, compiled into the binary and
//! applied in order by sqlx's migrator, which records them in `_sqlx_migrations`. It
//! checks the checksum of every applied migration, so an edited history is noticed; it
//! refuses a database that holds a migration this binary does not know; and on
//! PostgreSQL it takes an advisory lock, so two servers starting on one empty database
//! do not both create it. Migrations only go forward.

use sqlx::SqlSafeStr as _;
use sqlx::migrate::{MigrateError, Migration, MigrationType, Migrator};

use crate::db::{Dialect, Scope};
use crate::error::StoreError;
use crate::{Pool, Store};

/// One migration: its version, its name, and its SQL for each database kind.
struct Step {
    version: i64,
    description: &'static str,
    sqlite: &'static str,
    postgres: &'static str,
}

/// Every migration, oldest first. Applied ones are never edited: add a new one.
const STEPS: [Step; 12] = [
    Step {
        version: 1,
        description: "baseline",
        sqlite: include_str!("../schema/sqlite/0001_baseline.sql"),
        postgres: include_str!("../schema/postgres/0001_baseline.sql"),
    },
    Step {
        version: 2,
        description: "body",
        sqlite: include_str!("../schema/sqlite/0002_body.sql"),
        postgres: include_str!("../schema/postgres/0002_body.sql"),
    },
    Step {
        version: 3,
        description: "file refs",
        sqlite: include_str!("../schema/sqlite/0003_file_refs.sql"),
        postgres: include_str!("../schema/postgres/0003_file_refs.sql"),
    },
    Step {
        version: 4,
        description: "date values",
        sqlite: include_str!("../schema/sqlite/0004_date_values.sql"),
        postgres: include_str!("../schema/postgres/0004_date_values.sql"),
    },
    Step {
        version: 5,
        description: "uploads",
        sqlite: include_str!("../schema/sqlite/0005_uploads.sql"),
        postgres: include_str!("../schema/postgres/0005_uploads.sql"),
    },
    Step {
        version: 6,
        description: "upload path",
        sqlite: include_str!("../schema/sqlite/0006_upload_path.sql"),
        postgres: include_str!("../schema/postgres/0006_upload_path.sql"),
    },
    Step {
        version: 7,
        description: "upload purpose",
        sqlite: include_str!("../schema/sqlite/0007_upload_purpose.sql"),
        postgres: include_str!("../schema/postgres/0007_upload_purpose.sql"),
    },
    Step {
        version: 8,
        description: "content blocks",
        sqlite: include_str!("../schema/sqlite/0008_content_blocks.sql"),
        postgres: include_str!("../schema/postgres/0008_content_blocks.sql"),
    },
    Step {
        version: 9,
        description: "comment versions",
        sqlite: include_str!("../schema/sqlite/0009_comment_versions.sql"),
        postgres: include_str!("../schema/postgres/0009_comment_versions.sql"),
    },
    Step {
        version: 10,
        description: "display name",
        sqlite: include_str!("../schema/sqlite/0010_display_name.sql"),
        postgres: include_str!("../schema/postgres/0010_display_name.sql"),
    },
    Step {
        version: 11,
        description: "deleted boards by team",
        sqlite: include_str!("../schema/sqlite/0011_deleted_boards_by_team.sql"),
        postgres: include_str!("../schema/postgres/0011_deleted_boards_by_team.sql"),
    },
    Step {
        version: 12,
        description: "deleted board places",
        sqlite: include_str!("../schema/sqlite/0012_deleted_board_places.sql"),
        postgres: include_str!("../schema/postgres/0012_deleted_board_places.sql"),
    },
];

/// The table in which sqlx's migrator records what it applied.
const MIGRATIONS_TABLE: &str = "_sqlx_migrations";

/// A table of this server's schema, by which a database that has tables but no
/// migration record is told from an empty one.
const A_SCHEMA_TABLE: &str = "blocks";

/// Applies the migrations the database does not have yet.
///
/// # Errors
///
/// Returns [`StoreError::Schema`] with the reason if the database holds this server's
/// tables without their migration record, was migrated by a newer server, or has a
/// history that differs from this binary's; and
/// [`StoreError::Database`] if a statement fails.
pub(crate) async fn migrate(store: &Store) -> Result<(), StoreError> {
    store.with_conn(refuse_unrecorded_schemas).await?;

    let migrator = migrator(store.dialect());
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

fn migrator(dialect: Dialect) -> Migrator {
    let migrations = STEPS
        .iter()
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
