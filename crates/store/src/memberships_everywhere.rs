//! A user's board memberships in every organization, as the session that asks opens
//! them.

use fvoci_model::{ContainerKinds, ContainerMember, OrgKind};

use crate::db::{Id, Scope, Sql};
use crate::directory::IN_USE;
use crate::error::StoreError;
use crate::{container_kinds, container_team_access, containers};

/// The user's memberships of boards of `kinds` in live workspaces of every
/// organization, direct and through a team (`inherited_role`), ordered by board
/// identifier, read through `scope`, which reaches every organization
/// (`Store::container_memberships_of_user`).
pub(crate) async fn of_user(
    scope: &mut Scope<'_>,
    user_id: &str,
    login_provider: Option<&str>,
    kinds: ContainerKinds,
) -> Result<Vec<ContainerMember>, StoreError> {
    let mut sql = containers::select_members(scope);
    sql.push(
        " JOIN containers AS b ON b.id = bm.container_id \
         JOIN workspaces AS t ON t.id = b.workspace_id \
         JOIN organizations AS o ON o.id = t.org_id \
         JOIN workspace_members AS wm ON wm.workspace_id = t.id AND wm.user_id = bm.user_id \
         WHERE bm.user_id = ",
    )
    .bind(Id(user_id))
    .push(IN_USE)
    .push(&scope.same_organization(&[("bm", "b"), ("b", "t"), ("wm", "t")]));
    container_kinds::push_kinds(&mut sql, "b.kind", kinds);
    push_opened_by(&mut sql, user_id, login_provider);
    sql.push(" ORDER BY bm.container_id");
    let mut members = containers::fetch_members(scope, sql).await?;
    extend_across_organizations(scope, user_id, login_provider, kinds, &mut members).await?;
    Ok(members)
}

/// Adds to `members` the boards `user_id` has a role on through a team, of every
/// organization, as `Store::container_memberships_of_user` lists the direct ones: of a
/// workspace and an organization in use, that the session made through
/// `login_provider` opens. Sorted by board.
async fn extend_across_organizations(
    scope: &mut Scope<'_>,
    user_id: &str,
    login_provider: Option<&str>,
    kinds: ContainerKinds,
    members: &mut Vec<ContainerMember>,
) -> Result<(), StoreError> {
    let mut sql = scope.sql(&format!(
        "SELECT bt.container_id, tm.user_id, bt.role FROM container_teams AS bt \
         JOIN {} tm ON tm.team_id = bt.team_id AND tm.org_id = bt.org_id \
         AND tm.workspace_id = bt.workspace_id \
         JOIN containers AS b ON b.id = bt.container_id \
         JOIN workspaces AS t ON t.id = b.workspace_id \
         JOIN organizations AS o ON o.id = t.org_id WHERE tm.user_id = ",
        crate::teams::present_members(scope)
    ));
    sql.bind(Id(user_id))
        .push(IN_USE)
        .push(&scope.same_organization(&[("bt", "b"), ("b", "t")]));
    container_kinds::push_kinds(&mut sql, "b.kind", kinds);
    push_opened_by(&mut sql, user_id, login_provider);
    container_team_access::extend(scope, sql, members).await?;
    members.sort_by(|a, b| a.container_id.cmp(&b.container_id));
    Ok(())
}

/// Leaves out the workspaces `t`, of the organizations `o`, that a session of the user
/// made through `login_provider` (`None` for a password) does not open, as
/// [`crate::SessionTaken::opened_by`] has it: those of an organization that takes only
/// sessions made through another provider, and, for a session made through any, the
/// user's own space once the account has a password.
fn push_opened_by(sql: &mut Sql, user_id: &str, login_provider: Option<&str>) {
    sql.push(
        " AND NOT EXISTS (SELECT 1 FROM oidc_providers AS p \
         WHERE p.org_id = t.org_id AND p.sso_required = ",
    )
    .bind(true);
    if let Some(provider) = login_provider {
        sql.push_bind(" AND p.id <> ", Id(provider));
    }
    sql.push(")");
    if login_provider.is_some() {
        sql.push_bind(" AND NOT (o.kind = ", OrgKind::Personal.as_str())
            .push_bind(
                " AND EXISTS (SELECT 1 FROM users AS u WHERE u.id = ",
                Id(user_id),
            )
            .push(" AND u.password_hash <> ''))");
    }
}
