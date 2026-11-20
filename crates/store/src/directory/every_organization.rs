//! What a transaction of one organization asks of all of them: one read each, with the
//! wall of the transaction opened for that read and closed again before anything else
//! runs in it. Each read here names what it asks for (the oldest organization, the
//! organizations one account made), and nothing is written past the wall.

use fvoci_model::OrgKind;

use super::oldest_organization;
use crate::db::{Dialect, Id, Scope};
use crate::error::StoreError;

/// Runs the one read `read` with the rows of every organization in reach, in the
/// transaction of `scope`, which is one of an organization, and closes the wall again.
/// On SQLite, which has no wall, `read` runs as it is; so it does for a scope that
/// names no organization, whose reach is not this function's to change. Only inside a
/// transaction, whose setting the reach is.
///
/// # Errors
///
/// Returns the error of `read`, and [`StoreError::Database`] if the reach cannot be
/// changed. It is put back whatever `read` answered; where that fails after `read`
/// failed, the error of `read` is the one returned, and the transaction is one
/// PostgreSQL takes nothing more in.
async fn past_the_wall<T>(
    scope: &mut Scope<'_>,
    read: impl AsyncFnOnce(&mut Scope<'_>) -> Result<T, StoreError>,
) -> Result<T, StoreError> {
    if scope.dialect() != Dialect::Postgres || !scope.has_org() {
        return read(scope).await;
    }
    let reach = |on: &str| format!("SELECT set_config('fvoci.system', '{on}', true)");
    let sql = scope.sql(&reach("on"));
    scope.fetch_optional(sql).await?;
    let result = read(scope).await;
    let sql = scope.sql(&reach("off"));
    let closed = scope.fetch_optional(sql).await;
    match (result, closed) {
        (Ok(value), Ok(_)) => Ok(value),
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
