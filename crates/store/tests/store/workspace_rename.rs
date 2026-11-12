//! The migration that names the workspaces (17): a database a server left with the
//! table `teams` and the columns `team_id` keeps every row, under the new names, and
//! no name of the schema speaks of a team any more.

use std::collections::BTreeMap;

use fvoci_model::{Upload, UploadPurpose, Workspace, new_id};
use fvoci_store::{DbType, Store};
use serde_json::Value;
use sqlx::Connection as _;

use crate::common::{self, db_type};
use crate::tenancy::Before;

/// The table of the migrator's records, which the migration adds to.
const MIGRATIONS: &str = "_sqlx_migrations";

/// Every table of a database: its columns in their order, and its rows as text (`NULL`
/// as `None`), sorted.
type Dump = BTreeMap<String, (Vec<String>, Vec<Vec<Option<String>>>)>;

/// The name a table or a column had before the migration has after it.
fn renamed(name: &str) -> String {
    match name {
        "teams" => "workspaces".to_owned(),
        "team_id" => "workspace_id".to_owned(),
        other => other.to_owned(),
    }
}

/// Reads every table of the database past the store, the migrator's own left out.
async fn dump(database: &fvoci_store::testing::TestDatabase) -> Dump {
    let dbconfig = common::raw_dbconfig(database);
    let mut dump = Dump::new();
    match db_type() {
        DbType::Sqlite => {
            let mut conn = sqlx::sqlite::SqliteConnection::connect(&dbconfig)
                .await
                .expect("the database file opens");
            let tables: Vec<String> = sqlx::query_scalar(
                "SELECT name FROM sqlite_master WHERE type = 'table' \
                 AND name NOT LIKE 'sqlite_%'",
            )
            .fetch_all(&mut conn)
            .await
            .expect("the tables are listed");
            for table in tables.into_iter().filter(|table| table != MIGRATIONS) {
                let columns: Vec<String> =
                    sqlx::query_scalar("SELECT name FROM pragma_table_info(?) ORDER BY cid")
                        .bind(&table)
                        .fetch_all(&mut conn)
                        .await
                        .expect("the columns are listed");
                let sql = select_as_text(&table, &columns);
                let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
                    .fetch_all(&mut conn)
                    .await
                    .expect("the rows are read");
                let rows = rows.iter().map(|row| texts(row, columns.len())).collect();
                dump.insert(table, (columns, sorted(rows)));
            }
            conn.close().await.expect("the connection closes");
        }
        DbType::Postgres => {
            let mut conn = common::connect_past_the_store(&dbconfig).await;
            let tables: Vec<String> = sqlx::query_scalar(
                "SELECT table_name::text FROM information_schema.tables \
                 WHERE table_schema = current_schema() AND table_type = 'BASE TABLE'",
            )
            .fetch_all(&mut conn)
            .await
            .expect("the tables are listed");
            for table in tables.into_iter().filter(|table| table != MIGRATIONS) {
                let columns: Vec<String> = sqlx::query_scalar(
                    "SELECT column_name::text FROM information_schema.columns \
                     WHERE table_schema = current_schema() AND table_name = $1 \
                     ORDER BY ordinal_position",
                )
                .bind(&table)
                .fetch_all(&mut conn)
                .await
                .expect("the columns are listed");
                let sql = select_as_text(&table, &columns);
                let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
                    .fetch_all(&mut conn)
                    .await
                    .expect("the rows are read");
                let rows = rows.iter().map(|row| texts(row, columns.len())).collect();
                dump.insert(table, (columns, sorted(rows)));
            }
            conn.close().await.expect("the connection closes");
        }
    }
    dump
}

fn select_as_text(table: &str, columns: &[String]) -> String {
    let columns: Vec<String> = columns
        .iter()
        .map(|column| format!("CAST(\"{column}\" AS TEXT)"))
        .collect();
    format!("SELECT {} FROM \"{table}\"", columns.join(", "))
}

fn texts<R: sqlx::Row>(row: &R, columns: usize) -> Vec<Option<String>>
where
    usize: sqlx::ColumnIndex<R>,
    for<'r> Option<String>: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
{
    (0..columns)
        .map(|index| row.try_get(index).expect("a column read as text"))
        .collect()
}

fn sorted(mut rows: Vec<Vec<Option<String>>>) -> Vec<Vec<Option<String>>> {
    rows.sort();
    rows
}

/// A board in the trash as JSON, with the member that names its workspace under the
/// name it has after the migration.
fn board_with_the_new_name(text: &str) -> Value {
    let mut board: Value = serde_json::from_str(text).expect("a board in the trash is JSON");
    if let Some(object) = board.as_object_mut()
        && let Some(workspace) = object.remove("teamId")
    {
        object.insert("workspaceId".to_owned(), workspace);
    }
    board
}

/// The names of the schema that still speak of a team: tables, columns, indexes,
/// constraints, triggers, and on PostgreSQL the bodies of functions.
async fn names_of_a_team(database: &fvoci_store::testing::TestDatabase) -> Vec<String> {
    let dbconfig = common::raw_dbconfig(database);
    let names: Vec<String> = match db_type() {
        DbType::Sqlite => {
            let mut conn = sqlx::sqlite::SqliteConnection::connect(&dbconfig)
                .await
                .expect("the database file opens");
            let names = sqlx::query_scalar(
                "SELECT m.type || ' ' || m.name FROM sqlite_master AS m WHERE m.name LIKE '%team%' \
                 UNION ALL SELECT 'column ' || m.name || '.' || c.name \
                 FROM sqlite_master AS m, pragma_table_info(m.name) AS c \
                 WHERE m.type = 'table' AND c.name LIKE '%team%' \
                 UNION ALL SELECT 'index column ' || m.name || '.' || c.name \
                 FROM sqlite_master AS m, pragma_index_info(m.name) AS c \
                 WHERE m.type = 'index' AND c.name LIKE '%team%' \
                 UNION ALL SELECT 'key ' || m.name || ' -> ' || k.\"table\" \
                 FROM sqlite_master AS m, pragma_foreign_key_list(m.name) AS k \
                 WHERE m.type = 'table' AND (k.\"table\" LIKE '%team%' OR k.\"from\" LIKE '%team%')",
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
                 WHERE relnamespace = current_schema()::regnamespace AND relname LIKE '%team%' \
                 UNION ALL SELECT 'column ' || c.relname::text || '.' || a.attname::text \
                 FROM pg_attribute AS a JOIN pg_class AS c ON c.oid = a.attrelid \
                 WHERE c.relnamespace = current_schema()::regnamespace \
                 AND a.attname LIKE '%team%' AND NOT a.attisdropped \
                 UNION ALL SELECT 'constraint ' || conname::text FROM pg_constraint \
                 WHERE connamespace = current_schema()::regnamespace AND conname LIKE '%team%' \
                 UNION ALL SELECT 'policy ' || polname::text FROM pg_policy AS p \
                 JOIN pg_class AS c ON c.oid = p.polrelid \
                 WHERE c.relnamespace = current_schema()::regnamespace AND polname LIKE '%team%' \
                 UNION ALL SELECT 'trigger ' || tgname::text FROM pg_trigger AS t \
                 JOIN pg_class AS c ON c.oid = t.tgrelid \
                 WHERE c.relnamespace = current_schema()::regnamespace AND tgname LIKE '%team%' \
                 UNION ALL SELECT 'function ' || proname::text FROM pg_proc \
                 WHERE pronamespace = current_schema()::regnamespace \
                 AND (proname LIKE '%team%' OR prosrc LIKE '%team%')",
            )
            .fetch_all(&mut conn)
            .await
            .expect("the catalog is read");
            conn.close().await.expect("the connection closes");
            names
        }
    };
    names
}

/// A database that a server left before the workspaces were named so, with rows in
/// every table (those of the database before the organizations, carried through their
/// migrations), a board in the trash whose JSON names its team, and an archive being
/// uploaded into a team: after the migration every table has the rows it had, under
/// the new names, and the board in the trash names its workspace as a board does now.
/// The keys still hold, and the check of an upload's target finds the workspaces.
#[tokio::test]
async fn a_database_with_teams_keeps_every_row_under_the_names_of_the_workspaces() {
    let database = common::empty_database().await;
    fvoci_store::testing::migrate_to_before_organizations(&database).await;
    let before = Before::new();
    let raw = common::raw_dbconfig(&database);
    common::execute_raw(&raw, &before.statements()).await;
    fvoci_store::testing::migrate_to_before_workspaces(&database).await;
    let deleted_board = before.id("deleted_board");
    let workspace = before.id("older_workspace");
    let admin = before.id("admin");
    common::execute_raw(
        &raw,
        &format!(
            "UPDATE deleted_boards SET board = '{{\"id\": \"{deleted_board}\", \"teamId\": \
             \"{workspace}\", \"title\": \"gone\"}}' WHERE id = '{deleted_board}'; \
             INSERT INTO uploads (id, user_id, board_id, name, size, content_type, part_size, \
             create_at, expires_at, closed_at, multipart_id, purpose) VALUES \
             ('an-import', '{admin}', '{workspace}', 'archive.zip', 1, '', 1, 1, 9000000000000, 0, \
             '', 'import'); \
             DELETE FROM data_steps;"
        ),
    )
    .await;
    // The changes of the data that earlier migrations left to the server are taken as
    // done (their rows removed), so that what changes below is this migration's alone.
    let old = dump(&database).await;
    assert!(
        old["teams"].1.len() >= 2 && !old["deleted_boards"].1.is_empty(),
        "the old database has rows: {old:?}"
    );

    let store = Store::connect(database.db_type(), database.dbconfig())
        .await
        .expect("the store migrates the database");
    let mut new = dump(&database).await;
    // A table a later migration makes is there, empty.
    let audit_log = new
        .remove("audit_log")
        .expect("the audit log of migration 19");
    assert!(audit_log.1.is_empty(), "{audit_log:?}");
    the_keys_and_the_check_of_an_upload_find_the_workspaces(&store, &database, workspace).await;
    store.close().await;

    let expected: Dump = old
        .into_iter()
        .map(|(table, (columns, rows))| {
            let columns: Vec<String> = columns.iter().map(|column| renamed(column)).collect();
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
        if table == "deleted_boards" {
            let board = columns
                .iter()
                .position(|column| column == "board")
                .expect("board");
            let read = |rows: &Vec<Vec<Option<String>>>| -> Vec<Value> {
                let mut boards: Vec<Value> = rows
                    .iter()
                    .map(|row| board_with_the_new_name(row[board].as_deref().unwrap_or("null")))
                    .collect();
                boards.sort_by_key(ToString::to_string);
                boards
            };
            assert_eq!(
                read(new_rows),
                read(rows),
                "{table}: the boards, the member renamed"
            );
            let strip = |rows: &Vec<Vec<Option<String>>>| -> Vec<Vec<Option<String>>> {
                sorted(
                    rows.iter()
                        .map(|row| {
                            let mut row = row.clone();
                            row[board] = None;
                            row
                        })
                        .collect(),
                )
            };
            assert_eq!(strip(new_rows), strip(rows), "{table}: the other columns");
            assert!(
                new_rows
                    .iter()
                    .all(|row| !row[board].as_deref().unwrap_or("").contains("teamId")),
                "no board in the trash names a team: {new_rows:?}"
            );
        } else {
            assert_eq!(new_rows, rows, "{table}: the same rows");
        }
    }
    assert_eq!(names_of_a_team(&database).await, Vec::<String>::new());

    database.remove().await;
}

/// The keys that named the teams name the workspaces: on SQLite no row refers to
/// nothing; on PostgreSQL the check of an upload's target reads the renamed table, and
/// takes a workspace of the organization and refuses a board in its place.
async fn the_keys_and_the_check_of_an_upload_find_the_workspaces(
    store: &Store,
    database: &fvoci_store::testing::TestDatabase,
    workspace_id: &str,
) {
    match db_type() {
        DbType::Sqlite => {
            let path = common::raw_dbconfig(database);
            let mut conn = sqlx::sqlite::SqliteConnection::connect(&path)
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
            let org_id = store
                .only_organization(&Workspace::default())
                .await
                .expect("the organization");
            let ours = store.for_org(&org_id).expect("an identifier");
            let upload = |target: &str| Upload {
                id: new_id(),
                user_id: String::new(),
                purpose: UploadPurpose::Import,
                workspace_id: target.to_owned(),
                name: "archive.zip".to_owned(),
                size: 1,
                part_size: 1,
                create_at: 1,
                expires_at: i64::MAX,
                ..Upload::default()
            };
            let admin: String = sqlx::query_scalar("SELECT id::text FROM users WHERE is_admin")
                .fetch_one(&mut common::connect_past_the_store(database.dbconfig()).await)
                .await
                .expect("the administrator");
            let into = |target: &str| Upload {
                user_id: admin.clone(),
                ..upload(target)
            };
            ours.create_upload(&into(workspace_id))
                .await
                .expect("an import into a workspace of the organization");
            let board: String =
                sqlx::query_scalar("SELECT id::text FROM boards WHERE NOT is_template")
                    .fetch_one(&mut common::connect_past_the_store(database.dbconfig()).await)
                    .await
                    .expect("a board");
            ours.create_upload(&into(&board))
                .await
                .expect_err("a board is no workspace to import into");
        }
    }
}
