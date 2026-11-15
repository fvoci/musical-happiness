//! A server that connects as a role that does not own the tables: their owner (the
//! database's administrator, a superuser or not) brings the schema up to date, and the
//! server's role only uses the tables. It opens a schema that is up to date without a
//! statement that changes it, refuses one that is behind, and cannot turn the wall
//! between organizations off.

use std::path::PathBuf;

use fvoci_store::testing::{self, TableOwner, TestDatabase};
use fvoci_store::{DbType, Store, StoreError};

use crate::common;

async fn owned_apart(owner: TableOwner) -> TestDatabase {
    let directory = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("store-tests");
    TestDatabase::create_owned_apart(&directory, owner).await
}

/// The owner migrates, and the server's role opens and uses the tables it brought.
async fn a_role_that_does_not_own_the_tables_uses_them(owner: TableOwner) {
    let database = owned_apart(owner).await;
    let owner_store = Store::connect(database.db_type(), database.owner_dbconfig())
        .await
        .expect("the owner brings the schema up to date");
    if owner == TableOwner::Role {
        assert_eq!(
            owner_store
                .role_past_the_wall()
                .await
                .expect("the role is read"),
            None,
            "an owner that is no superuser migrated under the forced policies"
        );
    }
    owner_store.close().await;

    let store = Store::connect(database.db_type(), database.dbconfig())
        .await
        .expect("the server's role opens the schema as it is");
    let (org_id, workspace_id) = common::organization(&store, "apart").await;
    let store = store.for_org(&org_id).expect("an identifier");
    let user_id = common::user(&store, "someone").await;
    let boards = common::create_test_boards(&store, &workspace_id, &user_id, 2).await;
    assert_eq!(boards.len(), 2, "the role writes rows");
    store.close().await;
    database.remove().await;
}

#[tokio::test]
async fn a_role_that_does_not_own_the_tables_uses_what_a_superuser_migrated() {
    a_role_that_does_not_own_the_tables_uses_them(TableOwner::Superuser).await;
}

#[tokio::test]
async fn a_role_that_does_not_own_the_tables_uses_what_an_owner_held_by_the_wall_migrated() {
    a_role_that_does_not_own_the_tables_uses_them(TableOwner::Role).await;
}

/// A schema that is behind is not the server's role's to change: it is refused with
/// what to do, and left as it was, whoever owns the tables.
#[tokio::test]
async fn a_role_that_does_not_own_the_tables_refuses_a_schema_that_is_behind() {
    if common::db_type() != DbType::Postgres {
        return;
    }
    for owner in [TableOwner::Superuser, TableOwner::Role] {
        let database = owned_apart(owner).await;
        let behind = testing::latest_migration() - 1;
        testing::migrate_up_to(database.db_type(), database.owner_dbconfig(), behind).await;

        let refused = Store::connect(database.db_type(), database.dbconfig())
            .await
            .expect_err("the server's role does not migrate");
        assert!(
            matches!(&refused, StoreError::SchemaNotOwned { reason, .. }
                if reason.contains("not applied")),
            "{owner:?}: {refused}"
        );

        let untouched = Store::open(database.db_type(), database.owner_dbconfig())
            .await
            .expect("the owner connects");
        assert!(
            !untouched
                .schema_is_current()
                .await
                .expect("the record is read"),
            "{owner:?}: the refusal changed nothing"
        );
        untouched.close().await;

        let owner_store = Store::connect(database.db_type(), database.owner_dbconfig())
            .await
            .expect("the owner brings it up to date");
        owner_store.close().await;
        let store = Store::connect(database.db_type(), database.dbconfig())
            .await
            .expect("then the server's role opens it");
        store.close().await;
        database.remove().await;
    }
}

/// A record that is not this build's (a newer server's migration) is refused to a role
/// that does not own the tables with the reason, not as a schema to bring up to date.
#[tokio::test]
async fn a_role_that_does_not_own_the_tables_is_told_why_a_record_is_not_this_builds() {
    if common::db_type() != DbType::Postgres {
        return;
    }
    let database = owned_apart(TableOwner::Superuser).await;
    Store::connect(database.db_type(), database.owner_dbconfig())
        .await
        .expect("the owner brings the schema up to date")
        .close()
        .await;
    common::execute_raw(
        database.owner_dbconfig(),
        "INSERT INTO _sqlx_migrations (version, description, success, checksum, \
         execution_time) VALUES (999999, 'newer', true, '\\x00', 0)",
    )
    .await;
    let refused = Store::connect(database.db_type(), database.dbconfig())
        .await
        .expect_err("a newer server's record is refused");
    assert!(
        matches!(&refused, StoreError::SchemaNotOwned { reason, .. }
            if reason.contains("newer server")),
        "{refused}"
    );
    database.remove().await;
}

/// An empty schema that the server's role may not make tables in is refused the same
/// way: the owner makes them.
#[tokio::test]
async fn a_role_that_may_not_make_tables_refuses_an_empty_schema() {
    if common::db_type() != DbType::Postgres {
        return;
    }
    let database = owned_apart(TableOwner::Superuser).await;
    let refused = Store::connect(database.db_type(), database.dbconfig())
        .await
        .expect_err("nothing is made as the server's role");
    assert!(
        matches!(refused, StoreError::SchemaNotOwned { .. }),
        "{refused}"
    );
    database.remove().await;
}

/// The changes of the data that the migrations leave to the server are DML: the
/// server's role does those its owner did not.
#[tokio::test]
async fn a_role_that_does_not_own_the_tables_does_the_data_steps_left_to_it() {
    let database = owned_apart(TableOwner::Superuser).await;
    let latest = testing::latest_migration();
    testing::migrate_up_to(database.db_type(), database.owner_dbconfig(), latest).await;
    let store = Store::connect(database.db_type(), database.dbconfig())
        .await
        .expect("the server's role opens the schema and does the data steps");
    assert_eq!(
        store.count_rows("data_steps").await.expect("counted"),
        0,
        "no step is left"
    );
    store.close().await;
    database.remove().await;
}

/// A table the server's role was not granted, as one a migration made before the
/// default privileges were set, is named at the start rather than failing requests.
#[tokio::test]
async fn a_role_that_was_not_granted_a_table_is_told_which() {
    if common::db_type() != DbType::Postgres {
        return;
    }
    let database = owned_apart(TableOwner::Superuser).await;
    Store::connect(database.db_type(), database.owner_dbconfig())
        .await
        .expect("the owner brings the schema up to date")
        .close()
        .await;
    common::execute_raw(
        database.owner_dbconfig(),
        "DO $$ DECLARE grantee text; BEGIN \
           FOR grantee IN SELECT DISTINCT g.grantee FROM information_schema.role_table_grants g \
             WHERE g.table_schema = current_schema() AND g.table_name = 'blocks' \
             AND g.grantee <> current_user LOOP \
             EXECUTE format('REVOKE DELETE ON blocks FROM %I', grantee); \
           END LOOP; END $$",
    )
    .await;
    let refused = Store::connect(database.db_type(), database.dbconfig())
        .await
        .expect_err("a table the role may not write is refused");
    let said = refused.to_string();
    assert!(said.contains("blocks") && said.contains("grant"), "{said}");
    database.remove().await;
}

/// What the owner of the tables may do to the wall, the server's role may not: a server
/// that is broken into cannot let one organization read another's rows.
#[tokio::test]
async fn a_role_that_does_not_own_the_tables_cannot_turn_the_wall_off() {
    if common::db_type() != DbType::Postgres {
        return;
    }
    let database = owned_apart(TableOwner::Superuser).await;
    let owner_store = Store::connect(database.db_type(), database.owner_dbconfig())
        .await
        .expect("the owner brings the schema up to date");
    owner_store.close().await;

    for statement in [
        "ALTER TABLE blocks NO FORCE ROW LEVEL SECURITY",
        "ALTER TABLE blocks DISABLE ROW LEVEL SECURITY",
        "DROP POLICY org_wall ON blocks",
        "CREATE POLICY open_wall ON blocks USING (true)",
        "DROP TABLE blocks",
        "CREATE TABLE intruder (id integer)",
    ] {
        let refused = common::try_execute_raw(database.dbconfig(), statement)
            .await
            .expect_err(statement);
        let said = refused.to_string();
        assert!(
            said.contains("must be owner") || said.contains("permission denied"),
            "{statement}: {said}"
        );
    }
    database.remove().await;
}
