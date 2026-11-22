//! The migration of the diaries (55) on a database a server left before it, and what
//! the reads of a full diary cost: both say what they measured where the tests are run
//! with their output shown (`--nocapture`).

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use fvoci_model::{
    Block, BlockType, Container, ContainerKind, ContainerType, DiaryDate, DiaryMaking,
    ENTRIES_PER_DIARY, MonthDay, NewDiaryEntry, new_id,
};
use fvoci_store::{DbType, SharingChange, Store};
use sqlx::Connection as _;

use crate::boards::new_board;
use crate::common::{self, db_type};
use crate::workspace_rename::{Dump, dump};

/// The rows of every table, each as its values by the names of their columns: the
/// order of a table's columns is not what a migration keeps.
fn by_column(dump: &Dump) -> BTreeMap<String, Vec<BTreeMap<String, Option<String>>>> {
    dump.iter()
        .map(|(table, (columns, rows))| {
            let mut rows: Vec<BTreeMap<String, Option<String>>> = rows
                .iter()
                .map(|row| columns.iter().cloned().zip(row.iter().cloned()).collect())
                .collect();
            rows.sort();
            (table.clone(), rows)
        })
        .collect()
}

/// The columns of a table, in their order.
async fn columns_of(system: &Store, table: &str) -> Vec<String> {
    let query = match db_type() {
        DbType::Sqlite => format!("SELECT name FROM pragma_table_info('{table}') ORDER BY cid"),
        DbType::Postgres => format!(
            "SELECT column_name::text FROM information_schema.columns \
             WHERE table_schema = current_schema() AND table_name = '{table}' \
             ORDER BY ordinal_position"
        ),
    };
    system.texts_unchecked(&query).await.expect("read")
}

/// A database of the schema before the diaries, with an organization that has a board
/// and a wiki, each with blocks, members and a link, a template, and a board in the
/// trash; and the identifier of the board. Everything is stored by the store itself,
/// which writes that schema as it writes the one after it.
async fn a_database_from_before() -> (fvoci_store::testing::TestDatabase, String) {
    let database = common::empty_database().await;
    fvoci_store::testing::migrate_to_before_diaries(&database).await;
    // The changes of the data that earlier migrations left to the server are taken as
    // done (their rows removed), so that what changes afterwards is this migration's.
    common::execute_raw(&common::raw_connection(&database), "DELETE FROM data_steps").await;
    let store = Store::open(database.db_type(), database.database_connection())
        .await
        .expect("the database opens");
    let (org_id, workspace_id) = common::organization(&store, "people").await;
    let store = store.for_org(&org_id).expect("an identifier");
    let user_id = common::user(&store, "member").await;
    common::works_in(&store, &workspace_id, &user_id).await;
    let mut kept = String::new();
    for (kind, is_template, trashed) in [
        (ContainerKind::Board, false, false),
        (ContainerKind::Wiki, false, false),
        (ContainerKind::Board, true, false),
        (ContainerKind::Board, false, true),
    ] {
        let mut container = Container {
            kind,
            is_template,
            title: format!("a {} from before", kind.as_str()),
            ..new_board(&new_id(), &workspace_id, ContainerType::Private)
        };
        store
            .insert_container_with_admin(&mut container, &user_id)
            .await
            .expect("stored");
        let block_type = match kind {
            ContainerKind::Wiki => BlockType::Page,
            _ => BlockType::Card,
        };
        for title in ["one", "two"] {
            let mut block = Block {
                id: new_id(),
                container_id: container.id.clone(),
                block_type: block_type.clone(),
                title: title.to_owned(),
                ..Block::default()
            };
            store
                .insert_block(&mut block, &user_id)
                .await
                .expect("stored");
        }
        if trashed {
            store
                .delete_container(&container.id, &user_id)
                .await
                .expect("deleted");
        } else if !is_template {
            store
                .set_sharing(&container.id, SharingChange::On, &new_id(), &user_id)
                .await
                .expect("shared");
        }
        if kind == ContainerKind::Board && !is_template && !trashed {
            kept = container.id;
        }
    }
    store.close().await;
    (database, kept)
}

/// What the database says is wrong with its keys and its pages: nothing, for a
/// database that is whole. PostgreSQL holds its keys itself.
async fn broken_keys(database: &fvoci_store::testing::TestDatabase) -> Vec<String> {
    if db_type() != DbType::Sqlite {
        return Vec::new();
    }
    let mut conn = sqlx::sqlite::SqliteConnection::connect(&common::raw_connection(database))
        .await
        .expect("the database file opens");
    let mut found: Vec<String> =
        sqlx::query_scalar("SELECT \"table\" FROM pragma_foreign_key_check")
            .fetch_all(&mut conn)
            .await
            .expect("the keys are checked");
    let whole: Vec<String> = sqlx::query_scalar("PRAGMA integrity_check")
        .fetch_all(&mut conn)
        .await
        .expect("the pages are checked");
    found.extend(whole.into_iter().filter(|line| line != "ok"));
    conn.close().await.expect("the connection closes");
    found
}

/// Every key of the schema from a table to another: its table and columns, what it
/// refers to, and what it does when that goes; sorted.
async fn keys(database: &fvoci_store::testing::TestDatabase) -> Vec<String> {
    let raw = common::raw_connection(database);
    let mut keys: Vec<String> = match db_type() {
        DbType::Sqlite => {
            let mut conn = sqlx::sqlite::SqliteConnection::connect(&raw)
                .await
                .expect("the database file opens");
            let keys = sqlx::query_scalar(
                "SELECT m.name || '(' || k.\"from\" || ') -> ' || k.\"table\" || '(' \
                 || COALESCE(k.\"to\", '') || ') ' || k.on_delete \
                 FROM sqlite_master AS m, pragma_foreign_key_list(m.name) AS k \
                 WHERE m.type = 'table'",
            )
            .fetch_all(&mut conn)
            .await
            .expect("the keys are listed");
            conn.close().await.expect("the connection closes");
            keys
        }
        DbType::Postgres => {
            let mut conn = common::connect_past_the_store(&raw).await;
            let keys = sqlx::query_scalar(
                "SELECT conrelid::regclass::text || ' ' || conname::text || ' ' \
                 || pg_get_constraintdef(oid) FROM pg_constraint \
                 WHERE connamespace = current_schema()::regnamespace AND contype = 'f'",
            )
            .fetch_all(&mut conn)
            .await
            .expect("the keys are listed");
            conn.close().await.expect("the connection closes");
            keys
        }
    };
    keys.sort();
    keys
}

/// A database with boards and wikis keeps every row of every table through the
/// migration, each container of the kind it was; what refers to a container still does;
/// and afterwards a container may be a diary, and of no other new kind.
#[tokio::test]
async fn a_database_from_before_keeps_every_row_and_takes_diaries() {
    let (database, _) = a_database_from_before().await;
    let old = by_column(&dump(&database).await);
    for table in [
        "containers",
        "container_members",
        "blocks",
        "sharing",
        "deleted_containers",
        "deleted_blocks",
    ] {
        assert!(!old[table].is_empty(), "the old {table} has rows");
    }
    let kinds = |rows: &[BTreeMap<String, Option<String>>]| -> Vec<Option<String>> {
        rows.iter().map(|row| row["kind"].clone()).collect()
    };
    assert_eq!(kinds(&old["containers"]).len(), 3);
    let old_keys = keys(&database).await;
    assert!(
        old_keys
            .iter()
            .filter(|key| key.contains("containers"))
            .count()
            > 5,
        "{old_keys:?}"
    );

    let store = Store::connect(database.db_type(), database.database_connection())
        .await
        .expect("the database is migrated");
    let mut new = by_column(&dump(&database).await);
    assert_eq!(new.remove("diary_days"), Some(Vec::new()));
    assert_eq!(
        new.keys().collect::<Vec<_>>(),
        old.keys().collect::<Vec<_>>(),
        "the tables are those there were"
    );
    for (table, rows) in &old {
        assert_eq!(&new[table], rows, "every row of {table} is as it was");
    }
    assert_eq!(broken_keys(&database).await, Vec::<String>::new());

    let mut new_keys = keys(&database).await;
    new_keys.retain(|key| !key.starts_with("diary_days"));
    assert_eq!(new_keys, old_keys, "every key there was is as it was");
    let system = store.system();

    // A diary is stored now, and the check still refuses a kind there is none of.
    let user_id = common::user(&store.scoped(fvoci_store::Tenant::Unset), "writer").await;
    let found = system
        .texts_unchecked(&format!(
            "SELECT CAST(id AS TEXT) || '|' || CAST(default_workspace_id AS TEXT) \
             FROM organizations WHERE owner_id = '{user_id}'"
        ))
        .await
        .expect("read");
    let (own_org, own_workspace) = found[0].split_once('|').expect("two identifiers");
    let own = store.for_org(own_org).expect("an identifier");
    let diary = Container {
        kind: ContainerKind::Diary,
        ..new_board(&new_id(), own_workspace, ContainerType::Private)
    };
    let diary = own
        .create_diary(diary, &user_id, DiaryMaking::IfNone)
        .await
        .expect("a diary is made")
        .diary;
    let unknown = system
        .execute_unchecked(&format!(
            "UPDATE containers SET kind = 'journal' WHERE id = '{}'",
            diary.id
        ))
        .await;
    assert!(unknown.is_err(), "a kind there is none of is stored");
    assert_eq!(broken_keys(&database).await, Vec::<String>::new());
    store.close().await;
    database.remove().await;
}

/// What the migration costs on SQLite by how many containers there are, each with a
/// block: the column of the kind is made anew where it stands, so every row of the
/// table is written. The times are printed, and the keys are whole afterwards.
#[tokio::test]
async fn the_migration_on_sqlite_is_timed_by_the_rows_it_rewrites() {
    if db_type() != DbType::Sqlite {
        return;
    }
    for containers in [1_000_u32, 10_000, 100_000] {
        let (database, board_id) = a_database_from_before().await;
        let store = Store::open(database.db_type(), database.database_connection())
            .await
            .expect("the database opens");
        let system = store.system();
        let numbers = format!(
            "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < {containers})"
        );
        let id = |prefix: &str| format!("printf('{prefix}-0000-4000-8000-%012x', n.i)");
        for (table, key, prefix) in [
            ("containers", "id", "c0000000"),
            ("blocks", "container_id", "b0000000"),
        ] {
            let columns = columns_of(&system, table).await;
            let values: Vec<String> = columns
                .iter()
                .map(|column| match column.as_str() {
                    "id" => id(prefix),
                    "container_id" => id("c0000000"),
                    other => format!("t.{other}"),
                })
                .collect();
            system
                .execute_unchecked(&format!(
                    "{numbers} INSERT INTO {table} ({}) SELECT {} FROM n, \
                     (SELECT * FROM {table} WHERE {key} = '{board_id}' LIMIT 1) AS t",
                    columns.join(", "),
                    values.join(", ")
                ))
                .await
                .expect("the rows are copied");
        }
        store.close().await;

        let started = Instant::now();
        let store = Store::connect(database.db_type(), database.database_connection())
            .await
            .expect("the database is migrated");
        let took = started.elapsed();
        let rows = store
            .system()
            .count_rows("containers")
            .await
            .expect("counted");
        println!("migration 55 on SQLite: {rows} containers, each with a block, in {took:?}");
        assert_eq!(rows, i64::from(containers) + 3);
        assert_eq!(broken_keys(&database).await, Vec::<String>::new());
        store.close().await;
        database.remove().await;
    }
}

/// A diary of the space, by the store.
async fn a_diary(own: &Store, workspace_id: &str, user_id: &str) -> String {
    let diary = Container {
        kind: ContainerKind::Diary,
        ..new_board(&new_id(), workspace_id, ContainerType::Private)
    };
    let made = own.create_diary(diary, user_id, DiaryMaking::Another).await;
    made.expect("made").diary.id
}

fn day(text: &str) -> DiaryDate {
    text.parse().expect("a day")
}

fn of_day(date: DiaryDate) -> NewDiaryEntry {
    NewDiaryEntry {
        date: date.to_string(),
        mood: None,
        weather: "clear".to_owned(),
        place: String::new(),
    }
}

/// Fills the diary to what one may hold: the first day by the store, and every other as
/// a copy of it, `every` days after the one before from 1900 on (a day after the other
/// is past 2009 at the limit).
async fn fill(test: &common::TestStore, own: &Store, diary_id: &str, user_id: &str, every: u32) {
    let first = day("1900-01-01");
    let template = own
        .create_diary_entry(diary_id, first, of_day(first), ENTRIES_PER_DIARY, user_id)
        .await
        .expect("made")
        .entry;
    let system = test.system();
    let more = ENTRIES_PER_DIARY - 1;
    let (numbers, day, id, fields) = match db_type() {
        DbType::Sqlite => (
            format!(
                "(WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < {more}) SELECT i FROM n) AS n"
            ),
            format!("date('1900-01-01', '+' || (n.i * {every}) || ' days')"),
            "printf('e0000000-0000-4000-8000-%012x', n.i)".to_owned(),
            format!("json_object('date', date('1900-01-01', '+' || (n.i * {every}) || ' days'))"),
        ),
        DbType::Postgres => (
            format!("generate_series(1, {more}) AS n(i)"),
            format!("(DATE '1900-01-01' + n.i * {every})::text"),
            "('e0000000-0000-4000-8000-' || lpad(to_hex(n.i), 12, '0'))::uuid".to_owned(),
            format!("jsonb_build_object('date', (DATE '1900-01-01' + n.i * {every})::text)"),
        ),
    };
    let block_columns = columns_of(&system, "blocks").await;
    let block_values: Vec<String> = block_columns
        .iter()
        .map(|column| match column.as_str() {
            "id" => id.clone(),
            "title" => day.clone(),
            "fields" => fields.clone(),
            other => format!("t.{other}"),
        })
        .collect();
    let day_columns = columns_of(&system, "diary_days").await;
    let day_values: Vec<String> = day_columns
        .iter()
        .map(|column| match column.as_str() {
            "day" => day.clone(),
            "month_day" => format!("substr({day}, 6)"),
            "entry_id" => id.clone(),
            other => format!("t.{other}"),
        })
        .collect();
    for (table, columns, values, key) in [
        ("blocks", &block_columns, &block_values, "id"),
        ("diary_days", &day_columns, &day_values, "entry_id"),
    ] {
        system
            .execute_unchecked(&format!(
                "INSERT INTO {table} ({}) SELECT {} FROM {numbers}, \
                 (SELECT * FROM {table} WHERE {key} = '{}') AS t",
                columns.join(", "),
                values.join(", "),
                template.id
            ))
            .await
            .expect("the entries are copied");
    }
    if db_type() == DbType::Postgres {
        system
            .execute_unchecked("ANALYZE blocks")
            .await
            .expect("analyzed");
        system
            .execute_unchecked("ANALYZE diary_days")
            .await
            .expect("analyzed");
    }
}

/// What the plan of a read of a diary must say, the entries of a month and day first,
/// the days of a month second and the days alone (for the runs) third: the days are
/// found by the index the read is for, an entry by its own key and not by going through
/// those of its diary, the days alone from the key without the rows, and no table is
/// read through (what the runs are put together from, SQLite goes through as a
/// subquery of its own, `written`: that is no table).
fn check_plan(read: usize, plan: &str) {
    // Only the runs (the third read) go through a subquery of their own.
    let own = |line: &str| {
        read == 2 && (line.contains("SCAN written") || line.contains("SCAN (subquery-"))
    };
    let through = |line: &str| match db_type() {
        DbType::Sqlite => line.contains("SCAN ") && !own(line),
        DbType::Postgres => line.contains("Seq Scan"),
    };
    assert!(
        !plan.lines().any(through),
        "a table is read through: {plan}"
    );
    let said: [&[&str]; 3] = match db_type() {
        DbType::Sqlite => [
            &[
                "diary_days_month_day_idx (container_id=? AND month_day=?)",
                "SEARCH blocks USING INDEX",
                "AND id=?)",
            ],
            &[
                "SEARCH diary_days USING INDEX",
                "(container_id=? AND day>? AND day<?)",
            ],
            &["SEARCH diary_days USING COVERING INDEX", "(container_id=?)"],
        ],
        DbType::Postgres => [
            &["diary_days_month_day_idx", "blocks_pkey"],
            &["diary_days_pkey", "(day >= "],
            &["Index Only Scan using diary_days_pkey"],
        ],
    };
    for part in said[read] {
        assert!(
            plan.contains(part),
            "the plan does not say `{part}`: {plan}"
        );
    }
}

/// A diary as full as one may be answers for a day and for a month from the rows of its
/// days, and reads no more of the entries than it answers with: the plans of the two
/// queries name the indexes and scan no table, and both are printed with what the reads
/// took.
#[tokio::test]
async fn a_full_diary_answers_for_a_day_without_being_read() {
    const RUNS: u32 = 20;
    let test = common::setup().await;
    let user_id = common::user(&test, "writer").await;
    let (own, workspace_id) = common::in_own_space(&test, &user_id).await;
    let diary_id = a_diary(&own, &workspace_id, &user_id).await;
    fill(&test, &own, &diary_id, &user_id, 1).await;
    // Another diary beside it, to be left out of every answer.
    let beside = a_diary(&own, &workspace_id, &user_id).await;
    let of_beside = day("2000-11-21");
    own.create_diary_entry(
        &beside,
        of_beside,
        of_day(of_beside),
        ENTRIES_PER_DIARY,
        &user_id,
    )
    .await
    .expect("made");
    // The diary is full: one more day is refused.
    let over = day("2026-11-21");
    let one_more = own
        .create_diary_entry(&diary_id, over, of_day(over), ENTRIES_PER_DIARY, &user_id)
        .await;
    assert!(one_more.is_err(), "a full diary takes a day: {one_more:?}");

    let at: MonthDay = "11-21".parse().expect("a month and day");
    let (from, to) = (day("2000-11-01"), day("2000-11-30"));
    let mut took = [Duration::ZERO; 3];
    for _ in 0..RUNS {
        let started = Instant::now();
        let entries = own.get_diary_entries_on(&diary_id, at).await.expect("read");
        took[0] += started.elapsed();
        // 1900 to 2009: a year an entry.
        assert_eq!(entries.len(), 109);
        assert!(entries.iter().all(|entry| entry.title.ends_with("-11-21")));
        assert_eq!(entries[0].title, "2008-11-21");
        let started = Instant::now();
        let days = own
            .get_diary_days(&diary_id, Some(from), Some(to))
            .await
            .expect("read");
        took[1] += started.elapsed();
        assert_eq!(days.len(), 30);
        let started = Instant::now();
        let runs = own.get_diary_runs(&diary_id).await.expect("read");
        took[2] += started.elapsed();
        assert_eq!(runs.len(), 1, "a day after the other is one run");
        assert_eq!(
            (runs[0].from, runs[0].days),
            (day("1900-01-01"), ENTRIES_PER_DIARY)
        );
    }
    let plans = own
        .plans_of_diary_reads(&diary_id, at, from, to)
        .await
        .expect("explained");
    let all = own
        .get_diary_days(&diary_id, None, None)
        .await
        .expect("read");
    assert_eq!(
        all.len(),
        usize::try_from(ENTRIES_PER_DIARY).expect("a count")
    );
    for (read, ((name, plan), took)) in [
        "the entries of 11-21 in every year",
        "the days of a month",
        "the runs of the written days",
    ]
    .into_iter()
    .zip(&plans)
    .zip(took)
    .enumerate()
    {
        println!(
            "{name}, of {ENTRIES_PER_DIARY} entries on {:?}: {:?} a read; the plan:\n  {}",
            db_type(),
            took / RUNS,
            plan.join("\n  ")
        );
        check_plan(read, &plan.join("\n"));
    }
    test.teardown().await;
}

/// The most runs a diary may have, a run a day: every other day written, to the limit.
/// They are read from the key of the days alone, and what the read took and how large
/// their answer is are printed.
#[tokio::test]
async fn a_full_diary_of_every_other_day_answers_its_runs_from_the_key() {
    const RUNS: u32 = 5;
    let test = common::setup().await;
    let user_id = common::user(&test, "writer").await;
    let (own, workspace_id) = common::in_own_space(&test, &user_id).await;
    let diary_id = a_diary(&own, &workspace_id, &user_id).await;
    fill(&test, &own, &diary_id, &user_id, 2).await;
    let mut took = Duration::ZERO;
    let mut runs = Vec::new();
    for _ in 0..RUNS {
        let started = Instant::now();
        runs = own.get_diary_runs(&diary_id).await.expect("read");
        took += started.elapsed();
    }
    assert_eq!(
        runs.len(),
        usize::try_from(ENTRIES_PER_DIARY).expect("a count")
    );
    assert!(runs.iter().all(|run| run.days == 1 && run.from == run.to));
    assert_eq!(runs[1].from, day("1900-01-03"));
    let at: MonthDay = "11-21".parse().expect("a month and day");
    let [.., plan] = own
        .plans_of_diary_reads(&diary_id, at, day("2000-11-01"), day("2000-11-30"))
        .await
        .expect("explained");
    check_plan(2, &plan.join("\n"));
    let answer = serde_json::to_vec(&runs).expect("serialized").len();
    println!(
        "the runs of {ENTRIES_PER_DIARY} entries a day apart on {:?}: {:?} a read, \
         {answer} bytes of JSON; the plan:\n  {}",
        db_type(),
        took / RUNS,
        plan.join("\n  ")
    );
    test.teardown().await;
}
