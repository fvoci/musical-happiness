//! The wall between organizations: what PostgreSQL's row-level security keeps apart
//! whatever a query asks for, and the role the tests connect as, which the policies
//! hold. SQLite has one organization and no such wall.

use fvoci_store::DbType;
use sqlx::Connection as _;
use sqlx::postgres::PgConnection;

use crate::common;

/// A connection of the test's own to its database, past the store.
async fn connect_past_the_store(dbconfig: &str) -> PgConnection {
    let (url, schema) = dbconfig
        .rsplit_once("search_path=")
        .expect("the test dbconfig names its schema");
    let mut conn = PgConnection::connect(url.trim_end_matches(['?', '&']))
        .await
        .expect("the test PostgreSQL server accepts connections");
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!("SET search_path TO {schema}")))
        .execute(&mut conn)
        .await
        .expect("the schema is selected");
    conn
}

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
    let mut conn = connect_past_the_store(database.dbconfig()).await;
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
