//! The wall between organizations: what PostgreSQL's row-level security keeps apart
//! whatever a query asks for, and the role the tests connect as, which the policies
//! hold. SQLite has one organization and no such wall.

use fvoci_model::{Board, BoardType, new_id};
use fvoci_store::{DbType, Store, Tenant};
use sqlx::Connection as _;

use crate::common;

/// The policies hold the role the tests connect as, on tables it owns: a table whose
/// row-level security is forced and that has no policy reads as empty to it. A
/// superuser, a role with `BYPASSRLS`, and the owner of a table whose row-level
/// security is not forced read every row; as one of those, every test of the wall
/// would pass with the wall gone.
#[tokio::test]
async fn the_policies_hold_the_role_the_tests_connect_as() {
    if common::db_type() != DbType::Postgres {
        return;
    }
    let database = common::empty_database().await;
    let mut conn = common::connect_past_the_store(database.dbconfig()).await;
    sqlx::raw_sql(
        "CREATE TABLE probe (x INT); INSERT INTO probe VALUES (1); \
         ALTER TABLE probe ENABLE ROW LEVEL SECURITY",
    )
    .execute(&mut conn)
    .await
    .expect("the probe is made");
    let not_forced: i64 = sqlx::query_scalar("SELECT count(*) FROM probe")
        .fetch_one(&mut conn)
        .await
        .expect("the probe is read");
    sqlx::raw_sql("ALTER TABLE probe FORCE ROW LEVEL SECURITY")
        .execute(&mut conn)
        .await
        .expect("row-level security is forced");
    let forced: i64 = sqlx::query_scalar("SELECT count(*) FROM probe")
        .fetch_one(&mut conn)
        .await
        .expect("the probe is read");
    conn.close().await.expect("the connection closes");
    database.remove().await;

    assert_eq!(
        not_forced, 1,
        "the role owns its tables: without FORCE the policies do not hold it"
    );
    assert_eq!(
        forced, 0,
        "with FORCE and no policy the table reads as empty"
    );
}

/// Stores an open board of the team by the user through `store` and returns its
/// identifier.
async fn board(store: &Store, team_id: &str, user_id: &str, title: &str) -> String {
    let mut board = Board {
        id: new_id(),
        team_id: team_id.to_owned(),
        board_type: BoardType::Open,
        title: title.to_owned(),
        ..Board::default()
    };
    store
        .insert_board(&mut board, user_id)
        .await
        .expect("the board is stored");
    board.id
}

/// What two organizations store in one table stays apart, whatever the statement asks
/// for: each reads its own rows, a handle that names no organization reads none and
/// stores none, the system's handle reads all, and nobody stores a row as another's.
#[tokio::test]
async fn the_boards_of_two_organizations_are_out_of_each_others_reach() {
    if common::db_type() != DbType::Postgres {
        return;
    }
    let test = common::setup().await;
    let unset = test.scoped(Tenant::Unset);
    let other_org = unset
        .create_organization("the other")
        .await
        .expect("a second organization is made");
    let ours = &test.store;
    let theirs = test.for_org(&other_org).expect("an identifier");

    let user_id = common::user(ours, "anybody").await;
    let team_id = common::team(ours).await;
    let our_board = board(ours, &team_id, &user_id, "ours").await;
    let their_board = board(&theirs, &team_id, &user_id, "theirs").await;
    board(&theirs, &team_id, &user_id, "theirs too").await;

    let count = async |store: &Store| {
        store
            .count_rows("boards")
            .await
            .expect("the boards are counted")
    };
    assert_eq!(count(ours).await, 1, "ours");
    assert_eq!(count(&theirs).await, 2, "theirs");
    assert_eq!(count(&unset).await, 0, "no organization named");
    assert_eq!(count(&test.system()).await, 3, "the system's handle");

    // By its identifier too: what is another's is not there.
    assert_eq!(
        ours.get_board(&our_board).await.expect("ours").title,
        "ours"
    );
    let missing = ours
        .get_board(&their_board)
        .await
        .expect_err("their board is out of reach");
    assert!(missing.is_not_found(), "got {missing:?}");
    // Nor is it changed or deleted: the statement finds no row.
    let changed = ours
        .execute_unchecked(&format!(
            "UPDATE boards SET title = 'taken' WHERE id = '{their_board}'"
        ))
        .await
        .expect("the statement runs");
    assert_eq!(changed, 0, "an update of their board");
    let deleted = ours
        .execute_unchecked("DELETE FROM boards")
        .await
        .expect("the statement runs");
    assert_eq!(deleted, 1, "a delete of everything deletes ours");
    assert_eq!(count(&theirs).await, 2, "and leaves theirs");

    // A row is stored as the organization's own or not at all.
    let insert = |org: &str| {
        format!(
            "INSERT INTO boards (id, org_id, team_id, type, title, description, icon, \
             show_description, is_template, properties, card_properties, create_at, update_at) \
             VALUES ('{}', {org}, '{team_id}', 'O', '', '', '', false, false, '{{}}', '[]', 0, 0)",
            new_id()
        )
    };
    let as_theirs = ours
        .execute_unchecked(&insert(&format!("'{other_org}'")))
        .await
        .expect_err("a row of another organization is refused");
    assert!(
        as_theirs.to_string().contains("row-level security"),
        "{as_theirs}"
    );
    let as_nobody = unset
        .execute_unchecked(&insert("DEFAULT"))
        .await
        .expect_err("a row of no organization is refused");
    // The policy refuses it before the column does.
    assert!(
        as_nobody.to_string().contains("row-level security"),
        "{as_nobody}"
    );
    // The system's handle stores for whom it says, and for nobody if it does not say.
    test.system()
        .execute_unchecked(&insert(&format!("'{other_org}'")))
        .await
        .expect("the system stores a row of an organization it names");
    let unnamed = test
        .system()
        .execute_unchecked(&insert("DEFAULT"))
        .await
        .expect_err("the system names no organization by itself");
    assert!(unnamed.to_string().contains("org_id"), "{unnamed}");
    assert_eq!(count(&theirs).await, 3, "theirs, with the system's row");

    test.teardown().await;
}

/// The organization a transaction reaches is the transaction's and no longer: a
/// connection that goes back to the pool carries nothing of it to the next operation,
/// whichever handle that is of.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn nothing_of_an_organization_stays_on_a_pooled_connection() {
    if common::db_type() != DbType::Postgres {
        return;
    }
    let test = common::setup().await;
    let unset = test.scoped(Tenant::Unset);
    let other_org = unset
        .create_organization("the other")
        .await
        .expect("a second organization is made");
    let theirs = test.for_org(&other_org).expect("an identifier");
    let user_id = common::user(&test, "anybody").await;
    let team_id = common::team(&test).await;
    board(&test, &team_id, &user_id, "ours").await;
    board(&theirs, &team_id, &user_id, "theirs").await;
    board(&theirs, &team_id, &user_id, "theirs too").await;

    // Many more operations than the pool has connections, of every handle at once:
    // every connection serves each of them in turn.
    let mut reads = tokio::task::JoinSet::new();
    for round in 0..40 {
        for (store, expected) in [
            (test.store.clone(), 1),
            (theirs.clone(), 2),
            (unset.clone(), 0),
            (test.system(), 3),
        ] {
            reads.spawn(async move {
                let counted = store.count_rows("boards").await.expect("counted");
                (round, store.tenant(), counted, expected)
            });
        }
    }
    while let Some(read) = reads.join_next().await {
        let (round, tenant, counted, expected) = read.expect("the read ends");
        assert_eq!(counted, expected, "round {round}, {tenant:?}");
    }

    test.teardown().await;
}
