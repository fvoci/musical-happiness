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

/// Makes a user a member of the organization and of its default workspace, as a member,
/// unless the user is a member of the organization already; says whether the user
/// joined. Two at once for the same user both succeed, and one of them joins: the
/// second insert finds the first's row and does nothing.
pub(crate) async fn join_organization_unless_member(
    scope: &mut Scope<'_>,
    user_id: &str,
) -> Result<bool, StoreError> {
    let (org_role, workspace_role) = roles_of(InstanceRole::User);
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
