//! The migration that names the containers (41): a database a server left with the
//! tables `boards`, `board_members`, `board_teams`, `category_boards` and
//! `deleted_boards` and the columns `board_id` keeps every row, under the new names, and
//! no name of the schema speaks of a board any more.

use fvoci_model::new_id;
use fvoci_store::{DbType, Store};
use sqlx::Connection as _;

use crate::common::{self, db_type};
use crate::tenancy::Before;
use crate::workspace_rename::{Dump, dump};

/// The name a table or a column had before the migration has after it.
fn renamed(name: &str) -> String {
    match name {
        "boards" => "containers".to_owned(),
        "board_members" => "container_members".to_owned(),
        "board_teams" => "container_teams".to_owned(),
        "category_boards" => "category_containers".to_owned(),
        "deleted_boards" => "deleted_containers".to_owned(),
        "board_id" => "container_id".to_owned(),
        "board_role" => "container_role".to_owned(),
        "board" => "container".to_owned(),
        "prior_board_roles" => "prior_container_roles".to_owned(),
        other => other.to_owned(),
    }
}

/// A row as the migration leaves it: what is said of a container in the search's
/// outbox says so.
fn with_the_new_values(
    table: &str,
    columns: &[String],
    mut row: Vec<Option<String>>,
) -> Vec<Option<String>> {
    if table == "search_outbox"
        && let Some(scope) = columns.iter().position(|column| column == "scope")
        && row[scope].as_deref() == Some("board")
    {
        row[scope] = Some("container".to_owned());
    }
    row
}

/// The names of the schema that still speak of a board: tables, columns, indexes,
/// constraints, policies, triggers and functions. On PostgreSQL the bodies of the
/// functions too, but for what they say to whom they refuse.
async fn names_of_a_board(database: &fvoci_store::testing::TestDatabase) -> Vec<String> {
    let dbconfig = common::raw_dbconfig(database);
    match db_type() {
        DbType::Sqlite => {
            let mut conn = sqlx::sqlite::SqliteConnection::connect(&dbconfig)
                .await
                .expect("the database file opens");
            let names = sqlx::query_scalar(
                "SELECT m.type || ' ' || m.name FROM sqlite_master AS m WHERE m.name LIKE '%board%' \
                 UNION ALL SELECT 'column ' || m.name || '.' || c.name \
                 FROM sqlite_master AS m, pragma_table_info(m.name) AS c \
                 WHERE m.type = 'table' AND c.name LIKE '%board%' \
                 UNION ALL SELECT 'index column ' || m.name || '.' || c.name \
                 FROM sqlite_master AS m, pragma_index_info(m.name) AS c \
                 WHERE m.type = 'index' AND c.name LIKE '%board%' \
                 UNION ALL SELECT 'key ' || m.name || ' -> ' || k.\"table\" \
                 FROM sqlite_master AS m, pragma_foreign_key_list(m.name) AS k \
                 WHERE m.type = 'table' AND (k.\"table\" LIKE '%board%' OR k.\"from\" LIKE '%board%')",
            )
            .fetch_all(&mut conn)
            .await
            .expect("the schema is read");
            conn.close().await.expect("the connection closes");
            names
        }
        DbType::Postgres => {
            let mut conn = common::connect_past_the_store(&dbconfig).await;
            let names = sqlx::query_scalar(
                "SELECT 'relation ' || relname::text FROM pg_class \
                 WHERE relnamespace = current_schema()::regnamespace AND relname LIKE '%board%' \
                 UNION ALL SELECT 'column ' || c.relname::text || '.' || a.attname::text \
                 FROM pg_attribute AS a JOIN pg_class AS c ON c.oid = a.attrelid \
                 WHERE c.relnamespace = current_schema()::regnamespace \
                 AND a.attname LIKE '%board%' AND NOT a.attisdropped \
                 UNION ALL SELECT 'constraint ' || conname::text FROM pg_constraint \
                 WHERE connamespace = current_schema()::regnamespace AND conname LIKE '%board%' \
                 UNION ALL SELECT 'policy ' || polname::text FROM pg_policy AS p \
                 JOIN pg_class AS c ON c.oid = p.polrelid \
                 WHERE c.relnamespace = current_schema()::regnamespace AND polname LIKE '%board%' \
                 UNION ALL SELECT 'trigger ' || tgname::text FROM pg_trigger AS t \
                 JOIN pg_class AS c ON c.oid = t.tgrelid \
                 WHERE c.relnamespace = current_schema()::regnamespace AND tgname LIKE '%board%' \
                 UNION ALL SELECT 'function ' || proname::text FROM pg_proc \
                 WHERE pronamespace = current_schema()::regnamespace \
                 AND (proname LIKE '%board%' \
                 OR regexp_replace(prosrc, 'MESSAGE = ''[^'']*''', '', 'g') LIKE '%board%')",
            )
            .fetch_all(&mut conn)
            .await
            .expect("the catalog is read");
            conn.close().await.expect("the connection closes");
            names
        }
    }
}

/// The rows the tables made after the organizations have of a board: a team's role on
/// it, an invitation of a guest onto it, a link of a body on it, the search's note of
/// it, and the roles an administrator held on it before joining the workspace as the
/// organization's administrator.
fn rows_of_the_later_tables(before: &Before, team: &str, invitation: &str) -> String {
    let board = before.id("board");
    let card = before.id("card");
    let workspace = before.id("older_workspace");
    let admin = before.id("admin");
    let org = format!("(SELECT org_id FROM workspaces WHERE id = '{workspace}')");
    let body_links = match db_type() {
        DbType::Sqlite => format!(
            "INSERT INTO body_links (source_id, target_id, board_id) VALUES \
             ('{card}', '{card}', '{board}');"
        ),
        DbType::Postgres => format!(
            "INSERT INTO body_links (source_id, target_id, board_id, org_id) VALUES \
             ('{card}', '{card}', '{board}', {org});"
        ),
    };
    format!(
        "INSERT INTO teams (id, org_id, workspace_id, name, name_key) VALUES \
         ('{team}', {org}, '{workspace}', 'Design', 'design'); \
         INSERT INTO board_teams (board_id, team_id, org_id, workspace_id, role) VALUES \
         ('{board}', '{team}', {org}, '{workspace}', 'editor'); \
         INSERT INTO invitations (id, org_id, workspace_id, role, board_id, board_role, \
         token_hash, created_by, create_at, expires_at) VALUES \
         ('{invitation}', {org}, '{workspace}', 'guest', '{board}', 'commenter', 'hash', \
         '{admin}', 1, 9000000000000); \
         {body_links} \
         INSERT INTO search_outbox (scope, id, org_id, seq, due_at, attempts) VALUES \
         ('board', '{board}', {org}, 1, 1, 0), ('block', '{card}', {org}, 1, 1, 0); \
         UPDATE workspace_members SET prior_board_roles = '[[\"{board}\",[true,true,false,false]]]' \
         WHERE workspace_id = '{workspace}' AND user_id = '{admin}'; \
         DELETE FROM data_steps;"
    )
}

/// A database that a server left before the containers were named so, with rows in
/// every table that has a board's name or a column of it (those of the database before
/// the organizations carried through their migrations, and those of the tables made
/// since): after the migration every table has the rows it had, under the new names,
/// and the search's note of a whole board says 'container'. The keys still hold, and
/// the checks of a row's container find the containers.
#[tokio::test]
async fn a_database_with_boards_keeps_every_row_under_the_names_of_the_containers() {
    let database = common::empty_database().await;
    fvoci_store::testing::migrate_to_before_organizations(&database).await;
    let before = Before::new();
    let raw = common::raw_dbconfig(&database);
    common::execute_raw(&raw, &before.statements()).await;
    fvoci_store::testing::migrate_to_before_containers(&database).await;
    let (team, invitation) = (new_id(), new_id());
    common::execute_raw(&raw, &rows_of_the_later_tables(&before, &team, &invitation)).await;
    // The changes of the data that earlier migrations left to the server are taken as
    // done (their rows removed), so that what changes below is this migration's alone.
    let old = dump(&database).await;
    for table in [
        "boards",
        "board_members",
        "board_teams",
        "category_boards",
        "deleted_boards",
        "invitations",
        "body_links",
        "search_outbox",
    ] {
        assert!(
            !old[table].1.is_empty(),
            "the old {table} has rows: {old:?}"
        );
    }

    // Up to this migration, and not past it: what later ones add is theirs to test.
    fvoci_store::testing::migrate_to_the_containers(&database).await;
    let new = dump(&database).await;
    assert_eq!(names_of_a_board(&database).await, Vec::<String>::new());

    let expected: Dump = old
        .into_iter()
        .map(|(table, (columns, rows))| {
            let rows = rows
                .into_iter()
                .map(|row| with_the_new_values(&table, &columns, row))
                .collect::<Vec<_>>();
            let columns: Vec<String> = columns.iter().map(|column| renamed(column)).collect();
            let mut rows = rows;
            rows.sort();
            (renamed(&table), (columns, rows))
        })
        .collect();
    assert_eq!(
        new.keys().collect::<Vec<_>>(),
        expected.keys().collect::<Vec<_>>(),
        "the same tables, under the new names"
    );
    for (table, (columns, rows)) in &expected {
        let (new_columns, new_rows) = &new[table];
        assert_eq!(
            new_columns, columns,
            "{table}: the same columns, in their order"
        );
        assert_eq!(new_rows, rows, "{table}: the same rows");
    }

    let store = Store::connect(database.db_type(), database.dbconfig())
        .await
        .expect("the store migrates the database");
    the_keys_and_the_checks_find_the_containers(&database).await;
    store.close().await;
    database.remove().await;
}

/// The keys that named the boards name the containers: on SQLite no row refers to
/// nothing; on PostgreSQL the checks of an upload's and a category's container read
/// the renamed table: a container of the organization is taken, an identifier of
/// nothing refused.
async fn the_keys_and_the_checks_find_the_containers(
    database: &fvoci_store::testing::TestDatabase,
) {
    let raw = common::raw_dbconfig(database);
    match db_type() {
        DbType::Sqlite => {
            let mut conn = sqlx::sqlite::SqliteConnection::connect(&raw)
                .await
                .expect("the database file opens");
            let broken = sqlx::query("PRAGMA foreign_key_check")
                .fetch_all(&mut conn)
                .await
                .expect("the keys are checked");
            conn.close().await.expect("the connection closes");
            assert!(broken.is_empty(), "{} rows refer to nothing", broken.len());
        }
        DbType::Postgres => {
            let container: String =
                sqlx::query_scalar("SELECT id::text FROM containers WHERE NOT is_template")
                    .fetch_one(&mut common::connect_past_the_store(&raw).await)
                    .await
                    .expect("a container");
            let filing = |container_id: &str| {
                format!(
                    "INSERT INTO category_containers (id, user_id, category_id, container_id, \
                     org_id, create_at, update_at, sort_order, hidden) SELECT '{}', c.user_id, \
                     c.id, '{container_id}', c.org_id, 0, 0, 0, false FROM categories AS c \
                     LIMIT 1 ON CONFLICT DO NOTHING",
                    new_id()
                )
            };
            common::execute_raw(&raw, &filing(&container)).await;
            let refused = common::try_execute_raw(&raw, &filing(&new_id())).await;
            assert!(
                refused
                    .as_ref()
                    .is_err_and(common::is_foreign_key_violation),
                "a filing of no container is refused: {refused:?}"
            );
        }
    }
}
