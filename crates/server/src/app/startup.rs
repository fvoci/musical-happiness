//! The store a server starts with: opened, looked at, brought up to date, and reaching
//! the rows of the one organization the routes that name no workspace work in.

use std::time::Duration;

use anyhow::{Context as _, bail};
use fvoci_store::{DbType, Store};

use crate::config::Configuration;

/// Opens the database of the configuration, and looks at the role the server connects
/// as before anything is made as it: a start that is refused leaves the database as it
/// was. Nothing is changed in it until [`prepare_store`].
///
/// # Errors
///
/// Returns an error if the database cannot be reached, and if the role the server
/// connects as is one the wall between organizations does not hold while the
/// configuration takes in several organizations.
pub(crate) async fn open_store(config: &Configuration) -> anyhow::Result<Store> {
    let store = connect(config).await?;
    check_the_wall_holds(&store, config).await?;
    Ok(store)
}

/// Makes the opened database ready for the server, once it holds the lock of the
/// server: brings the schema up to date, and gives the handle that reaches the rows
/// of the one organization the routes that name no workspace work in.
///
/// # Errors
///
/// Returns an error if the database is not this server's to open, or cannot be
/// brought up to date.
pub(crate) async fn prepare_store(store: Store) -> anyhow::Result<Store> {
    store
        .migrate()
        .await
        .context("bringing the database's schema up to date")?;
    only_organization_scope(store).await
}

/// Connects to the database, trying up to `dbpingattempts` times a second apart, so that
/// a server started together with its database waits for it.
async fn connect(config: &Configuration) -> anyhow::Result<Store> {
    let db_type: DbType = config.db_type.parse()?;
    let attempts = config.db_ping_attempts.max(1);
    let mut attempt = 1;
    loop {
        match Store::open(db_type, &config.db_config).await {
            Ok(store) => return Ok(store),
            Err(fvoci_store::StoreError::Database(error)) if attempt < attempts => {
                tracing::warn!(%error, attempt, attempts, "the database is not reachable yet");
                attempt += 1;
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
            Err(error) => return Err(error).context("connecting to the database"),
        }
    }
}

/// Looks at the role the server connects to PostgreSQL as: a superuser, or a role with
/// `BYPASSRLS`, passes the policies that keep the rows of the organizations apart, and
/// the safety net below the server's own checks is not there.
///
/// # Errors
///
/// Fails for such a role if `org_creation` is `open`: a server that takes in several
/// organizations does not start without the net. With the one organization of
/// `first_user_only` it starts, and says so (the default user of a database container
/// is a superuser, and a server set up that way goes on working).
async fn check_the_wall_holds(store: &Store, config: &Configuration) -> anyhow::Result<()> {
    let Some(role) = store
        .role_past_the_wall()
        .await
        .context("reading the role of the database connection")?
    else {
        return Ok(());
    };
    if config.takes_several_organizations() {
        bail!(
            "the database role {role:?} is a superuser or has BYPASSRLS, so the row-level \
             security policies that keep organizations apart do not hold it; with \
             org_creation \"open\" the server connects as a role made for it (LOGIN, \
             NOSUPERUSER, NOBYPASSRLS) that owns its tables"
        );
    }
    tracing::warn!(
        role,
        "the database role is a superuser or has BYPASSRLS: the row-level security \
         policies that keep organizations apart do not hold it. The server works; the \
         safety net below its own checks is off"
    );
    Ok(())
}

/// A handle to the store that reaches the rows of the one organization the server
/// works in: the oldest, made now, with its first workspace, if the database has none.
///
/// TEMPORARY. Every route that names no workspace works in this organization, as do
/// the websocket connections. A route that names its workspace gets its organization
/// from there; when all do, this function and its one call go. On PostgreSQL what is
/// left without an organization then shows by reaching nothing; SQLite has no wall, and
/// shows only the statements that name the organization themselves (the workspaces and the
/// members), so that run tells less. The first organization of an empty database is
/// made here too, and needs another place then.
async fn only_organization_scope(store: Store) -> anyhow::Result<Store> {
    // The first workspace of an organization that is made now: the root workspace. It
    // and its organization are nobody's, and unnamed, until the first user signs up
    // with the name of their organization (`App::sign_up`).
    let workspace = fvoci_model::Workspace {
        id: fvoci_model::new_id(),
        ..fvoci_model::Workspace::default()
    };
    let org_id = store
        .only_organization(&workspace)
        .await
        .context("unable to get the organization")?;
    store
        .for_org(&org_id)
        .context("the organization's identifier")
}
