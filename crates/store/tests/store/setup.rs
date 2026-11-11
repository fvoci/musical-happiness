//! Connecting, schema migration and the databases that are refused: what
//! `Store::connect` promises. And what the schema it creates promises: the same tables
//! on both databases, with foreign keys that are enforced.

use std::path::Path;

use fvoci_model::{
    Block, BlockType, BoardType, BodySummary, Category, CategoryType, FileInfo, new_id,
};
use fvoci_store::{BlockFilter, DbType, Store, StoreError};
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
/// since the store does not look for the board first. The store reads that refusal as
/// "the board is not found".
fn record_for_no_board(stored_name: String) -> FileInfo {
    FileInfo {
        id: new_id(),
        board_id: new_id(),
        stored_name,
        ..FileInfo::default()
    }
}

/// A row that refers to something that does not exist.
#[derive(Debug, Clone, Copy)]
enum Orphan {
    /// A file record for a board that does not exist.
    FileRecord,
    /// A category of a user who does not exist.
    Category,
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
                (Orphan::FileRecord, store.save_file_info(&record).await)
            } else {
                let category = Category {
                    id: new_id(),
                    name: format!("Category {index}"),
                    user_id: new_id(),
                    team_id,
                    ..Category::default()
                };
                (Orphan::Category, store.create_category(&category).await)
            }
        });
    }
    let mut refused = 0;
    while let Some(write) = writes.join_next().await {
        let (orphan, outcome) = write.expect("the write ran to its end");
        let error = outcome.expect_err("a row that refers to nothing is refused");
        // The store hands on the refusal of a category as the database gave it, and
        // reads the refusal of a file record as "no such board".
        let over_the_foreign_key = match orphan {
            Orphan::FileRecord => matches!(&error, StoreError::NotFound(what) if what == "board"),
            Orphan::Category => common::refused_over_a_foreign_key(&error),
        };
        assert!(
            over_the_foreign_key,
            "{orphan:?}: refused over the foreign key: got {error:?}"
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
        matches!(&error, StoreError::NotFound(what) if what == "board"),
        "refused over the foreign key, which reads as no such board: got {error:?}"
    );

    store.close().await;
}

/// Whether the database refused a statement because a row would break a `CHECK` of its
/// table.
fn is_check_violation(error: &sqlx::Error) -> bool {
    error
        .as_database_error()
        .is_some_and(sqlx::error::DatabaseError::is_check_violation)
}

/// Sets one column of one row past the store, and returns how many rows changed or the
/// database's refusal.
async fn set_column(
    test: &common::TestStore,
    table: &str,
    column: &str,
    value: &str,
    id: &str,
) -> Result<u64, sqlx::Error> {
    common::try_execute_raw(
        &test.dbconfig(),
        &format!("UPDATE {table} SET {column} = '{value}' WHERE id = '{id}'"),
    )
    .await
}

/// The schema keeps what the model promises: a board is open (`O`) or private (`P`),
/// whoever writes the row. Anything else is refused, in whatever letter case.
#[tokio::test]
async fn the_schema_refuses_a_board_type_that_is_none() {
    let test = common::setup().await;
    let user_id = common::user(&test, "owner").await;
    let team_id = common::team(&test).await;
    let boards = common::create_test_boards(&test, &team_id, &user_id, 1).await;
    let board_id = &boards[0].id;

    for board_type in ["", " ", "X", "o", "p", "OP", "open", "private", "O "] {
        let error = set_column(&test, "boards", "type", board_type, board_id)
            .await
            .expect_err(&format!("the board type {board_type:?} is refused"));
        assert!(
            is_check_violation(&error),
            "{board_type:?}: refused by the check: got {error:?}"
        );
        let board = test.get_board(board_id).await.expect("the board is read");
        assert_eq!(
            board.board_type,
            BoardType::Open,
            "{board_type:?}: the board is as it was"
        );
    }
    for board_type in [BoardType::Private, BoardType::Open] {
        let changed = set_column(&test, "boards", "type", board_type.as_str(), board_id)
            .await
            .expect("a board type that is one is stored");
        assert_eq!(changed, 1, "{board_type:?}: the one row of the board");
        let board = test.get_board(board_id).await.expect("the board is read");
        assert_eq!(board.board_type, board_type, "as written");
    }

    // A row that is new is checked as one that is changed: the same row is stored with
    // a type that is one, and refused with one that is none.
    let new_board = |board_type: &str| {
        format!(
            "INSERT INTO boards (id, team_id, type, title, description, icon, \
             show_description, is_template, properties, card_properties, create_at, \
             update_at) VALUES ('{}', '{team_id}', '{board_type}', '', '', '', FALSE, FALSE, \
             '{{}}', '[]', 1, 1)",
            new_id()
        )
    };
    let error = common::try_execute_raw(&test.dbconfig(), &new_board("X"))
        .await
        .expect_err("a new board of a type that is none is refused");
    assert!(is_check_violation(&error), "got {error:?}");
    let stored = common::try_execute_raw(&test.dbconfig(), &new_board("P"))
        .await
        .expect("a new board of a type that is one is stored");
    assert_eq!(stored, 1, "the one row");

    test.teardown().await;
}

/// The schema keeps what the model promises: a category is one the server made
/// (`system`) or one of the user's own (`custom`), whoever writes the row. Anything
/// else is refused, the empty type too.
#[tokio::test]
async fn the_schema_refuses_a_category_type_that_is_none() {
    let test = common::setup().await;
    let user_id = common::user(&test, "owner").await;
    let team_id = common::team(&test).await;
    let category = Category {
        id: new_id(),
        name: "Category".to_owned(),
        user_id: user_id.clone(),
        team_id: team_id.clone(),
        ..Category::default()
    };
    test.create_category(&category)
        .await
        .expect("the category is stored");

    for category_type in ["", " ", "odd", "System", "CUSTOM", "customs", "system "] {
        let error = set_column(&test, "categories", "type", category_type, &category.id)
            .await
            .expect_err(&format!("the category type {category_type:?} is refused"));
        assert!(
            is_check_violation(&error),
            "{category_type:?}: refused by the check: got {error:?}"
        );
        let stored = test
            .get_category(&category.id)
            .await
            .expect("the category is read");
        assert_eq!(
            stored.category_type,
            CategoryType::Custom,
            "{category_type:?}: the category is as it was"
        );
    }
    for category_type in [CategoryType::System, CategoryType::Custom] {
        let changed = set_column(
            &test,
            "categories",
            "type",
            category_type.as_str(),
            &category.id,
        )
        .await
        .expect("a category type that is one is stored");
        assert_eq!(changed, 1, "{category_type:?}: the one row of the category");
        let stored = test
            .get_category(&category.id)
            .await
            .expect("the category is read");
        assert_eq!(stored.category_type, category_type, "as written");
    }

    // A row that is new is checked as one that is changed: the same row is stored with
    // a type that is one, and refused with one that is none.
    let new_category = |category_type: &str| {
        format!(
            "INSERT INTO categories (id, name, user_id, team_id, create_at, update_at, type) \
             VALUES ('{}', 'New', '{user_id}', '{team_id}', 1, 1, '{category_type}')",
            new_id()
        )
    };
    let error = common::try_execute_raw(&test.dbconfig(), &new_category("odd"))
        .await
        .expect_err("a new category of a type that is none is refused");
    assert!(is_check_violation(&error), "got {error:?}");
    let stored = common::try_execute_raw(&test.dbconfig(), &new_category("system"))
        .await
        .expect("a new category of a type that is one is stored");
    assert_eq!(stored, 1, "the one row");

    test.teardown().await;
}

/// The column types that only PostgreSQL has. Where its baseline names one of them,
/// SQLite's says `TEXT`.
const POSTGRES_ONLY_TYPES: [&str; 3] = ["UUID", "JSONB", "JSON"];

/// Reads the baseline schema of a database kind.
fn baseline_schema(directory: &str) -> String {
    schema_file(directory, "0001_baseline.sql")
}

/// Reads a migration of a database kind.
fn schema_file(directory: &str, name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("schema")
        .join(directory)
        .join(name);
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

/// The two migrations that add the body describe one change, as the baselines do.
#[test]
fn the_two_body_migrations_differ_in_the_column_types_only() {
    let postgres = schema_file("postgres", "0002_body.sql");
    let sqlite = schema_file("sqlite", "0002_body.sql");
    assert_eq!(
        statement_lines(&postgres).len(),
        5,
        "five columns: {postgres}"
    );
    assert_eq!(
        schema_difference(&postgres, &sqlite),
        None,
        "the body migrations of the two databases add different columns"
    );
}

/// A database of the baseline alone, with blocks in it, takes the body migration: the
/// blocks have no body and nothing to count.
#[tokio::test]
async fn blocks_stored_before_the_body_migration_have_no_body() {
    let test = common::setup().await;
    let user_id = common::user(&test, "migrated").await;
    let team_id = common::team(&test).await;
    let boards = common::create_test_boards(&test, &team_id, &user_id, 1).await;
    let mut card = Block {
        id: new_id(),
        board_id: boards[0].id.clone(),
        block_type: BlockType::Card,
        title: "from before".to_owned(),
        ..Block::default()
    };
    test.insert_block(&mut card, &user_id)
        .await
        .expect("the card is stored");
    test.store.close().await;
    // Back to the baseline, as a database the previous server left behind.
    for statement in [
        "ALTER TABLE blocks DROP COLUMN body",
        "ALTER TABLE blocks DROP COLUMN body_version",
        "ALTER TABLE blocks DROP COLUMN body_text",
        "ALTER TABLE blocks DROP COLUMN body_tasks_total",
        "ALTER TABLE blocks DROP COLUMN body_tasks_done",
        "DELETE FROM _sqlx_migrations WHERE version = 2",
    ] {
        common::execute_raw(&test.dbconfig(), statement).await;
    }

    let store = Store::connect(common::db_type(), &test.dbconfig())
        .await
        .expect("the store migrates the database");
    let read = store.get_block(&card.id).await.expect("the card is read");
    assert_eq!(read.body, None, "{read:?}");
    assert_eq!(read.title, "from before");
    let listed = store
        .get_listed_blocks(
            &BlockFilter::board(&boards[0].id),
            fvoci_store::Previews::Without,
        )
        .await
        .expect("the cards are listed");
    assert_eq!(
        listed,
        [fvoci_store::ListedBlock {
            block: read,
            summary: BodySummary::default(),
            last_comment_at: 0,
            preview: None,
        }],
        "no body, no tasks and no comments"
    );
    store.close().await;

    test.teardown().await;
}

/// Two attachment blocks that name their file by `fileId`, one by `attachmentId`, and
/// two cards whose bodies show an image deep in the document; the second and the fourth
/// go to the trash.
fn blocks_showing(board_id: &str) -> [Block; 5] {
    let image = |name: &str| Block {
        id: new_id(),
        board_id: board_id.to_owned(),
        block_type: BlockType::Attachment,
        fields: serde_json::from_value(serde_json::json!({ "fileId": name }))
            .expect("the fields are a JSON object"),
        ..Block::default()
    };
    let card = |name: &str| Block {
        id: new_id(),
        board_id: board_id.to_owned(),
        block_type: BlockType::Card,
        body: fvoci_model::Body::parse(serde_json::json!({"type": "doc", "content": [
            {"type": "paragraph", "content": [{"type": "text", "text": "x"}]},
            // Deep in the document, as the back-fill must find it.
            {"type": "bulletList", "content": [{"type": "listItem", "content": [
                {"type": "paragraph", "content": [{"type": "text", "text": "y"}]},
                {"type": "blockquote", "content": [
                    {"type": "image", "attrs": {"fileId": name}},
                ]},
            ]}]},
        ]}))
        .expect("the body may be stored"),
        ..Block::default()
    };
    [
        image("7image.png"),
        image("7trash.png"),
        card("7body.png"),
        card("7trashbody.png"),
        Block {
            id: new_id(),
            board_id: board_id.to_owned(),
            block_type: BlockType::Attachment,
            fields: serde_json::from_value(
                serde_json::json!({ "attachmentId": "7attachment.png" }),
            )
            .expect("the fields are a JSON object"),
            ..Block::default()
        },
    ]
}

/// Writes, as the server before migration 4 stored them, date properties on every
/// board, and the values of cards and the filters of views among the blocks: a date in
/// the previous shape, one that cannot be read, and their look-alikes in a text
/// property.
async fn store_previous_shapes(dbconfig: &str, blocks: &[Block]) {
    let definitions = r#"[{"id": "due", "type": "date"}, {"id": "bad", "type": "date"}, {"id": "note", "type": "text"}]"#;
    let card_fields = r#"{"properties": {"due": "{\"from\":1772679600000}", "note": "{\"from\":1}", "bad": "x"}}"#;
    let view_fields = r#"{"filter": {"operation": "and", "filters": [{"propertyId": "due", "condition": "is", "values": ["1772679600000"]}, {"operation": "or", "filters": [{"propertyId": "note", "condition": "is", "values": ["1"]}]}]}}"#;
    let mut statements = vec![format!(
        "UPDATE boards SET card_properties = '{definitions}'"
    )];
    for (block, fields) in blocks.iter().zip([
        card_fields,
        view_fields,
        card_fields,
        view_fields,
        card_fields,
    ]) {
        statements.push(format!(
            "UPDATE blocks SET fields = '{fields}' WHERE id = '{}'",
            block.id
        ));
    }
    for statement in &statements {
        common::execute_raw(dbconfig, statement).await;
    }
}

/// The data step of migration 4 turns stored date values of the previous shape into
/// those of `docs/s6-design.md` 5.7: a card's value of a date property and a view's
/// date filter, live, in the trash, and on a board in the trash. Other properties and
/// filters stay as they were.
#[tokio::test]
async fn the_date_value_step_rewrites_what_was_stored_before() {
    let test = common::setup().await;
    let user_id = common::user(&test, "migrated").await;
    let team_id = common::team(&test).await;
    let boards = common::create_test_boards(&test, &team_id, &user_id, 2).await;
    let (board_id, trashed_board) = (&boards[0].id, &boards[1].id);
    let block = |board_id: &str, block_type| Block {
        id: new_id(),
        board_id: board_id.to_owned(),
        block_type,
        ..Block::default()
    };
    // A card and a view that stay, a card and a view that go to the trash, and a card
    // of the board that goes to the trash.
    let mut blocks = [
        block(board_id, BlockType::Card),
        block(board_id, BlockType::View),
        block(board_id, BlockType::Card),
        block(board_id, BlockType::View),
        block(trashed_board, BlockType::Card),
    ];
    common::insert_blocks(&test, &mut blocks, &user_id).await;
    test.store.close().await;
    store_previous_shapes(&test.dbconfig(), &blocks).await;
    let store = Store::connect(common::db_type(), &test.dbconfig())
        .await
        .expect("the store starts");
    for in_trash in [&blocks[2], &blocks[3]] {
        store
            .delete_block(&in_trash.id, &user_id)
            .await
            .expect("a block goes to the trash");
    }
    store
        .delete_board(trashed_board, &user_id)
        .await
        .expect("a board goes to the trash");
    store.close().await;
    common::execute_raw(
        &test.dbconfig(),
        "INSERT INTO data_steps (name) VALUES ('date_values')",
    )
    .await;

    let store = Store::connect(common::db_type(), &test.dbconfig())
        .await
        .expect("the store does the step");
    store
        .undelete_board(trashed_board, &user_id)
        .await
        .expect("the board comes back");
    for in_trash in [&blocks[2], &blocks[3]] {
        store
            .undelete_block(&in_trash.id, &user_id)
            .await
            .expect("a block comes back");
    }
    let expected = serde_json::json!({
        "due": {"from": "2026-03-05"}, "note": "{\"from\":1}", "bad": null,
    });
    for card in [&blocks[0], &blocks[2], &blocks[4]] {
        let card = store.get_block(&card.id).await.expect("the card");
        assert_eq!(card.fields["properties"], expected, "{card:?}");
    }
    for view in [&blocks[1], &blocks[3]] {
        let view = store.get_block(&view.id).await.expect("the view");
        let filters = &view.fields["filter"]["filters"];
        assert_eq!(
            filters[0]["values"],
            serde_json::json!(["2026-03-05"]),
            "{view:?}"
        );
        assert_eq!(
            filters[1]["filters"][0]["values"],
            serde_json::json!(["1"]),
            "a filter of another property"
        );
    }
    store.close().await;

    // Done once: the step's row is gone, and the server starts without it.
    let store = Store::connect(common::db_type(), &test.dbconfig())
        .await
        .expect("the store starts again");
    store.close().await;
    let left = common::try_execute_raw(&test.dbconfig(), "DELETE FROM data_steps")
        .await
        .expect("the table is there");
    assert_eq!(left, 0, "no step is left");

    test.teardown().await;
}

/// The migration of file references puts down what the blocks show, live and in the
/// trash, and counts a file that none shows from the migration: it is collected only
/// once the grace period has passed from then.
#[tokio::test]
async fn the_file_reference_migration_finds_what_the_blocks_show() {
    let test = common::setup().await;
    let user_id = common::user(&test, "migrated").await;
    let team_id = common::team(&test).await;
    let boards = common::create_test_boards(&test, &team_id, &user_id, 1).await;
    let board_id = &boards[0].id;
    for name in [
        "7image.png",
        "7body.png",
        "7trash.png",
        "7trashbody.png",
        "7attachment.png",
        "7none.png",
    ] {
        let file = fvoci_model::FileInfo {
            id: new_id(),
            board_id: board_id.clone(),
            stored_name: name.to_owned(),
            create_at: 1,
            path: format!("{board_id}/{name}"),
            ..fvoci_model::FileInfo::default()
        };
        test.save_file_info(&file).await.expect("a file is stored");
    }
    let mut blocks = blocks_showing(board_id);
    common::insert_blocks(&test, &mut blocks, &user_id).await;
    for in_trash in [&blocks[1], &blocks[3]] {
        test.delete_block(&in_trash.id, &user_id)
            .await
            .expect("an image and a card go to the trash");
    }
    test.store.close().await;
    // Back to before the migration.
    for statement in [
        "DROP TABLE file_refs",
        "DROP INDEX file_info_unreferenced_at_idx",
        "ALTER TABLE file_info DROP COLUMN unreferenced_at",
        "DELETE FROM _sqlx_migrations WHERE version = 3",
    ] {
        common::execute_raw(&test.dbconfig(), statement).await;
    }
    let migrated_at = crate::blocks::now_millis();

    let store = Store::connect(common::db_type(), &test.dbconfig())
        .await
        .expect("the store migrates the database");
    let collected = |before: i64| {
        let store = &store;
        async move {
            let mut names: Vec<String> = store
                .collect_unreferenced_files(before)
                .await
                .expect("the files no block shows are collected")
                .into_iter()
                .map(|file| file.stored_name)
                .collect();
            names.sort();
            names
        }
    };
    assert_eq!(
        collected(migrated_at - 1_000).await,
        Vec::<String>::new(),
        "counted from the migration, not from the upload"
    );
    assert_eq!(
        collected(crate::blocks::now_millis() + 3_600_000).await,
        ["7none.png"],
        "the image, the attachment, the bodies and the trash show theirs"
    );
    store.close().await;

    test.teardown().await;
}

/// Stores a record of a file of the board for each name.
async fn save_files(store: &Store, board_id: &str, names: &[&str]) {
    for name in names {
        let file = fvoci_model::FileInfo {
            id: new_id(),
            board_id: board_id.to_owned(),
            stored_name: (*name).to_owned(),
            create_at: 1,
            path: format!("{board_id}/{name}"),
            ..fvoci_model::FileInfo::default()
        };
        store.save_file_info(&file).await.expect("a file is stored");
    }
}

/// The step of migration 8 deletes the content blocks an older server stored, with what
/// is below them, on the board and in the trash, and leaves the files that only they
/// showed to be collected. The other blocks and their files stay.
#[tokio::test]
async fn the_content_block_step_deletes_them_and_leaves_their_files_to_collect() {
    let test = common::setup().await;
    let user_id = common::user(&test, "migrated").await;
    let team_id = common::team(&test).await;
    let boards = common::create_test_boards(&test, &team_id, &user_id, 1).await;
    let board_id = &boards[0].id;
    save_files(&test, board_id, &["7image.png", "7kept.png"]).await;
    let block = |block_type: &str, fields: serde_json::Value| Block {
        id: new_id(),
        board_id: board_id.clone(),
        block_type: BlockType::from(block_type),
        fields: serde_json::from_value(fields).expect("the fields are a JSON object"),
        ..Block::default()
    };
    let none = serde_json::json!({});
    // A card, an attachment, a view and a legacy board block that stay; an image, a
    // text, a heading in the trash and a checkbox that go. The image is stored as an
    // attachment first, which puts down its file, and then made an image as an older
    // server stored it.
    let mut blocks = [
        block("card", none.clone()),
        block("attachment", serde_json::json!({"fileId": "7kept.png"})),
        block("view", none.clone()),
        block("board", none.clone()),
        block("attachment", serde_json::json!({"fileId": "7image.png"})),
        block("text", none.clone()),
        block("h1", none.clone()),
        block("checkbox", none.clone()),
    ];
    common::insert_blocks(&test, &mut blocks, &user_id).await;
    // A comment below the text and one below the heading, which takes it to the trash:
    // they go with them.
    let mut below = [
        Block {
            parent_id: blocks[5].id.clone(),
            ..block("comment", none.clone())
        },
        Block {
            parent_id: blocks[6].id.clone(),
            ..block("comment", none)
        },
    ];
    common::insert_blocks(&test, &mut below, &user_id).await;
    test.delete_block(&blocks[6].id, &user_id)
        .await
        .expect("the heading goes to the trash");
    test.store.close().await;
    common::execute_raw(
        &test.dbconfig(),
        &format!(
            "UPDATE blocks SET type = 'image' WHERE id = '{}'",
            blocks[4].id
        ),
    )
    .await;
    common::execute_raw(
        &test.dbconfig(),
        "INSERT INTO data_steps (name) VALUES ('content_blocks')",
    )
    .await;

    let store = Store::connect(common::db_type(), &test.dbconfig())
        .await
        .expect("the store does the step");
    for kept in &blocks[..4] {
        store.get_block(&kept.id).await.expect("the block stays");
    }
    for gone in blocks[4..].iter().chain(&below) {
        let error = store
            .get_block(&gone.id)
            .await
            .expect_err("the content block is gone");
        assert!(error.is_not_found(), "{error:?}");
    }
    let error = store
        .undelete_block(&blocks[6].id, &user_id)
        .await
        .expect_err("nor in the trash");
    assert!(error.is_not_found(), "{error:?}");
    let collected: Vec<String> = store
        .collect_unreferenced_files(crate::blocks::now_millis() + 3_600_000)
        .await
        .expect("the files no block shows are collected")
        .into_iter()
        .map(|file| file.stored_name)
        .collect();
    assert_eq!(
        collected,
        ["7image.png"],
        "the image's file, not the attachment's"
    );
    store.close().await;
    let left = common::try_execute_raw(&test.dbconfig(), "DELETE FROM deleted_blocks")
        .await
        .expect("the trash is there");
    assert_eq!(
        left, 0,
        "the heading and the comment below it left the trash"
    );

    test.teardown().await;
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
