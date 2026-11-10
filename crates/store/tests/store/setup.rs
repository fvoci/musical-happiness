//! Connecting, schema migration and the databases that are refused: what
//! `Store::connect` promises. And what the schema it creates promises: the same tables
//! on both databases, with foreign keys that are enforced.

use std::path::Path;

use fvoci_model::{Block, Category, FileInfo, new_id};
use fvoci_store::{DbType, Store, StoreError};
use tokio::task::JoinSet;

use crate::common;

#[tokio::test]
async fn connect_creates_the_schema_once_and_reopens_it() {
    let test = common::setup().await;
    assert_eq!(test.db_type(), common::db_type());
    let version = test
        .db_version()
        .await
        .expect("the database reports a version");
    assert!(!version.is_empty(), "the version string is not empty");

    // A second store on the same database finds the schema and leaves it alone.
    let again = Store::connect(common::db_type(), &test.dbconfig())
        .await
        .expect("an existing schema at the current version is accepted");
    again.close().await;

    test.teardown().await;
}

/// Connects to a database that holds what `prepare` put there and returns the refusal.
async fn refusal_of_a_database_with(prepare: &str) -> String {
    let database = common::empty_database().await;
    common::execute_raw(&common::raw_dbconfig(&database), prepare).await;

    let error = Store::connect(database.db_type(), database.dbconfig())
        .await
        .expect_err("the database is not this server's to open");
    assert!(matches!(error, StoreError::Schema(_)), "got {error:?}");
    database.remove().await;
    error.to_string()
}

#[tokio::test]
async fn a_database_of_the_original_server_is_refused_with_the_reason() {
    // The bookkeeping table of the original's migration engine: a fixed name on SQLite,
    // the table prefix and `schema_migrations` on PostgreSQL.
    let bookkeeping = match common::db_type() {
        DbType::Sqlite => "db_migrations",
        DbType::Postgres => "boards_schema_migrations",
    };
    let refusal = refusal_of_a_database_with(&format!(
        "CREATE TABLE {bookkeeping} (version BIGINT NOT NULL PRIMARY KEY, name VARCHAR NOT NULL); \
         CREATE TABLE blocks (id VARCHAR(36) PRIMARY KEY);"
    ))
    .await;
    assert!(
        refusal.contains("created by the original server"),
        "the refusal says why: {refusal}"
    );
}

#[tokio::test]
async fn tables_without_a_migration_record_are_refused() {
    let refusal =
        refusal_of_a_database_with("CREATE TABLE blocks (id VARCHAR(36) PRIMARY KEY);").await;
    assert!(
        refusal.contains("no record of schema migrations"),
        "the refusal says why: {refusal}"
    );
}

#[tokio::test]
async fn a_database_migrated_by_a_newer_server_is_refused() {
    let test = common::setup().await;
    test.store.close().await;
    // What a later version of this server leaves behind: a migration this one lacks.
    common::execute_raw(
        &test.dbconfig(),
        "INSERT INTO _sqlx_migrations \
         (version, description, success, checksum, execution_time) \
         SELECT 9999, 'from the future', success, checksum, execution_time \
         FROM _sqlx_migrations WHERE version = 1",
    )
    .await;

    let error = Store::connect(common::db_type(), &test.dbconfig())
        .await
        .expect_err("a database with a migration this server does not know is refused");
    assert!(
        matches!(&error, StoreError::Schema(reason) if reason.contains("newer server")),
        "got {error:?}"
    );

    test.teardown().await;
}

#[tokio::test]
async fn a_changed_migration_history_is_refused() {
    let test = common::setup().await;
    test.store.close().await;
    common::execute_raw(
        &test.dbconfig(),
        "UPDATE _sqlx_migrations SET checksum = checksum || checksum WHERE version = 1",
    )
    .await;

    let error = Store::connect(common::db_type(), &test.dbconfig())
        .await
        .expect_err("a database whose applied migration differs is refused");
    assert!(
        matches!(&error, StoreError::Schema(reason) if reason.contains("differs")),
        "got {error:?}"
    );

    test.teardown().await;
}

#[tokio::test]
async fn the_tables_carry_no_prefix() {
    let test = common::setup().await;
    // A statement that names the tables plainly runs, on either database.
    common::execute_raw(
        &test.dbconfig(),
        "SELECT (SELECT COUNT(*) FROM blocks) + (SELECT COUNT(*) FROM boards) \
         + (SELECT COUNT(*) FROM users) + (SELECT COUNT(*) FROM sessions)",
    )
    .await;
    test.teardown().await;
}

#[tokio::test]
async fn in_memory_sqlite_databases_are_private_to_their_store() {
    let first = Store::connect(
        DbType::Sqlite,
        "file::memory:?cache=shared&_busy_timeout=5000",
    )
    .await
    .expect("an in-memory database");
    let second = Store::connect(DbType::Sqlite, "file::memory:?cache=shared")
        .await
        .expect("another in-memory database");

    // A block of a board of a team, by a user, all in the first store only.
    let user_id = common::user(&first, "only-in-first").await;
    let team_id = common::team(&first).await;
    let boards = common::create_test_boards(&first, &team_id, &user_id, 1).await;
    let mut block = Block {
        id: new_id(),
        board_id: boards[0].id.clone(),
        created_by: user_id.clone(),
        ..Block::default()
    };
    first
        .insert_block(&mut block, &user_id)
        .await
        .expect("inserted");

    assert!(
        first.get_block(&block.id).await.is_ok(),
        "the first store has the block"
    );
    let missing = second
        .get_block(&block.id)
        .await
        .expect_err("not in the second store");
    assert!(missing.is_not_found(), "got {missing:?}");
    let missing = second
        .get_user_by_id(&user_id)
        .await
        .expect_err("not in the second store");
    assert!(missing.is_not_found(), "got {missing:?}");

    first.close().await;
    second.close().await;
}

/// How many writes [`foreign_keys_are_enforced_on_the_connections_of_the_store`] sends
/// at once: several times the connections a store keeps, so that each of them is used.
const WRITES_AT_ONCE: usize = 40;

/// A file record for a board that does not exist: a row only the database can refuse,
/// since the store does not look for the board first.
fn record_for_no_board(stored_name: String) -> FileInfo {
    FileInfo {
        id: new_id(),
        board_id: new_id(),
        stored_name,
        ..FileInfo::default()
    }
}

/// The schema's foreign keys are enforced on the connections the store uses, each of
/// them. SQLite enforces them only on a connection that asks for it, so one that did
/// not would store what the others refuse.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn foreign_keys_are_enforced_on_the_connections_of_the_store() {
    let test = common::setup().await;
    let store = &test.store;
    let user_id = common::user(store, "user").await;
    let team_id = common::team(store).await;

    // Rows that refer to nothing, by a single statement and by a transaction: a file
    // record for a board that does not exist, a category of a user who does not.
    let mut writes = JoinSet::new();
    for index in 0..WRITES_AT_ONCE {
        let store = test.store.clone();
        let team_id = team_id.clone();
        writes.spawn(async move {
            if index % 2 == 0 {
                let record = record_for_no_board(format!("7file{index}.png"));
                store.save_file_info(&record).await
            } else {
                let category = Category {
                    id: new_id(),
                    name: format!("Category {index}"),
                    user_id: new_id(),
                    team_id,
                    ..Category::default()
                };
                store.create_category(&category).await
            }
        });
    }
    let mut refused = 0;
    while let Some(write) = writes.join_next().await {
        let error = write
            .expect("the write ran to its end")
            .expect_err("a row that refers to nothing is refused");
        assert!(
            common::refused_over_a_foreign_key(&error),
            "refused over the foreign key: got {error:?}"
        );
        refused += 1;
    }
    assert_eq!(refused, WRITES_AT_ONCE, "every write was refused");

    // The same rows are stored once what they refer to exists.
    let boards = common::create_test_boards(store, &team_id, &user_id, 1).await;
    let record = FileInfo {
        board_id: boards[0].id.clone(),
        ..record_for_no_board("7file.png".to_owned())
    };
    store
        .save_file_info(&record)
        .await
        .expect("a file record for a board that exists is stored");
    let category = Category {
        id: new_id(),
        name: "Category".to_owned(),
        user_id: user_id.clone(),
        team_id: team_id.clone(),
        ..Category::default()
    };
    store
        .create_category(&category)
        .await
        .expect("a category of a user who exists is stored");
    let listed = store
        .get_user_categories(&user_id, &team_id)
        .await
        .expect("the categories are listed");
    assert_eq!(
        listed.len(),
        1,
        "the one category that was not refused: {listed:?}"
    );

    test.teardown().await;
}

/// An in-memory SQLite database enforces the foreign keys as a database file does.
#[tokio::test]
async fn foreign_keys_are_enforced_in_an_in_memory_sqlite_database() {
    let store = Store::connect(DbType::Sqlite, ":memory:")
        .await
        .expect("an in-memory database");

    let error = store
        .save_file_info(&record_for_no_board("7file.png".to_owned()))
        .await
        .expect_err("a file record for a board that does not exist is refused");
    assert!(
        common::refused_over_a_foreign_key(&error),
        "refused over the foreign key: got {error:?}"
    );

    store.close().await;
}

/// The column types that only PostgreSQL has. Where its baseline names one of them,
/// SQLite's says `TEXT`.
const POSTGRES_ONLY_TYPES: [&str; 3] = ["UUID", "JSONB", "JSON"];

/// Reads the baseline schema of a database kind.
fn baseline_schema(directory: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("schema")
        .join(directory)
        .join("0001_baseline.sql");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("reading {}: {error}", path.display()))
}

/// The statements of a schema file: its lines without their comments, the blank ones
/// left out, each with the number it has in the file. (No string in the schema holds
/// `--`.)
fn statement_lines(schema: &str) -> Vec<(usize, &str)> {
    schema
        .lines()
        .enumerate()
        .filter_map(|(index, line)| {
            let statement = line
                .split_once("--")
                .map_or(line, |(statement, _comment)| statement)
                .trim_end();
            (!statement.is_empty()).then_some((index + 1, statement))
        })
        .collect()
}

/// The words of a line: its runs of letters, digits and underscores.
fn words(line: &str) -> impl Iterator<Item = &str> {
    line.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .filter(|word| !word.is_empty())
}

/// A line of the PostgreSQL baseline as the SQLite baseline has it: every word that is
/// a PostgreSQL-only type reads `TEXT`, and nothing else differs.
fn with_sqlite_types(line: &str) -> String {
    let is_word = |c: char| c.is_ascii_alphanumeric() || c == '_';
    let mut converted = String::with_capacity(line.len());
    for piece in line.split_inclusive(|c: char| !is_word(c)) {
        let (word, separator) = piece.split_at(piece.trim_end_matches(|c: char| !is_word(c)).len());
        converted.push_str(if POSTGRES_ONLY_TYPES.contains(&word) {
            "TEXT"
        } else {
            word
        });
        converted.push_str(separator);
    }
    converted
}

/// Where two baselines stop describing one schema, if they do: the first statement
/// line of the PostgreSQL one that, with its types as SQLite has them, is not the
/// corresponding line of the SQLite one. Comments are not compared.
fn schema_difference(postgres: &str, sqlite: &str) -> Option<String> {
    let postgres_lines = statement_lines(postgres);
    let sqlite_lines = statement_lines(sqlite);
    for (index, (number, line)) in postgres_lines.iter().enumerate() {
        let Some((sqlite_number, sqlite_line)) = sqlite_lines.get(index) else {
            return Some(format!(
                "PostgreSQL line {number} `{line}` is past the end of the SQLite baseline"
            ));
        };
        let expected = with_sqlite_types(line);
        if expected != *sqlite_line {
            return Some(format!(
                "PostgreSQL line {number} `{line}` should be `{expected}` on SQLite, whose \
                 line {sqlite_number} is `{sqlite_line}`"
            ));
        }
    }
    sqlite_lines
        .get(postgres_lines.len())
        .map(|(number, line)| {
            format!("SQLite line {number} `{line}` is past the end of the PostgreSQL baseline")
        })
}

/// The two baseline files describe one schema: statement for statement the same, but
/// for the column types that only PostgreSQL has, which are `TEXT` on SQLite. A table,
/// column, key, default or index that one file has and the other lacks fails here.
#[test]
fn the_two_baseline_schemas_differ_in_the_column_types_only() {
    let postgres = baseline_schema("postgres");
    let sqlite = baseline_schema("sqlite");
    let postgres_lines = statement_lines(&postgres);
    assert!(
        postgres_lines.len() > 100,
        "the PostgreSQL baseline is read whole: {} lines of statements",
        postgres_lines.len()
    );

    assert_eq!(
        schema_difference(&postgres, &sqlite),
        None,
        "the baselines of the two databases describe different schemas"
    );

    // The rule is no wider than the files: each of the types is in the PostgreSQL
    // baseline, and none of them is in SQLite's.
    let sqlite_lines = statement_lines(&sqlite);
    for postgres_type in POSTGRES_ONLY_TYPES {
        assert!(
            postgres_lines
                .iter()
                .any(|(_, line)| words(line).any(|word| word == postgres_type)),
            "the PostgreSQL baseline has no {postgres_type} column"
        );
        let in_sqlite = sqlite_lines
            .iter()
            .find(|(_, line)| words(line).any(|word| word == postgres_type));
        assert_eq!(
            in_sqlite, None,
            "the SQLite baseline names the PostgreSQL type {postgres_type}"
        );
    }
}

/// The comparison of the baselines sees what it is there to see: it lets the type
/// names and the comments differ, and nothing else.
#[test]
fn the_comparison_of_the_baselines_lets_only_types_and_comments_differ() {
    let postgres = "-- PostgreSQL.\n\
                    CREATE TABLE t (\n    \
                        id UUID NOT NULL PRIMARY KEY, -- an identifier\n    \
                        parent_id UUID,\n    \
                        json_name TEXT NOT NULL DEFAULT '',\n    \
                        fields JSON NOT NULL,\n    \
                        properties JSONB NOT NULL\n\
                    );\n\
                    CREATE INDEX t_parent_id_idx ON t (parent_id);\n";
    let sqlite = "-- SQLite.\n\n\
                  CREATE TABLE t (\n    \
                      id TEXT NOT NULL PRIMARY KEY,\n    \
                      parent_id TEXT,\n    \
                      -- A name, not a type.\n    \
                      json_name TEXT NOT NULL DEFAULT '',\n    \
                      fields TEXT NOT NULL,\n    \
                      properties TEXT NOT NULL\n\
                  );\n\
                  CREATE INDEX t_parent_id_idx ON t (parent_id);\n";
    assert_eq!(
        schema_difference(postgres, sqlite),
        None,
        "types and comments may differ"
    );

    let drifts = [
        (
            "a type outside the rule",
            "id TEXT NOT NULL",
            "id BLOB NOT NULL",
        ),
        (
            "a constraint",
            "parent_id TEXT,",
            "parent_id TEXT NOT NULL,",
        ),
        ("a column", "    fields TEXT NOT NULL,\n", ""),
        (
            "an index",
            "CREATE INDEX t_parent_id_idx ON t (parent_id);\n",
            "",
        ),
        ("a PostgreSQL type kept", "fields TEXT", "fields JSON"),
        (
            "a statement more",
            ");\nCREATE",
            ");\nCREATE TABLE u (id TEXT);\nCREATE",
        ),
    ];
    for (what, from, to) in drifts {
        assert!(sqlite.contains(from), "{what}: the case changes something");
        let drifted = sqlite.replacen(from, to, 1);
        assert_ne!(
            schema_difference(postgres, &drifted),
            None,
            "{what}: a difference in the SQLite baseline is seen"
        );
    }
    let without_index =
        postgres.replacen("CREATE INDEX t_parent_id_idx ON t (parent_id);\n", "", 1);
    assert_ne!(
        schema_difference(&without_index, sqlite),
        None,
        "a statement the PostgreSQL baseline lacks is seen"
    );
}
