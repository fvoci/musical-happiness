//! Who belongs to an organization and who is in a workspace.
//!
//! Every statement here names the organization itself: the handle's
//! ([`Tenant::Org`](crate::Tenant::Org)).

use fvoci_model::{BoardType, OrgRole, WorkspaceRole};

use crate::db::{Id, Scope, Sql, now_millis};
use crate::error::StoreError;
use crate::users::InstanceRole;
use crate::{Store, org_admin};

/// Opens the condition that the board `b` is in a workspace in use the membership `wm`
/// is of: the caller narrows the memberships and closes the parenthesis. A workspace in
/// the trash has no members to anybody.
const IN_USE_MEMBERSHIPS: &str = " AND b.workspace_id IN (SELECT wm.workspace_id \
     FROM workspace_members AS wm JOIN workspaces AS ws ON ws.id = wm.workspace_id \
     WHERE ws.delete_at = 0";

/// Opens the condition on a board `b` that it is an open board of a workspace the user
/// works in (a member who is no guest: `WorkspaceAccess::works_there`), "or": the
/// caller appends the other way to the board and closes the parenthesis. An open
/// board is no secret to who works in its workspace, and to nobody else.
pub(crate) fn push_open_boards_at_work<'sql>(sql: &'sql mut Sql, user_id: &str) -> &'sql mut Sql {
    sql.push_bind(" AND ((b.type = ", BoardType::Open.as_str())
        .push(IN_USE_MEMBERSHIPS)
        .push_bind(" AND wm.user_id = ", Id(user_id))
        .push_bind(" AND wm.role <> ", WorkspaceRole::Guest.as_str())
        .push(")) OR ")
}

/// The condition that the membership `bm` has a role: one without opens nothing.
pub(crate) const BM_HAS_A_ROLE: &str =
    " AND (bm.scheme_admin OR bm.scheme_editor OR bm.scheme_commenter OR bm.scheme_viewer)";

/// Pushes the condition that the membership `bm` is the user's and that the user is
/// still a member of the board's workspace `b`, a guest too: a role left on a board after
/// leaving its workspace finds nothing.
pub(crate) fn push_own_membership_in_workspace<'sql>(
    sql: &'sql mut Sql,
    user_id: &str,
) -> &'sql mut Sql {
    sql.push_bind("bm.user_id = ", Id(user_id))
        .push(IN_USE_MEMBERSHIPS)
        .push_bind(" AND wm.user_id = ", Id(user_id))
        .push(")")
}

impl Store {
    /// Makes a user a member of the handle's organization.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NoOrganization`] if the handle names no organization, and
    /// [`StoreError::Database`] if the statement fails, as it does for a user who is a
    /// member already or does not exist.
    pub async fn add_organization_member(
        &self,
        user_id: &str,
        role: OrgRole,
    ) -> Result<(), StoreError> {
        self.with_conn(async |scope| add_organization_member(scope, user_id, role).await)
            .await
    }

    /// The identifiers of who works in a workspace of the handle's organization: its
    /// members who are no guests, by identifier.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the query fails.
    pub async fn get_workspace_worker_ids(
        &self,
        workspace_id: &str,
    ) -> Result<Vec<String>, StoreError> {
        self.with_conn(async |scope| {
            let mut sql = scope.sql("SELECT user_id FROM workspace_members WHERE workspace_id = ");
            sql.bind(Id(workspace_id))
                .push_bind(" AND role <> ", WorkspaceRole::Guest.as_str())
                .push(" ORDER BY user_id");
            scope
                .fetch_all(sql)
                .await?
                .iter()
                .map(|row| row.id(0))
                .collect()
        })
        .await
    }

    /// Makes a user a member of a workspace of the handle's organization.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NoOrganization`] if the handle names no organization,
    /// [`StoreError::NotFound`] if the organization has no such workspace, and
    /// [`StoreError::Database`] if the statement fails, as it does for a user who is a
    /// member already or does not exist.
    pub async fn add_workspace_member(
        &self,
        workspace_id: &str,
        user_id: &str,
        role: WorkspaceRole,
    ) -> Result<(), StoreError> {
        self.with_conn(async |scope| {
            let mut sql = scope.sql(
                "INSERT INTO workspace_members (workspace_id, user_id, role, create_at) SELECT id, ",
            );
            sql.bind(Id(user_id))
                .push_bind(", ", role.as_str())
                .push_bind(", ", now_millis())
                .push_bind(" FROM workspaces WHERE id = ", Id(workspace_id))
                .push_bind(" AND org_id = ", scope.org()?)
                .push_bind(" AND delete_at = ", 0_i64);
            if scope.execute(sql).await? == 0 {
                return Err(StoreError::not_found("workspace"));
            }
            Ok(())
        })
        .await
    }
}

async fn add_organization_member(
    scope: &mut Scope<'_>,
    user_id: &str,
    role: OrgRole,
) -> Result<(), StoreError> {
    let mut sql =
        scope.sql("INSERT INTO organization_members (org_id, user_id, role, create_at) VALUES (");
    sql.bind(scope.org()?)
        .push_bind(", ", Id(user_id))
        .push_bind(", ", role.as_str())
        .push_bind(", ", now_millis())
        .push(")");
    scope.execute(sql).await?;
    Ok(())
}

/// The role in an organization that goes with being, or not being, an administrator
/// of the instance.
const fn org_role_of_instance(role: InstanceRole) -> OrgRole {
    match role {
        InstanceRole::Administrator => OrgRole::Admin,
        InstanceRole::User => OrgRole::Member,
    }
}

/// The roles in an organization and in its default workspace of who registers, by what
/// they are to the instance: its first user administers both.
const fn roles_on_registering(role: InstanceRole) -> (OrgRole, WorkspaceRole) {
    match role {
        InstanceRole::Administrator => (OrgRole::Admin, WorkspaceRole::Admin),
        InstanceRole::User => (OrgRole::Member, WorkspaceRole::Member),
    }
}

/// Makes a user a member of the organization and of its default workspace, as an
/// administrator of both if the user is one of the instance: one who registered, or one
/// a provider of the organization let in (`Store::finish_oidc_sign_in`).
///
/// TEMPORARY for registration, with [`Store::only_organization`]: until signing up makes
/// an organization and an invitation leads into one, whoever registers joins the one
/// there is. A provider's newcomers join its organization so for good.
pub(crate) async fn join_organization(
    scope: &mut Scope<'_>,
    user_id: &str,
    instance_role: InstanceRole,
) -> Result<(), StoreError> {
    let (org_role, workspace_role) = roles_on_registering(instance_role);
    add_organization_member(scope, user_id, org_role).await?;
    let mut sql = scope.sql(
        "INSERT INTO workspace_members (workspace_id, user_id, role, create_at) SELECT t.id, ",
    );
    sql.bind(Id(user_id))
        .push_bind(", ", workspace_role.as_str())
        .push_bind(", ", now_millis())
        .push_bind(
            " FROM organizations AS o JOIN workspaces AS t ON t.id = o.default_workspace_id \
             WHERE o.id = ",
            scope.org()?,
        )
        .push_bind(" AND t.delete_at = ", 0_i64);
    // An organization has a default workspace in use; one without is broken, and
    // nobody is to end up in it and in no workspace.
    if scope.execute(sql).await? == 0 {
        return Err(StoreError::Corrupt {
            column: "default_workspace_id",
            reason: "the organization has no default workspace in use".to_owned(),
        });
    }
    Ok(())
}

/// Makes a user a member of the organization and of its default workspace, as a member,
/// unless the user is a member of the organization already; says whether the user
/// joined. Two at once for the same user both succeed, and one of them joins: the
/// second insert finds the first's row and does nothing.
pub(crate) async fn join_organization_unless_member(
    scope: &mut Scope<'_>,
    user_id: &str,
) -> Result<bool, StoreError> {
    let (org_role, workspace_role) = roles_on_registering(InstanceRole::User);
    let mut sql =
        scope.sql("INSERT INTO organization_members (org_id, user_id, role, create_at) VALUES (");
    sql.bind(scope.org()?)
        .push_bind(", ", Id(user_id))
        .push_bind(", ", org_role.as_str())
        .push_bind(", ", now_millis())
        .push(") ON CONFLICT DO NOTHING");
    if scope.execute(sql).await? == 0 {
        return Ok(false);
    }
    let mut sql = scope.sql(
        "INSERT INTO workspace_members (workspace_id, user_id, role, create_at) SELECT t.id, ",
    );
    sql.bind(Id(user_id))
        .push_bind(", ", workspace_role.as_str())
        .push_bind(", ", now_millis())
        .push_bind(
            " FROM organizations AS o JOIN workspaces AS t ON t.id = o.default_workspace_id \
             WHERE o.id = ",
            scope.org()?,
        )
        .push_bind(" AND t.delete_at = ", 0_i64)
        .push(" ON CONFLICT DO NOTHING");
    if scope.execute(sql).await? == 0 {
        // Somebody of the default workspace already, or an organization without one in
        // use, which is broken: nobody is to end up in it and in no workspace.
        let mut sql = scope.sql(
            "SELECT 1 FROM organizations AS o JOIN workspaces AS t \
             ON t.id = o.default_workspace_id WHERE o.id = ",
        );
        sql.bind(scope.org()?)
            .push_bind(" AND t.delete_at = ", 0_i64);
        if scope.fetch_optional(sql).await?.is_none() {
            return Err(StoreError::Corrupt {
                column: "default_workspace_id",
                reason: "the organization has no default workspace in use".to_owned(),
            });
        }
    }
    Ok(true)
}

/// Gives a member of the organization the role in it that goes with being, or no
/// longer being, an administrator of the instance, as the organization's route does
/// (`org_admin::change_org_role`): recorded, and who stops being one is taken out of
/// what they joined as its administrator, on behalf of `actor_id`. No role in a
/// workspace changes. Somebody who is no member stays none.
///
/// TEMPORARY, as [`join_organization`] is: while the instance's administrators are the
/// organization's.
pub(crate) async fn follow_instance_role(
    scope: &mut Scope<'_>,
    user_id: &str,
    instance_role: InstanceRole,
    actor_id: &str,
) -> Result<(), StoreError> {
    let org_role = org_role_of_instance(instance_role);
    let org = org_admin::lock_organization(scope).await?;
    match org_admin::org_role_of(scope, user_id).await? {
        Some(before) if before != org_role => {
            org_admin::change_org_role(scope, &org.id, user_id, org_role, actor_id).await?;
        }
        // Already of that role; one who is no administrator has joined nothing to keep.
        _ if org_role != OrgRole::Admin => {
            org_admin::revoke_joinings(scope, user_id, actor_id).await?;
        }
        _ => {}
    }
    Ok(())
}

/// Whether no other member of the board than the user is an administrator.
pub(crate) async fn is_only_admin(
    scope: &mut Scope<'_>,
    board_id: &str,
    user_id: &str,
) -> Result<bool, StoreError> {
    // Another admin counts who can act as one: a member of the board's workspace who is
    // no guest. A guest is no more than an editor, and who has left opens nothing.
    let mut sql = scope.sql(
        "SELECT 1 FROM board_members AS bm JOIN boards AS b ON b.id = bm.board_id \
         JOIN workspace_members AS wm \
         ON wm.workspace_id = b.workspace_id AND wm.user_id = bm.user_id WHERE bm.board_id = ",
    );
    sql.bind(Id(board_id))
        .push_bind(" AND bm.user_id <> ", Id(user_id))
        .push_bind(" AND bm.scheme_admin = ", true)
        .push_bind(" AND wm.role <> ", WorkspaceRole::Guest.as_str())
        .push(" LIMIT 1");
    Ok(scope.fetch_optional(sql).await?.is_none())
}
