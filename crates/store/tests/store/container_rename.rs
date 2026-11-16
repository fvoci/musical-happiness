//! The migration that names the containers (41): a database a server left with the
//! tables `boards`, `board_members`, `board_teams`, `category_boards` and
//! `deleted_boards` and the columns `board_id` keeps every row, under the new names, and
//! no name of the schema speaks of a board any more.

use fvoci_model::new_id;
use fvoci_store::{DbType, Store};
use serde_json::Value;
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
/// outbox and in the audit log says so, and what the trash keeps as JSON names a
/// block's and a member's container `containerId` and a file's `container_id`.
fn with_the_new_values(
    table: &str,
    columns: &[String],
    mut row: Vec<Option<String>>,
) -> Vec<Option<String>> {
    let column = |name: &str| columns.iter().position(|column| column == name);
    if table == "search_outbox"
        && let Some(scope) = column("scope")
        && row[scope].as_deref() == Some("board")
    {
        row[scope] = Some("container".to_owned());
    }
    if table == "audit_log"
        && let Some(action) = column("action")
    {
        let new = match row[action].as_deref() {
            Some("self_added_to_board") => Some("self_added_to_container"),
            Some("added_to_board") => Some("added_to_container"),
            _ => None,
        };
        if let Some(new) = new {
            row[action] = Some(new.to_owned());
        }
    }
    let keys: &[(&str, &str, &str)] = match table {
        "deleted_blocks" => &[("block", "boardId", "containerId")],
        "deleted_boards" => &[
            ("members", "boardId", "containerId"),
            ("files", "board_id", "container_id"),
        ],
        _ => &[],
    };
    for &(name, from, to) in keys {
        if let Some(index) = column(name)
            && let Some(text) = &row[index]
        {
            let mut value: Value = serde_json::from_str(text).expect("the trash keeps JSON");
            let objects: Vec<&mut Value> = if let Value::Array(items) = &mut value {
                items.iter_mut().collect()
            } else {
                vec![&mut value]
            };
            for object in objects {
                if let Some(object) = object.as_object_mut()
                    && let Some(id) = object.remove(from)
                {
                    object.insert(to.to_owned(), id);
                }
            }
            row[index] = Some(value.to_string());
        }
    }
    row
}

/// The JSON a column of the trash keeps, as it is compared: the migration writes it
/// again in its own layout (SQLite puts a renamed member last), so the members of every
/// object are in the order of their names; a list keeps its own order.
fn comparable(
    table: &str,
    columns: &[String],
    mut row: Vec<Option<String>>,
) -> Vec<Option<String>> {
    let json: &[&str] = match table {
        "deleted_blocks" => &["block"],
        "deleted_containers" => &["container", "members", "files", "places", "team_roles"],
        _ => &[],
    };
    for (index, column) in columns.iter().enumerate() {
        if json.contains(&column.as_str())
            && let Some(text) = &row[index]
        {
            let value: Value = serde_json::from_str(text).expect("the trash keeps JSON");
            row[index] = Some(in_order(&value));
        }
    }
    row
}

/// `value` as text, with the members of every object in the order of their names.
fn in_order(value: &Value) -> String {
    match value {
        Value::Object(members) => {
            let mut members: Vec<(&String, &Value)> = members.iter().collect();
            members.sort_by_key(|(name, _)| *name);
            let members: Vec<String> = members
                .into_iter()
                .map(|(name, value)| format!("{}:{}", Value::from(name.as_str()), in_order(value)))
                .collect();
            format!("{{{}}}", members.join(","))
        }
        Value::Array(items) => {
            let items: Vec<String> = items.iter().map(in_order).collect();
            format!("[{}]", items.join(","))
        }
        other => other.to_string(),
    }
}

/// The names of the schema that still speak of a board: tables, columns, indexes,
/// constraints, policies, triggers and functions. On PostgreSQL the bodies of the
/// functions too, but for what they say to whom they refuse; and not the columns of
/// the index of a key, which keep the names they were made with (no statement reads
/// them there: the definition of an index reads the table's columns).
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
                 AND NOT EXISTS (SELECT 1 FROM pg_constraint AS k WHERE k.conindid = c.oid) \
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

/// Every index of the schema: its table, its name, whether it is unique or partial, and
/// its columns (SQLite), or its definition (PostgreSQL); sorted.
async fn indexes(database: &fvoci_store::testing::TestDatabase) -> Vec<String> {
    let dbconfig = common::raw_dbconfig(database);
    let mut indexes: Vec<String> = match db_type() {
        DbType::Sqlite => {
            let mut conn = sqlx::sqlite::SqliteConnection::connect(&dbconfig)
                .await
                .expect("the database file opens");
            let indexes = sqlx::query_scalar(
                "SELECT m.name || ' ' || l.name || ' ' || l.\"unique\" || ' ' || l.partial || ' ' \
                 || (SELECT group_concat(i.name, ',') FROM pragma_index_info(l.name) AS i) \
                 FROM sqlite_master AS m, pragma_index_list(m.name) AS l WHERE m.type = 'table'",
            )
            .fetch_all(&mut conn)
            .await
            .expect("the indexes are read");
            conn.close().await.expect("the connection closes");
            indexes
        }
        DbType::Postgres => {
            let mut conn = common::connect_past_the_store(&dbconfig).await;
            let indexes = sqlx::query_scalar(
                "SELECT pg_get_indexdef(i.indexrelid) FROM pg_index AS i \
                 JOIN pg_class AS c ON c.oid = i.indrelid \
                 WHERE c.relnamespace = current_schema()::regnamespace",
            )
            .fetch_all(&mut conn)
            .await
            .expect("the indexes are read");
            conn.close().await.expect("the connection closes");
            indexes
        }
    };
    indexes.sort();
    indexes
}

/// The rows the tables made after the organizations have of a board: a team's role on
/// it, an invitation of a guest onto it, a link of a body on it, the search's note of
/// it, the roles an administrator held on it before joining the workspace as the
/// organization's administrator, and the records of two people put on it; and the
/// trash's JSON as the server wrote it, two members' and two files' board and a block's
/// after its fields, which hold a `\u0000` (PostgreSQL's `json` keeps it, `jsonb`
/// refuses it) and a member of their own named `boardId`, which is not the block's.
fn rows_of_the_later_tables(before: &Before, team: &str, invitation: &str) -> String {
    let board = before.id("board");
    let card = before.id("card");
    let workspace = before.id("older_workspace");
    let admin = before.id("admin");
    let member = before.id("member");
    let deleted = before.id("deleted_board");
    let deleted_card = before.id("deleted_card");
    let (added, joined) = (new_id(), new_id());
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
         INSERT INTO audit_log (id, org_id, workspace_id, actor_id, actor_kind, action, \
         target_id, subject_id, create_at) VALUES \
         ('{added}', {org}, '{workspace}', '{admin}', 'person', 'added_to_board', '{board}', \
         '{member}', 1), \
         ('{joined}', {org}, '{workspace}', '{admin}', 'person', 'self_added_to_board', \
         '{board}', NULL, 2); \
         UPDATE deleted_boards SET \
         members = '[{{\"boardId\":\"{deleted}\",\"userId\":\"{admin}\",\"schemeAdmin\":true}}, \
         {{\"boardId\":\"{deleted}\",\"userId\":\"{member}\",\"schemeViewer\":true}}]', \
         files = '[{{\"id\":\"f\",\"board_id\":\"{deleted}\",\"stored_name\":\"s\"}}, \
         {{\"id\":\"g\",\"board_id\":\"{deleted}\",\"stored_name\":\"t\"}}]' \
         WHERE id = '{deleted}'; \
         UPDATE deleted_blocks SET \
         block = '{{\"type\":\"card\",\"fields\":{{\"note\":\"a\\u0000b\",\"boardId\":\"kept\"}}, \
         \"boardId\":\"{board}\"}}' \
         WHERE id = '{deleted_card}'; \
         DELETE FROM data_steps;"
    )
}

/// A database that a server left before the containers were named so, with rows in
/// every table that has a board's name or a column of it (those of the database before
/// the organizations carried through their migrations, and those of the tables made
/// since): after the migration every table has the rows it had, under the new names;
/// the search's note of a whole board and the audit's records say 'container', and the
/// trash's JSON names the containers as the model does. Every index is there as it
/// was, the keys still hold, and the checks of a row's container find the containers.
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
    let old_indexes = indexes(&database).await;
    for table in [
        "boards",
        "board_members",
        "board_teams",
        "category_boards",
        "deleted_boards",
        "invitations",
        "body_links",
        "search_outbox",
        "audit_log",
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
    let mut renamed_indexes: Vec<String> = old_indexes
        .iter()
        .map(|index| index.replace("board", "container"))
        .collect();
    renamed_indexes.sort();
    assert_eq!(
        indexes(&database).await,
        renamed_indexes,
        "every index is there as it was, under the new names"
    );

    let expected: Dump = old
        .into_iter()
        .map(|(table, (columns, rows))| {
            let new_table = renamed(&table);
            let new_columns: Vec<String> = columns.iter().map(|column| renamed(column)).collect();
            let mut rows: Vec<_> = rows
                .into_iter()
                .map(|row| with_the_new_values(&table, &columns, row))
                .map(|row| comparable(&new_table, &new_columns, row))
                .collect();
            rows.sort();
            (new_table, (new_columns, rows))
        })
        .collect();
    assert_eq!(
        new.keys().collect::<Vec<_>>(),
        expected.keys().collect::<Vec<_>>(),
        "the same tables, under the new names"
    );
    for (table, (columns, rows)) in &expected {
        let (new_columns, new_rows) = &new[table];
        let mut new_rows: Vec<_> = new_rows
            .iter()
            .map(|row| comparable(table, new_columns, row.clone()))
            .collect();
        new_rows.sort();
        assert_eq!(
            new_columns, columns,
            "{table}: the same columns, in their order"
        );
        assert_eq!(&new_rows, rows, "{table}: the same rows");
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
            let upload = |container_id: &str| {
                format!(
                    "INSERT INTO uploads (id, user_id, container_id, name, size, content_type, \
                     part_size, create_at, expires_at, closed_at, multipart_id, org_id) \
                     SELECT '{}', u.id, '{container_id}', 'a.png', 1, 'image/png', 1, 1, \
                     9000000000000, 0, '', c.org_id FROM users AS u, containers AS c \
                     WHERE c.id = '{container}' LIMIT 1",
                    new_id()
                )
            };
            for (statement, what) in [
                (filing(&container), "a filing of a container"),
                (upload(&container), "an upload to a container"),
            ] {
                let stored = common::try_execute_raw(&raw, &statement).await;
                assert!(stored.is_ok(), "{what} is stored: {stored:?}");
            }
            for (statement, what) in [
                (filing(&new_id()), "a filing of no container"),
                (upload(&new_id()), "an upload to no container"),
            ] {
                let refused = common::try_execute_raw(&raw, &statement).await;
                assert!(
                    refused
                        .as_ref()
                        .is_err_and(common::is_foreign_key_violation),
                    "{what} is refused: {refused:?}"
                );
            }
        }
    }
}
