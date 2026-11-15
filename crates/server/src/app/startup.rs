//! The store a server starts with: opened, looked at, brought up to date, and reaching
//! no organization until a request identifies one.

use std::time::Duration;

use anyhow::{Context as _, bail};
use fvoci_store::{DbType, Store, StoreError};

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
/// server: brings the schema up to date without choosing or creating an organization.
///
/// # Errors
///
/// Returns an error if the database is not this server's to open, or cannot be
/// brought up to date.
pub(crate) async fn prepare_store(store: Store) -> anyhow::Result<Store> {
    match store.migrate().await {
        Ok(()) => Ok(store),
        Err(error @ StoreError::SchemaNotOwned { .. }) => Err(anyhow::Error::from(error).context(
            "the server does not start: run `fvoci-server --migrate` first, with the \
                 dbconfig of the role that owns the tables",
        )),
        Err(error) => Err(error).context("bringing the database's schema up to date"),
    }
}

/// Brings the database's schema up to date and ends: `fvoci-server --migrate`, run as
/// the role that owns the tables when the server connects as one that does not. It
/// holds the lock of the server meanwhile, so that no server serves the database while
/// its tables change, and it does not look at the wall: the owner that migrates may be a
/// superuser.
///
/// # Errors
///
/// Returns an error if the database cannot be reached, another process serves it for
/// longer than `server_lock_wait_seconds`, or the schema cannot be brought up to date.
pub async fn migrate_database(config: &Configuration) -> anyhow::Result<()> {
    let store = connect(config).await?;
    let lock = match super::take_server_lock(&store, config).await {
        Ok(lock) => lock,
        Err(error) => {
            store.close().await;
            return Err(error);
        }
    };
    let migrated = store.migrate().await;
    let whereabouts = store.role_and_schema().await;
    // As a server ends: the database is closed before its lock is let go.
    store.close().await;
    lock.release().await;
    migrated.context("bringing the database's schema up to date")?;
    let (role, schema) = whereabouts.unwrap_or_default();
    tracing::info!(role, schema, "the database's schema is up to date");
    Ok(())
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
