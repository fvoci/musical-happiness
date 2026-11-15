//! A server that connects to PostgreSQL as a role that does not own the tables: their
//! owner, the database's administrator, brings the schema up to date with
//! `fvoci-server --migrate` (`fvoci_server::app::migrate_database`), and the server only
//! uses them. It serves; it does not start on a schema that is behind it, and says what
//! to run; and the migration takes the lock of the server, as a start does.

use std::path::PathBuf;

use fvoci_model::{Board, BoardType, Card};
use fvoci_server::Server;
use fvoci_server::config::Configuration;
use fvoci_store::DbType;
use fvoci_store::testing::{
    TableOwner, TestDatabase, db_type_of_the_tests, forget_migration, latest_migration,
    migrate_up_to,
};

use crate::boards::new_board;
use crate::common::{Setup, TEST_WORKSPACE_ID, TEST_WS};
use crate::content_search::new_card;

/// A board, and a card with a body, through the API of a server whose role does not own
/// the tables, whoever owns them.
#[tokio::test]
async fn a_server_serves_tables_its_role_does_not_own() {
    for owner in [TableOwner::Superuser, TableOwner::Role] {
        let th = Setup::one_user()
            .connecting_as_a_role_that_does_not_own_the_tables(owner)
            .start()
            .await;
        let board: Board = th
            .client
            .create_board(&new_board(TEST_WORKSPACE_ID, BoardType::Open, "Apart"))
            .await
            .created_json();
        let card = new_card(
            &th.client,
            &board.id,
            "Owned apart",
            "written by the server",
        )
        .await;
        let read: Card = th
            .client
            .get(&format!("{TEST_WS}/cards/{}", card.id))
            .await
            .ok_json();
        assert_eq!(read.title, "Owned apart", "{owner:?}");
        th.teardown().await;
    }
}

/// A server whose role does not own the tables does not start on a schema that is
/// behind it, and names the migration to run; the owner's migration takes the lock of
/// the server, and the server then starts. While it serves, a migration of a schema
/// that is up to date does nothing, and one of a schema behind is refused. Whoever owns
/// the tables.
#[tokio::test]
async fn a_server_waits_for_the_owner_to_migrate_a_schema_behind_it() {
    if db_type_of_the_tests() != DbType::Postgres {
        return;
    }
    for owner in [TableOwner::Superuser, TableOwner::Role] {
        the_owner_migrates_while_no_server_serves(owner).await;
    }
}

async fn the_owner_migrates_while_no_server_serves(owner: TableOwner) {
    let directory = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("api-tests")
        .join(fvoci_model::new_id());
    let database = TestDatabase::create_owned_apart(&directory, owner).await;
    migrate_up_to(
        database.db_type(),
        database.owner_dbconfig(),
        latest_migration() - 1,
    )
    .await;
    let config = |dbconfig: &str| Configuration {
        db_type: database.db_type().as_str().to_owned(),
        db_config: dbconfig.to_owned(),
        files_path: directory.join("uploads").display().to_string(),
        skip_template_init: true,
        server_lock_wait_seconds: 0,
        ..Configuration::default()
    };

    let Err(refused) = Server::new(config(database.dbconfig()), "").await else {
        panic!("the server started on a schema behind it");
    };
    let refusal = format!("{refused:#}");
    assert!(
        refusal.contains("fvoci-server --migrate"),
        "{owner:?}: {refusal}"
    );

    fvoci_server::app::migrate_database(&config(database.owner_dbconfig()))
        .await
        .expect("the owner brings the schema up to date");
    let server = Server::new(config(database.dbconfig()), "")
        .await
        .expect("then the server starts");

    // Up to date, a migration leaves the schema alone while the server serves it, as a
    // second `up` of the compose file asks for one.
    fvoci_server::app::migrate_database(&config(database.owner_dbconfig()))
        .await
        .expect("nothing to do while the server serves the database");
    // Behind, it waits for the lock the server holds, and gives up.
    forget_migration(
        database.db_type(),
        database.owner_dbconfig(),
        latest_migration(),
    )
    .await;
    let busy = fvoci_server::app::migrate_database(&config(database.owner_dbconfig()))
        .await
        .expect_err("no migration while a server serves the database");
    let said = format!("{busy:#}");
    assert!(
        said.contains("another process is the server") && said.contains("stop it"),
        "{owner:?}: {said}"
    );
    server.shutdown().await;
    database.remove().await;
}
