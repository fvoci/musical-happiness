//! What a transaction of one organization asks of all of them: one read each, with the
//! wall of the transaction opened for that read and closed again before anything else
//! runs in it. Each read here names what it asks for (the oldest organization, the
//! organizations one account made), and nothing is written past the wall.
//!
//! The opening and the closing are here too ([`open_wall`], [`close_wall`]), for the
//! one other place that opens a transaction's wall: a registration's statement on the
//! one link it was made by (`signups.rs`).

use fvoci_model::OrgKind;

use super::oldest_organization;
use crate::db::{Dialect, Id, Scope, Sql};
use crate::error::StoreError;

/// What the reach of a transaction was before its wall was opened.
#[must_use = "the wall is closed again with `close_wall`"]
pub(crate) struct OpenedWall(String);

/// Opens the wall of the transaction of `scope`, on PostgreSQL: the rows of every
/// organization are in its reach until [`close_wall`] puts the reach back as it was,
/// which is not always closed: a transaction that opened it before keeps it open.
/// Only inside a transaction, whose setting the reach is.
///
/// # Errors
///
/// Returns [`StoreError::Database`] if the reach cannot be read or changed.
pub(crate) async fn open_wall(scope: &mut Scope<'_>) -> Result<OpenedWall, StoreError> {
    let sql = scope.sql("SELECT COALESCE(current_setting('fvoci.system', true), 'off')");
    let before = match scope.fetch_optional(sql).await? {
        Some(row) => row.text(0)?,
        None => "off".to_owned(),
    };
    set_reach(scope, "on").await?;
    Ok(OpenedWall(before))
}

/// Puts the reach of the transaction back as it was before [`open_wall`].
///
/// # Errors
///
/// Returns [`StoreError::Database`] if the reach cannot be changed.
pub(crate) async fn close_wall(scope: &mut Scope<'_>, wall: OpenedWall) -> Result<(), StoreError> {
    set_reach(scope, &wall.0).await
}

async fn set_reach(scope: &mut Scope<'_>, reach: &str) -> Result<(), StoreError> {
    let mut sql = scope.sql("SELECT set_config('fvoci.system', ");
    sql.bind(reach).push(", true)");
    scope.fetch_optional(sql).await?;
    Ok(())
}

/// Runs the one read `read` with the rows of every organization in reach, in the
/// transaction of `scope`, which is one of an organization, and puts the reach back
/// as it was. On SQLite, which has no wall, `read` runs as it is. Only inside a
/// transaction, whose setting the reach is.
///
/// # Errors
///
/// Returns [`StoreError::NoOrganization`] for a scope that names no organization, on
/// either database: what such a scope reaches is not this function's to change, and
/// a read that it could not make would answer as if there were nothing to find.
/// Returns the error of `read`, and [`StoreError::Database`] if the reach cannot be
/// changed. It is put back whatever `read` answered; where that fails after `read`
/// failed, the error of `read` is the one returned, and the transaction is one
/// PostgreSQL takes nothing more in.
async fn past_the_wall<T>(
    scope: &mut Scope<'_>,
    read: impl AsyncFnOnce(&mut Scope<'_>) -> Result<T, StoreError>,
) -> Result<T, StoreError> {
    if !scope.has_org() {
        return Err(StoreError::NoOrganization);
    }
    if scope.dialect() != Dialect::Postgres {
        return read(scope).await;
    }
    let wall = open_wall(scope).await?;
    let result = read(scope).await;
    let closed = close_wall(scope, wall).await;
    match (result, closed) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(error), _) | (Ok(_), Err(error)) => Err(error),
    }
}

/// Whether the organization of `scope` is the instance's own: the oldest one in use,
/// whose administrators are the instance's (`members::follow_instance_role`). Asked in
/// the caller's transaction, so that the answer holds for what it does; the
/// organization's wall hides the others, and is opened for this one read.
pub(crate) async fn is_the_instances_organization(
    scope: &mut Scope<'_>,
) -> Result<bool, StoreError> {
    let Some(org) = scope.org_id() else {
        return Ok(false);
    };
    let oldest = past_the_wall(scope, async |scope| oldest_organization(scope).await).await?;
    Ok(oldest.is_some_and(|id| id == org))
}

/// How many organizations in use the account `user_id` made, that people work in
/// together: by signing up with the name of one, or later. Of all, which the wall of
/// the caller's transaction hides and is opened for this one read. Not the ones it was
/// let into, and not its own space.
///
/// # Errors
///
/// Returns [`StoreError::NoOrganization`] for a scope that names no organization: the
/// count is asked by a transaction that makes one, and nothing is counted as none
/// for want of reach.
pub(crate) async fn organizations_made_by(
    scope: &mut Scope<'_>,
    user_id: &str,
) -> Result<i64, StoreError> {
    past_the_wall(scope, async |scope| {
        let mut sql = scope.sql("SELECT COUNT(*) FROM organizations WHERE created_by = ");
        sql.bind(Id(user_id))
            .push(" AND delete_at = 0 AND is_system = false")
            .push_bind(" AND kind = ", OrgKind::Team.as_str());
        match scope.fetch_optional(sql).await? {
            Some(row) => row.int(0),
            None => Ok(0),
        }
    })
    .await
}

/// Appends the condition that `kind`, the column of an organization's kind, says one
/// that people work in together. Written into the statement and not bound: the index
/// of those organizations by age (0051) holds no account's own space, and a database
/// reads it only for a statement that says the kind itself. For the reads that look
/// for the oldest of them among however many spaces the accounts have.
pub(crate) fn of_people(sql: &mut Sql, kind: &str) {
    sql.push(" AND ")
        .push(kind)
        .push(" = '")
        .push(OrgKind::Team.as_str())
        .push("'");
}
