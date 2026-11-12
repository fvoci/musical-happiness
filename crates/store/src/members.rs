//! Who belongs to an organization and who is in a workspace.
//!
//! Every statement here names the organization itself: the handle's
//! ([`Tenant::Org`](crate::Tenant::Org)).

use fvoci_model::{BoardType, OrgRole, WorkspaceRole};

use crate::Store;
use crate::db::{Id, Scope, Sql, now_millis};
use crate::error::StoreError;
use crate::users::InstanceRole;

/// Opens the condition on a board `b` that it is an open board of a workspace the user
/// works in (a member who is no guest: `WorkspaceAccess::works_there`), "or": the
/// caller appends the other way to the board and closes the parenthesis. An open
/// board is no secret to who works in its workspace, and to nobody else.
pub(crate) fn push_open_boards_at_work<'sql>(sql: &'sql mut Sql, user_id: &str) -> &'sql mut Sql {
    sql.push_bind(" AND ((b.type = ", BoardType::Open.as_str())
        .push(" AND b.workspace_id IN (SELECT wm.workspace_id FROM workspace_members AS wm")
        .push_bind(" WHERE wm.user_id = ", Id(user_id))
        .push_bind(" AND wm.role <> ", WorkspaceRole::Guest.as_str())
        .push(")) OR ")
}

/// Pushes the condition that the membership `bm` is the user's and that the user is
/// still a member of the board's workspace `b`, a guest too: a role left on a board after
/// leaving its workspace finds nothing.
pub(crate) fn push_own_membership_in_workspace<'sql>(
    sql: &'sql mut Sql,
    user_id: &str,
) -> &'sql mut Sql {
    sql.push_bind("bm.user_id = ", Id(user_id))
        .push(" AND b.workspace_id IN (SELECT wm.workspace_id FROM workspace_members AS wm")
        .push_bind(" WHERE wm.user_id = ", Id(user_id))
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

/// The roles in an organization and in its workspaces that go with being, or not
/// being, an administrator of the instance.
fn roles_of(role: InstanceRole) -> (OrgRole, WorkspaceRole) {
    match role {
        InstanceRole::Administrator => (OrgRole::Admin, WorkspaceRole::Admin),
        InstanceRole::User => (OrgRole::Member, WorkspaceRole::Member),
    }
}

/// Makes a user who registered a member of the organization and of its default
/// workspace, as an administrator of both if the user is one of the instance.
///
/// TEMPORARY, with [`Store::only_organization`]: until signing up makes an organization
/// and an invitation leads into one, whoever registers joins the one there is.
pub(crate) async fn join_organization(
    scope: &mut Scope<'_>,
    user_id: &str,
    instance_role: InstanceRole,
) -> Result<(), StoreError> {
    let (org_role, workspace_role) = roles_of(instance_role);
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

/// Gives a member of the organization the role that goes with being, or no longer
/// being, an administrator of the instance: in the organization, and in its workspaces
/// where the user is no guest. Somebody who is no member stays none.
///
/// TEMPORARY, as [`join_organization`] is: while the instance's administrators are the
/// organization's.
pub(crate) async fn follow_instance_role(
    scope: &mut Scope<'_>,
    user_id: &str,
    instance_role: InstanceRole,
) -> Result<(), StoreError> {
    let (org_role, workspace_role) = roles_of(instance_role);
    let mut sql = scope.sql("UPDATE organization_members SET role = ");
    sql.bind(org_role.as_str())
        .push_bind(" WHERE user_id = ", Id(user_id))
        .push_bind(" AND org_id = ", scope.org()?);
    scope.execute(sql).await?;
    let mut sql = scope.sql("UPDATE workspace_members SET role = ");
    sql.bind(workspace_role.as_str())
        .push_bind(" WHERE user_id = ", Id(user_id))
        .push_bind(" AND role <> ", WorkspaceRole::Guest.as_str())
        .push_bind(
            " AND workspace_id IN (SELECT id FROM workspaces WHERE org_id = ",
            scope.org()?,
        )
        .push(")");
    scope.execute(sql).await?;
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
