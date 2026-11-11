//! The setting `org_creation` and the check, at the start, of the role the server
//! connects to PostgreSQL as: a server that takes in several organizations does not
//! start as a role that the wall between them does not hold.

use fvoci_model::BoardType;
use fvoci_server::config::Configuration;
use fvoci_store::DbType;
use fvoci_store::testing::db_type_of_the_tests;

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
