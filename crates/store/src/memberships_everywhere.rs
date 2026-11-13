//! A user's board memberships in every organization, as the session that asks opens
//! them.

use fvoci_model::BoardMember;

use crate::db::{Id, Scope, Sql};
use crate::directory::IN_USE;
use crate::error::StoreError;
use crate::{board_team_access, boards};

/// The user's board memberships in live workspaces of every organization, direct
/// and through a team (`inherited_role`), ordered by board identifier, read through
/// `scope`, which reaches every organization (`Store::board_memberships_of_user`).
pub(crate) async fn of_user(
    scope: &mut Scope<'_>,
    user_id: &str,
    login_provider: Option<&str>,
) -> Result<Vec<BoardMember>, StoreError> {
    let mut sql = boards::select_members(scope);
    sql.push(
        " JOIN boards AS b ON b.id = bm.board_id \
         JOIN workspaces AS t ON t.id = b.workspace_id \
         JOIN organizations AS o ON o.id = t.org_id \
         JOIN workspace_members AS wm ON wm.workspace_id = t.id AND wm.user_id = bm.user_id \
         WHERE bm.user_id = ",
    )
    .bind(Id(user_id))
    .push(IN_USE)
    .push(&scope.same_organization(&[("bm", "b"), ("b", "t"), ("wm", "t")]));
    push_opened_by(&mut sql, login_provider);
    sql.push(" ORDER BY bm.board_id");
    let mut members = boards::fetch_members(scope, sql).await?;
    board_team_access::extend_across_organizations(scope, user_id, login_provider, &mut members)
        .await?;
    Ok(members)
}

/// Leaves out the workspaces `t` of an organization that takes only sessions made
/// through its provider, unless the session was made through `login_provider`.
pub(crate) fn push_opened_by(sql: &mut Sql, login_provider: Option<&str>) {
    sql.push(
        " AND NOT EXISTS (SELECT 1 FROM oidc_providers AS p \
         WHERE p.org_id = t.org_id AND p.sso_required = ",
    )
    .bind(true);
    if let Some(provider) = login_provider {
        sql.push_bind(" AND p.id <> ", Id(provider));
    }
    sql.push(")");
}
