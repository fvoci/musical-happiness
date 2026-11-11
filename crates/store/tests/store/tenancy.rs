//! The wall between organizations: what PostgreSQL's row-level security keeps apart
//! whatever a query asks for, and the role the tests connect as, which the policies
//! hold. SQLite has one organization and no such wall.

use fvoci_store::DbType;
use sqlx::Connection as _;
use sqlx::postgres::PgConnection;

use crate::common;

/// The tests connect to PostgreSQL as a role that is no superuser and is not exempt
/// from row-level security. As one that is, every test of the wall would pass with the
/// wall gone.
#[tokio::test]
async fn the_tests_connect_to_postgresql_as_a_role_the_policies_hold() {
    if common::db_type() != DbType::Postgres {
        return;
    }
    let url = std::env::var(fvoci_store::testing::CONN_STRING_ENV)
        .expect("a PostgreSQL test run names its database");
    let mut conn = PgConnection::connect(&url)
        .await
        .expect("the test PostgreSQL server accepts connections");
    let (role, superuser, bypasses): (String, bool, bool) = sqlx::query_as(
        "SELECT rolname::text, rolsuper, rolbypassrls FROM pg_roles WHERE rolname = current_user",
    )
    .fetch_one(&mut conn)
    .await
    .expect("the role is read");
    conn.close().await.expect("the connection closes");
    assert!(
        !superuser && !bypasses,
        "the tests must not connect as {role}: superuser {superuser}, bypasses row-level \
         security {bypasses}"
    );
}
