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

/// The migration right after `BEFORE_AUDIT_ORG_MEMBER_REMOVED_VERSION` is 65.
#[test]
fn the_migration_after_before_audit_org_member_removed_is_the_one() {
    let step = STEPS
        .iter()
        .find(|step| step.version == BEFORE_AUDIT_ORG_MEMBER_REMOVED_VERSION + 1)
        .expect("the migration after BEFORE_AUDIT_ORG_MEMBER_REMOVED_VERSION is in the list");
    assert_eq!(step.description, "audit org member removed");
}
