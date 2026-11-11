//! Who belongs to an organization and who is in a workspace.
//!
//! Every statement here names the organization itself: the handle's
//! ([`Tenant::Org`](crate::Tenant::Org)).

use fvoci_model::{OrgRole, WorkspaceRole};

use crate::Store;
use crate::db::{Id, Scope, now_millis};
use crate::error::StoreError;
use crate::users::InstanceRole;

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
                .push_bind(" FROM teams WHERE id = ", Id(workspace_id))
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
            " FROM organizations AS o JOIN teams AS t ON t.id = o.default_workspace_id \
             WHERE o.id = ",
            scope.org()?,
        )
        .push_bind(" AND t.delete_at = ", 0_i64);
    scope.execute(sql).await?;
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
            " AND workspace_id IN (SELECT id FROM teams WHERE org_id = ",
            scope.org()?,
        )
        .push(")");
    scope.execute(sql).await?;
    Ok(())
}
