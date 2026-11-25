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

mod inspect;
mod steps;

use sqlx::SqlSafeStr as _;
use sqlx::migrate::{MigrateError, Migration, MigrationType, Migrator};

pub(crate) use self::inspect::{SchemaState, may_change, state, ungranted_tables, whereabouts};
use self::steps::STEPS;
use crate::db::Dialect;
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

/// The migration before the one that adds `org_member_removed` to `audit_log`.
#[cfg(any(test, feature = "test-util"))]
pub(crate) const BEFORE_AUDIT_ORG_MEMBER_REMOVED_VERSION: i64 = 64;

/// The migration before course schedules (0066).
#[cfg(any(test, feature = "test-util"))]
pub(crate) const BEFORE_COURSE_SCHEDULES_VERSION: i64 = 65;

/// The migration before unit reviews and notes (0067).
#[cfg(any(test, feature = "test-util"))]
pub(crate) const BEFORE_UNIT_REVIEWS_VERSION: i64 = 66;

/// The migration before course meetings and exams (0068).
#[cfg(any(test, feature = "test-util"))]
pub(crate) const BEFORE_COURSE_EVENTS_VERSION: i64 = 67;

/// The migration before subscriptions `user_id` cleanup (0069).
#[cfg(any(test, feature = "test-util"))]
pub(crate) const BEFORE_SUBSCRIPTIONS_USER_ID_VERSION: i64 = 68;

/// The migration before container visibility, `public_links`, and `files` (0070).
#[cfg(any(test, feature = "test-util"))]
pub(crate) const BEFORE_CONTAINER_VISIBILITY_VERSION: i64 = 69;

/// The migration before native view `fields` keys (0071).
#[cfg(any(test, feature = "test-util"))]
pub(crate) const BEFORE_NATIVE_VIEW_FIELD_KEYS_VERSION: i64 = 70;

/// The migration before nullable `deleted_at` (0072).
#[cfg(any(test, feature = "test-util"))]
pub(crate) const BEFORE_NULLABLE_DELETED_AT_VERSION: i64 = 71;

/// The migration before the single `role` enum on `container_members` (0073).
#[cfg(any(test, feature = "test-util"))]
pub(crate) const BEFORE_CONTAINER_MEMBER_ROLE_ENUM_VERSION: i64 = 72;

/// The migration before native property definitions and lowercase colors (0074).
#[cfg(any(test, feature = "test-util"))]
pub(crate) const BEFORE_NATIVE_PROPERTY_DEFINITIONS_VERSION: i64 = 73;

/// The migration before unified `created_at` / `updated_at` timestamps (0075).
#[cfg(any(test, feature = "test-util"))]
pub(crate) const BEFORE_UNIFIED_TIMESTAMPS_VERSION: i64 = 74;

/// The migration before dedicated `user_settings` / `user_client_state`, removal of
/// `blocks."schema"`, and `organizations.kind` `'shared'` (0076).
#[cfg(any(test, feature = "test-util"))]
pub(crate) const BEFORE_USER_SETTINGS_AND_BLOCK_CLEANUP_VERSION: i64 = 75;

/// The tables that are no organization's: the accounts and what is true of the
/// installation. Every other table of the schema is one of [`ORG_TABLES`].
#[cfg(any(test, feature = "test-util"))]
pub(crate) const GLOBAL_TABLES: [&str; 13] = [
    "_sqlx_migrations",
    "data_steps",
    "email_changes",
    "email_reverts",
    "instance",
    "password_resets",
    "search_index_state",
    "sessions",
    "signups",
    "user_client_state",
    "user_identities",
    "user_settings",
    "users",
];

/// The tables that hold rows of organizations, and the organizations themselves. On
/// PostgreSQL each is behind the wall: row-level security enabled and forced, with the
/// policy `org_wall`. A test of the store reads the catalog to see that every table of
/// the schema is in this list or in [`GLOBAL_TABLES`], and that it is so.
#[cfg(any(test, feature = "test-util"))]
pub(crate) const ORG_TABLES: [&str; 48] = [
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
    "course_exams",
    "course_meetings",
    "course_schedules",
    "deleted_blocks",
    "deleted_containers",
    "diary_days",
    "exam_scores",
    "file_refs",
    "file_versions",
    "files",
    "invitations",
    "loose_objects",
    "meeting_attendance",
    "notification_outbox",
    "notifications",
    "oidc_former_providers",
    "oidc_held_sign_ins",
    "oidc_login_flows",
    "oidc_pending_providers",
    "oidc_providers",
    "organization_members",
    "organizations",
    "public_links",
    "search_outbox",
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
    store.with_conn(inspect::refuse_unrecorded_schemas).await?;

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
            "UPDATE users SET updated_at = (EXTRACT(EPOCH FROM now()) * 1000)::BIGINT;",
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

    /// The migration right after `BEFORE_AUDIT_ORG_MEMBER_REMOVED_VERSION` is 65.
    #[test]
    fn the_migration_after_before_audit_org_member_removed_is_the_one() {
        let step = STEPS
            .iter()
            .find(|step| step.version == BEFORE_AUDIT_ORG_MEMBER_REMOVED_VERSION + 1)
            .expect("the migration after BEFORE_AUDIT_ORG_MEMBER_REMOVED_VERSION is in the list");
        assert_eq!(step.description, "audit org member removed");
    }

    /// Migration 66 is course schedules, immediately following `BEFORE_COURSE_SCHEDULES_VERSION` (65).
    #[test]
    fn migration_66_is_course_schedules() {
        let step = STEPS
            .iter()
            .find(|step| step.version == BEFORE_COURSE_SCHEDULES_VERSION + 1)
            .expect("migration 66 is in the list");
        assert_eq!(step.version, 66);
        assert_eq!(step.description, "course schedules");
    }

    /// Migration 67 is unit reviews and notes, immediately following `BEFORE_UNIT_REVIEWS_VERSION` (66).
    #[test]
    fn migration_67_is_unit_reviews_and_notes() {
        let step = STEPS
            .iter()
            .find(|step| step.version == BEFORE_UNIT_REVIEWS_VERSION + 1)
            .expect("migration 67 is in the list");
        assert_eq!(step.version, 67);
        assert_eq!(step.description, "unit reviews and notes");
    }

    /// Migration 68 is course meetings and exams, immediately following `BEFORE_COURSE_EVENTS_VERSION` (67).
    #[test]
    fn migration_68_is_course_meetings_and_exams() {
        let step = STEPS
            .iter()
            .find(|step| step.version == BEFORE_COURSE_EVENTS_VERSION + 1)
            .expect("migration 68 is in the list");
        assert_eq!(step.version, 68);
        assert_eq!(step.description, "course meetings and exams");
    }

    /// Migration 69 is subscriptions `user_id`, immediately following `BEFORE_SUBSCRIPTIONS_USER_ID_VERSION` (68).
    #[test]
    fn migration_69_is_subscriptions_user_id() {
        let step = STEPS
            .iter()
            .find(|step| step.version == BEFORE_SUBSCRIPTIONS_USER_ID_VERSION + 1)
            .expect("migration 69 is in the list");
        assert_eq!(step.version, 69);
        assert_eq!(step.description, "subscriptions user id");
    }

    /// Migration 70 is container visibility, `public_links`, and `files`, immediately following `BEFORE_CONTAINER_VISIBILITY_VERSION` (69).
    #[test]
    fn migration_70_is_container_visibility_public_links_files() {
        let step = STEPS
            .iter()
            .find(|step| step.version == BEFORE_CONTAINER_VISIBILITY_VERSION + 1)
            .expect("migration 70 is in the list");
        assert_eq!(step.version, 70);
        assert_eq!(step.description, "container visibility public links files");
    }

    /// Migration 71 is native view field keys, immediately following `BEFORE_NATIVE_VIEW_FIELD_KEYS_VERSION` (70).
    #[test]
    fn migration_71_is_native_view_field_keys() {
        let step = STEPS
            .iter()
            .find(|step| step.version == BEFORE_NATIVE_VIEW_FIELD_KEYS_VERSION + 1)
            .expect("migration 71 is in the list");
        assert_eq!(step.version, 71);
        assert_eq!(step.description, "native view field keys");
    }

    /// Migration 72 is nullable `deleted_at`, immediately following `BEFORE_NULLABLE_DELETED_AT_VERSION` (71).
    #[test]
    fn migration_72_is_nullable_deleted_at() {
        let step = STEPS
            .iter()
            .find(|step| step.version == BEFORE_NULLABLE_DELETED_AT_VERSION + 1)
            .expect("migration 72 is in the list");
        assert_eq!(step.version, 72);
        assert_eq!(step.description, "nullable deleted at");
    }

    /// Migration 73 is container member role enum, immediately following `BEFORE_CONTAINER_MEMBER_ROLE_ENUM_VERSION` (72).
    #[test]
    fn migration_73_is_container_member_role_enum() {
        let step = STEPS
            .iter()
            .find(|step| step.version == BEFORE_CONTAINER_MEMBER_ROLE_ENUM_VERSION + 1)
            .expect("migration 73 is in the list");
        assert_eq!(step.version, 73);
        assert_eq!(step.description, "container member role enum");
    }

    /// Migration 74 is native property definitions and colors, immediately following `BEFORE_NATIVE_PROPERTY_DEFINITIONS_VERSION` (73).
    #[test]
    fn migration_74_is_native_property_definitions_and_colors() {
        let step = STEPS
            .iter()
            .find(|step| step.version == BEFORE_NATIVE_PROPERTY_DEFINITIONS_VERSION + 1)
            .expect("migration 74 is in the list");
        assert_eq!(step.version, 74);
        assert_eq!(step.description, "native property definitions and colors");
    }

    /// Migration 75 is unified `created_at` / `updated_at` timestamps, immediately following `BEFORE_UNIFIED_TIMESTAMPS_VERSION` (74).
    #[test]
    fn migration_75_is_unified_created_updated_timestamps() {
        let step = STEPS
            .iter()
            .find(|step| step.version == BEFORE_UNIFIED_TIMESTAMPS_VERSION + 1)
            .expect("migration 75 is in the list");
        assert_eq!(step.version, 75);
        assert_eq!(step.description, "unified created updated timestamps");
    }

    /// Migration 76 splits `preferences` into `user_settings` and `user_client_state`,
    /// drops `blocks."schema"`, and updates shared `organizations.kind` to `'shared'`.
    #[test]
    fn migration_76_is_user_settings_client_state_and_block_cleanup() {
        let step = STEPS
            .iter()
            .find(|step| step.version == BEFORE_USER_SETTINGS_AND_BLOCK_CLEANUP_VERSION + 1)
            .expect("migration 76 is in the list");
        assert_eq!(step.version, 76);
        assert_eq!(
            step.description,
            "user settings client state and block cleanup"
        );
    }
}
