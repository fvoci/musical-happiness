//! The setting `org_creation` and the check, at the start, of the role the server
//! connects to PostgreSQL as: a server that takes in several organizations does not
//! start as a role that the wall between them does not hold.

use fvoci_model::BoardType;
use fvoci_server::Server;
use fvoci_server::config::Configuration;
use fvoci_store::testing::{TestDatabase, db_type_of_the_tests};
use fvoci_store::{DbType, Store};

use crate::common::{ROOT_TEAM_ID, Setup};

fn open(config: &mut Configuration) {
    "open".clone_into(&mut config.org_creation);
}

/// Why a server with this setup does not start.
async fn refusal(setup: Setup) -> String {
    match setup.try_start().await {
        Ok(server) => {
            server.teardown().await;
            panic!("the server started");
        }
        Err(error) => format!("{error:#}"),
    }
}

/// With `org_creation` set to `open` the server does not start as a superuser, whom no
/// policy holds, and says why; as a role the policies hold, it starts.
#[tokio::test]
async fn a_server_for_several_organizations_does_not_start_past_the_wall() {
    if db_type_of_the_tests() != DbType::Postgres {
        return;
    }
    let refusal = refusal(Setup::native().connecting_as_superuser().configure(open)).await;
    assert!(
        refusal.contains("superuser") && refusal.contains("org_creation"),
        "the refusal says why: {refusal}"
    );

    let ts = Setup::native().configure(open).start().await;
    ts.teardown().await;
}

/// A start that is refused over the role leaves the database as it was: nothing is
/// made as the role that was refused, which would own it.
#[tokio::test]
async fn a_start_refused_over_the_role_makes_nothing_in_the_database() {
    if db_type_of_the_tests() != DbType::Postgres {
        return;
    }
    let directory = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("api-tests")
        .join(fvoci_model::new_id());
    let database = TestDatabase::create_as_superuser(&directory).await;
    let config = Configuration {
        db_type: database.db_type().as_str().to_owned(),
        db_config: database.dbconfig().to_owned(),
        org_creation: "open".to_owned(),
        files_path: directory.join("uploads").display().to_string(),
        skip_template_init: true,
        ..Configuration::default()
    };
    let refused = Server::new(config, "")
        .await
        .expect_err("the server does not start as a superuser");
    assert!(format!("{refused:#}").contains("superuser"), "{refused:#}");

    let store = Store::open(database.db_type(), database.dbconfig())
        .await
        .expect("the database is reached");
    let no_table = store
        .count_rows("_sqlx_migrations")
        .await
        .expect_err("no migration ran");
    assert!(
        no_table.to_string().contains("_sqlx_migrations"),
        "the table is not there: {no_table}"
    );
    store.close().await;
    database.remove().await;
}

/// With the one organization of the default setting the server starts as a superuser
/// too, as a database container sets it up, and works: the wall is not there, the
/// server's own checks are.
#[tokio::test]
async fn a_server_of_one_organization_starts_past_the_wall_and_works() {
    if db_type_of_the_tests() != DbType::Postgres {
        return;
    }
    let ts = Setup::native()
        .connecting_as_superuser()
        .start()
        .await
        .init_basic()
        .await;
    let private = ts.create_board(ROOT_TEAM_ID, BoardType::Private).await;
    ts.client
        .get(&format!("/boards/{}", private.id))
        .await
        .check_ok();
    // The second user is no member of the board: the server says so, wall or no wall.
    ts.client2
        .get(&format!("/boards/{}", private.id))
        .await
        .check_not_found();
    ts.teardown().await;
}

/// `open` needs PostgreSQL: an SQLite database holds one organization. Any other word
/// than the two is refused on either database.
#[tokio::test]
async fn org_creation_is_one_of_two_words_and_open_needs_postgresql() {
    if db_type_of_the_tests() == DbType::Sqlite {
        let refusal = refusal(Setup::native().configure(open)).await;
        assert!(
            refusal.contains("org_creation") && refusal.contains("postgres"),
            "the refusal says why: {refusal}"
        );
    }
    let refusal = refusal(Setup::native().configure(|config| {
        "everybody".clone_into(&mut config.org_creation);
    }))
    .await;
    assert!(
        refusal.contains("org_creation") && refusal.contains("first_user_only"),
        "the refusal names the words: {refusal}"
    );
}
