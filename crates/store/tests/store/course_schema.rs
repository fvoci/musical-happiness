//! The migration of courses (61) on a database a server left before it: every existing
//! row, foreign key, check constraint, and diary/sharing trigger survives, courses can
//! be stored in both workspaces of people and personal spaces, and unknown kinds stay
//! refused.

use std::collections::BTreeMap;

use fvoci_model::{
    Block, BlockType, Container, ContainerKind, ContainerVisibility, DiaryDate, NewDiaryEntry,
    new_id,
};
use fvoci_store::{DbType, Store};
use sqlx::Connection as _;

use crate::boards::new_board;
use crate::common::{self, db_type};
use crate::workspace_rename::{Dump, dump};

/// The rows of every table, each as its values by column name (column order is not part
/// of the migration contract on SQLite where `kind` is rebuilt in place).
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

/// Fixture identifiers seeded before migration 61.
struct PreMigration {
    database: fvoci_store::testing::TestDatabase,
    people_org: String,
    people_ws: String,
    user_id: String,
    own_org: String,
    own_ws: String,
    board_id: String,
    diary_id: String,
}

/// Seeds a database before migration 61 (at migration 59; 60 is unused) with an
/// organization of people (board, wiki, template, trashed board, blocks, sharing) and a
/// personal space (diary + entry).
async fn database_before_courses() -> PreMigration {
    let database = common::empty_database().await;
    fvoci_store::testing::migrate_to_before_courses(&database).await;
    common::execute_raw(&common::raw_connection(&database), "DELETE FROM data_steps").await;
    let store = Store::open(database.db_type(), database.database_connection())
        .await
        .expect("the database opens");
    let (people_org, people_ws) = common::organization(&store, "people").await;
    let scoped = store.for_org(&people_org).expect("an identifier");
    let user_id = common::pre0072_user(&scoped, &people_org, &people_ws, "member").await;

    let mut board_id = String::new();
    for (kind, is_template, trashed) in [
        (ContainerKind::Board, false, false),
        (ContainerKind::Wiki, false, false),
        (ContainerKind::Board, true, false),
        (ContainerKind::Board, false, true),
    ] {
        let container = Container {
            kind,
            is_template,
            title: format!("a {} before courses", kind.as_str()),
            ..new_board(&new_id(), &people_ws, ContainerVisibility::Private)
        };
        scoped
            .execute_unchecked(&format!(
                "INSERT INTO containers \
                 (id, workspace_id, created_by, modified_by, type, kind, title, description, \
                  icon, show_description, is_template, properties, card_properties, create_at, update_at) \
                 VALUES ('{}', '{people_ws}', '{user_id}', '{user_id}', 'P', '{}', '{}', '', '', \
                  false, {is_template}, '{{}}', '[]', 1, 1)",
                container.id,
                kind.as_str(),
                container.title,
            ))
            .await
            .expect("stored");
        scoped
            .execute_unchecked(&format!(
                "INSERT INTO container_members \
                 (container_id, user_id, scheme_admin, scheme_editor, scheme_commenter, scheme_viewer) \
                 VALUES ('{}', '{user_id}', true, true, false, false)",
                container.id
            ))
            .await
            .expect("member stored");
        let block_type = match kind {
            ContainerKind::Wiki => BlockType::Page,
            _ => BlockType::Card,
        };
        let mut blocks = Vec::with_capacity(2);
        for title in ["one", "two"] {
            let mut block = Block {
                id: new_id(),
                container_id: container.id.clone(),
                block_type: block_type.clone(),
                title: title.to_owned(),
                ..Block::default()
            };
            scoped
                .telling_nobody()
                .insert_block(&mut block, &user_id)
                .await
                .expect("stored");
            blocks.push(block);
        }
        if trashed {
            trash_container_before_courses(&scoped, &container, &blocks, &user_id).await;
        } else if !is_template {
            let old_links = ["shar", "ing"].concat();
            scoped
                .execute_unchecked(&format!(
                    "INSERT INTO {old_links} (container_id, enabled, token, modified_by, update_at) \
                     VALUES ('{}', true, '{}', '{user_id}', 1)",
                    container.id,
                    new_id()
                ))
                .await
                .expect("linked");
        }
        if kind == ContainerKind::Board && !is_template && !trashed {
            board_id = container.id;
        }
    }

    let (own_org, own_ws, diary_id) = seed_diary_in_own_space(&store, &user_id).await;
    store.close().await;
    PreMigration {
        database,
        people_org,
        people_ws,
        user_id,
        own_org,
        own_ws,
        board_id,
        diary_id,
    }
}

/// Moves a pre-`0069` container and its blocks to `deleted_containers` and
/// `deleted_blocks` directly so seeding at migration `0060` does not query post-`0069`
/// `subscriptions.user_id`.
async fn trash_container_before_courses(
    scoped: &Store,
    container: &Container,
    blocks: &[Block],
    user_id: &str,
) {
    let deletion_id = new_id();
    let now = 1_000_i64;
    for block in blocks {
        let mut trashed = block.clone();
        trashed.deleted_at = Some(now);
        let json = serde_json::to_string(&trashed)
            .expect("block json")
            .replace('\'', "''");
        scoped
            .execute_unchecked(&format!(
                "INSERT INTO deleted_blocks \
                 (id, container_id, parent_id, deleted_at, deleted_by, deletion_id, block) \
                 VALUES ('{}', '{}', NULL, {now}, '{user_id}', '{deletion_id}', '{json}')",
                block.id, container.id
            ))
            .await
            .expect("deleted_block stored");
    }
    let mut in_trash = container.clone();
    in_trash.deleted_at = Some(now);
    let json = serde_json::to_string(&in_trash)
        .expect("container json")
        .replace(r#""visibility":"private""#, r#""type":"P""#)
        .replace('\'', "''");
    scoped
        .execute_unchecked(&format!(
            "INSERT INTO deleted_containers \
             (id, workspace_id, deleted_at, deleted_by, deletion_id, container, members, files, places) \
             VALUES ('{}', '{}', {now}, '{user_id}', '{deletion_id}', '{json}', '[]', '[]', '[]')",
            container.id, container.workspace_id
        ))
        .await
        .expect("deleted_container stored");
    for sql in [
        format!("DELETE FROM blocks WHERE container_id = '{}'", container.id),
        format!("DELETE FROM containers WHERE id = '{}'", container.id),
    ] {
        scoped
            .execute_unchecked(&sql)
            .await
            .expect("live row deleted");
    }
}

async fn seed_diary_in_own_space(store: &Store, user_id: &str) -> (String, String, String) {
    let found = store
        .system()
        .texts_unchecked(&format!(
            "SELECT CAST(id AS TEXT) || '|' || CAST(default_workspace_id AS TEXT) \
             FROM organizations WHERE owner_id = '{user_id}'"
        ))
        .await
        .expect("personal space");
    let (own_org, own_ws) = found[0].split_once('|').expect("two identifiers");
    let (own_org, own_ws) = (own_org.to_owned(), own_ws.to_owned());
    let own = store.for_org(&own_org).expect("an identifier");
    let diary_id = new_id();
    own.execute_unchecked(&format!(
        "INSERT INTO containers \
         (id, workspace_id, created_by, modified_by, type, kind, title, description, icon, \
          show_description, is_template, properties, card_properties, create_at, update_at) \
         VALUES ('{diary_id}', '{own_ws}', '{user_id}', '{user_id}', 'P', 'diary', 'my diary', \
          '', '', false, false, '{{}}', '[]', 1, 1)"
    ))
    .await
    .expect("diary stored");
    own.execute_unchecked(&format!(
        "INSERT INTO container_members \
         (container_id, user_id, scheme_admin, scheme_editor, scheme_commenter, scheme_viewer) \
         VALUES ('{diary_id}', '{user_id}', true, true, false, false)"
    ))
    .await
    .expect("diary member stored");
    let date: DiaryDate = "2026-11-22".parse().expect("a date");
    own.create_diary_entry(
        &diary_id,
        date,
        NewDiaryEntry {
            date: date.to_string(),
            mood: None,
            weather: "clear".to_owned(),
            place: String::new(),
        },
        100,
        user_id,
    )
    .await
    .expect("entry stored");
    (own_org, own_ws, diary_id)
}

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

#[tokio::test]
async fn migration_0061_preserves_rows_keys_and_triggers_and_allows_courses() {
    let pre = database_before_courses().await;
    let old = by_column(&dump(&pre.database).await);
    for table in [
        "containers",
        "container_members",
        "blocks",
        "sharing",
        "deleted_containers",
        "deleted_blocks",
        "diary_days",
    ] {
        assert!(!old[table].is_empty(), "pre-0061 {table} has rows");
    }
    let old_keys = keys(&pre.database).await;
    assert!(
        old_keys
            .iter()
            .filter(|key| key.contains("containers"))
            .count()
            > 5,
        "{old_keys:?}"
    );

    fvoci_store::testing::migrate_up_to(
        pre.database.db_type(),
        pre.database.database_connection(),
        61,
    )
    .await;
    let new = by_column(&dump(&pre.database).await);
    assert_eq!(
        new.keys().collect::<Vec<_>>(),
        old.keys().collect::<Vec<_>>(),
        "migration 0061 adds no new tables"
    );
    for (table, rows) in &old {
        assert_eq!(&new[table], rows, "every row of {table} is preserved");
    }
    assert_eq!(broken_keys(&pre.database).await, Vec::<String>::new());
    assert_eq!(
        keys(&pre.database).await,
        old_keys,
        "every foreign key is preserved"
    );

    let store = Store::connect(pre.database.db_type(), pre.database.database_connection())
        .await
        .expect("migrations up to latest apply cleanly");

    // Courses can now be stored in both workspaces of people and personal spaces.
    for (org_id, workspace_id) in [
        (&pre.people_org, &pre.people_ws),
        (&pre.own_org, &pre.own_ws),
    ] {
        let scoped = store.for_org(org_id).expect("an identifier");
        let mut course = Container {
            kind: ContainerKind::Course,
            title: "Rust Basics".to_owned(),
            ..new_board(&new_id(), workspace_id, ContainerVisibility::Private)
        };
        scoped
            .insert_container_with_admin(&mut course, &pre.user_id)
            .await
            .expect("a course is stored");
        let read = scoped.get_container(&course.id).await.expect("read back");
        assert_eq!(read.kind, ContainerKind::Course);
    }

    assert_triggers_and_checks_hold(&store, &pre).await;
    assert_eq!(broken_keys(&pre.database).await, Vec::<String>::new());
    store.close().await;
    pre.database.remove().await;
}

fn assert_err_contains(result: Result<u64, fvoci_store::StoreError>, needle: &str, context: &str) {
    let err = result.expect_err(context).to_string();
    assert!(
        err.contains(needle),
        "{context}: expected `{needle}`, got `{err}`"
    );
}

async fn assert_triggers_and_checks_hold(store: &Store, pre: &PreMigration) {
    let system = store.system();
    let bad_kind = system
        .execute_unchecked(&format!(
            "UPDATE containers SET kind = 'lesson' WHERE id = '{}'",
            pre.board_id
        ))
        .await;
    let check_needle = match db_type() {
        DbType::Sqlite => "CHECK constraint failed",
        DbType::Postgres => "containers_kind_check",
    };
    assert_err_contains(bad_kind, check_needle, "unknown container kind CHECK");

    // 1) diaries_of_a_space (BEFORE INSERT on containers)
    let insert_diary_sql = match db_type() {
        DbType::Sqlite => format!(
            "INSERT INTO containers (id, workspace_id, visibility, kind, title, description, icon, \
             show_description, is_template, properties, card_properties, create_at, update_at) \
             VALUES ('{}', '{}', 'private', 'diary', 'forbidden', '', '', 0, 0, '{{}}', '[]', 0, 0)",
            new_id(),
            pre.people_ws
        ),
        DbType::Postgres => format!(
            "INSERT INTO containers (id, org_id, workspace_id, visibility, kind, title, description, icon, \
             show_description, is_template, properties, card_properties, create_at, update_at) \
             VALUES ('{}', '{}', '{}', 'private', 'diary', 'forbidden', '', '', false, false, '{{}}', '[]', 0, 0)",
            new_id(),
            pre.people_org,
            pre.people_ws
        ),
    };
    assert_err_contains(
        system.execute_unchecked(&insert_diary_sql).await,
        "a diary is of the space of an account",
        "diaries_of_a_space trigger",
    );

    // 2) diaries_stay_of_a_space (BEFORE UPDATE on containers)
    assert_err_contains(
        system
            .execute_unchecked(&format!(
                "UPDATE containers SET kind = 'diary' WHERE id = '{}'",
                pre.board_id
            ))
            .await,
        "a diary is of the space of an account",
        "diaries_stay_of_a_space trigger",
    );

    // 3) public_links_of_no_diary (BEFORE INSERT on public_links)
    let insert_share_sql = match db_type() {
        DbType::Sqlite => format!(
            "INSERT INTO public_links (container_id, enabled, token, update_at) \
             VALUES ('{}', 1, 'tok-diary', 0)",
            pre.diary_id
        ),
        DbType::Postgres => format!(
            "INSERT INTO public_links (container_id, org_id, enabled, token, update_at) \
             VALUES ('{}', '{}', TRUE, 'tok-diary', 0)",
            pre.diary_id, pre.own_org
        ),
    };
    assert_err_contains(
        system.execute_unchecked(&insert_share_sql).await,
        "a diary has no link",
        "public_links_of_no_diary trigger",
    );

    // 4) public_links_stay_of_no_diary (BEFORE UPDATE OF container_id on public_links)
    let update_share_sql = match db_type() {
        DbType::Sqlite => format!(
            "UPDATE public_links SET container_id = '{}' WHERE container_id = '{}'",
            pre.diary_id, pre.board_id
        ),
        DbType::Postgres => format!(
            "UPDATE public_links SET container_id = '{}', org_id = '{}' WHERE container_id = '{}'",
            pre.diary_id, pre.own_org, pre.board_id
        ),
    };
    assert_err_contains(
        system.execute_unchecked(&update_share_sql).await,
        "a diary has no link",
        "public_links_stay_of_no_diary trigger",
    );
}
