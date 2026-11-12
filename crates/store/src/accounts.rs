//! Making an account and letting it in: the one place where a user's row is stored and
//! where a user becomes a member of an organization and of a workspace of it.
//!
//! Whoever signs up with the name of a team, whoever accepts an invitation without an
//! account, and whoever a provider vouches for the first time all get their account
//! here ([`insert_account`]) and are let in here ([`join_organization`],
//! [`join_workspace`]).

use fvoci_model::{OrgRole, User, Workspace, WorkspaceRole};

use crate::Store;
use crate::db::{Id, Scope, Sql, now_millis};
use crate::error::StoreError;
use crate::{users, workspaces};

/// The organization somebody who signs up with the name of a team starts in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Founding<'a> {
    /// The organization of the handle, which is there and nobody's: the one the server
    /// made when it started. Only the first user of the instance takes it.
    First,
    /// A new organization, under the identifier of the handle.
    New {
        /// The identifier of its first workspace.
        workspace_id: &'a str,
    },
}

/// A workspace of the scope's organization that a user is let into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Which<'a> {
    /// The workspace with this identifier.
    Workspace(&'a str),
    /// The organization's default workspace.
    DefaultWorkspace,
}

impl Store {
    /// Stores the account of somebody who signs up with the name of a team, and the
    /// team: the organization of the handle and its first workspace, both under the
    /// name, with the user as the administrator of both. Returns the identifier of the
    /// workspace. All of it happens or none.
    ///
    /// `user` is updated to what was stored, as [`insert_account`] says; whoever is the
    /// first user of the instance is its administrator, however they found their
    /// organization. Who is first is decided with the insertion: of two who sign up at
    /// once into an empty instance, one is.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NotFirstUser`] if [`Founding::First`] is asked and the
    /// instance has had its first user; [`StoreError::Conflict`] if another user,
    /// deleted or not, has the name or the email address in any letter case;
    /// [`StoreError::NoOrganization`] if the handle names no organization; and
    /// [`StoreError::Database`] if a statement fails, as it does for
    /// [`Founding::New`] on an SQLite database, which holds one organization.
    pub async fn sign_up(
        &self,
        user: &mut User,
        team_name: &str,
        founding: Founding<'_>,
    ) -> Result<String, StoreError> {
        self.transaction(async |scope| {
            let is_first = become_first(scope).await?;
            let workspace_id = match founding {
                Founding::First if !is_first => return Err(StoreError::NotFirstUser),
                Founding::First => name_organization(scope, team_name).await?,
                Founding::New { workspace_id } => {
                    found_organization(scope, team_name, workspace_id).await?;
                    workspace_id.to_owned()
                }
            };
            insert_account(scope, user, is_first).await?;
            join_organization(scope, &user.id, OrgRole::Admin).await?;
            join_workspace(
                scope,
                Which::Workspace(&workspace_id),
                &user.id,
                WorkspaceRole::Admin,
            )
            .await?;
            Ok(workspace_id)
        })
        .await
    }

    /// Whether the instance has had its first user.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the query fails.
    pub async fn is_bootstrapped(&self) -> Result<bool, StoreError> {
        self.with_conn(async |scope| {
            let sql = scope.sql("SELECT bootstrapped FROM instance WHERE id = 1");
            match scope.fetch_optional(sql).await? {
                Some(row) => row.bool(0),
                None => Ok(false),
            }
        })
        .await
    }

    /// Stores a user past the ways in, for a test: the first of the instance as its
    /// administrator, and, through a handle that names an organization, as a member of
    /// it and of its default workspace (an administrator of both if of the instance).
    ///
    /// # Errors
    ///
    /// As [`Store::sign_up`] for the account.
    #[cfg(any(test, feature = "test-util"))]
    pub async fn create_user(&self, user: &mut User) -> Result<(), StoreError> {
        self.transaction(async |scope| {
            let is_first = become_first(scope).await?;
            insert_account(scope, user, is_first).await?;
            if scope.has_org() {
                let (org_role, workspace_role) = if is_first {
                    (OrgRole::Admin, WorkspaceRole::Admin)
                } else {
                    (OrgRole::Member, WorkspaceRole::Member)
                };
                join_organization(scope, &user.id, org_role).await?;
                join_workspace(scope, Which::DefaultWorkspace, &user.id, workspace_role).await?;
            }
            Ok(())
        })
        .await
    }
}

/// Turns on the flag that the instance has had its first user, and says whether this
/// transaction did: whoever turns it on is the first user. A second transaction that
/// tries at the same moment waits for this one and then finds the flag on.
async fn become_first(scope: &mut Scope<'_>) -> Result<bool, StoreError> {
    let mut sql = scope.sql("UPDATE instance SET bootstrapped = ");
    sql.bind(true)
        .push_bind(" WHERE id = 1 AND bootstrapped = ", false);
    Ok(scope.execute(sql).await? > 0)
}

/// Stores the row of a new account: `user` with the current time as creation and
/// modification time, not deleted, and an administrator of the instance if `is_first`,
/// whatever the caller put there. Of its other members the identifier, the names, the
/// email address and the password hash are stored.
///
/// # Errors
///
/// Returns [`StoreError::Conflict`] if another user, deleted or not, has the name or
/// the email address in any letter case, and [`StoreError::InvalidId`] if the
/// identifier is not one.
pub(crate) async fn insert_account(
    scope: &mut Scope<'_>,
    user: &mut User,
    is_first: bool,
) -> Result<(), StoreError> {
    let now = now_millis();
    user.create_at = now;
    user.update_at = now;
    user.delete_at = 0;
    user.is_admin = is_first;
    users::insert_user_row(scope, user).await
}

/// Makes the user a member of the scope's organization in `role`, unless they are one,
/// whatever as; says whether they joined. Two at once for the same user both succeed,
/// and one of them joins.
pub(crate) async fn join_organization(
    scope: &mut Scope<'_>,
    user_id: &str,
    role: OrgRole,
) -> Result<bool, StoreError> {
    let mut sql =
        scope.sql("INSERT INTO organization_members (org_id, user_id, role, create_at) VALUES (");
    sql.bind(scope.org()?)
        .push_bind(", ", Id(user_id))
        .push_bind(", ", role.as_str())
        .push_bind(", ", now_millis())
        .push(") ON CONFLICT DO NOTHING");
    Ok(scope.execute(sql).await? > 0)
}

/// Makes the user a member, in `role`, of a workspace in use of the scope's
/// organization, unless they are a member of it, whatever as; says whether they joined.
///
/// # Errors
///
/// Returns [`StoreError::NotFound`] if the organization has no such workspace in use,
/// and [`StoreError::Corrupt`] if it has no default workspace in use, which is broken:
/// nobody is to end up in an organization and in no workspace of it.
pub(crate) async fn join_workspace(
    scope: &mut Scope<'_>,
    which: Which<'_>,
    user_id: &str,
    role: WorkspaceRole,
) -> Result<bool, StoreError> {
    let mut sql = scope.sql(
        "INSERT INTO workspace_members (workspace_id, user_id, role, create_at) SELECT t.id, ",
    );
    sql.bind(Id(user_id))
        .push_bind(", ", role.as_str())
        .push_bind(", ", now_millis())
        .push(" FROM ");
    push_workspace(&mut sql, scope, which)?;
    sql.push(" ON CONFLICT DO NOTHING");
    if scope.execute(sql).await? > 0 {
        return Ok(true);
    }
    // A member already, or no such workspace to be let into.
    let mut sql = scope.sql("SELECT 1 FROM ");
    push_workspace(&mut sql, scope, which)?;
    if scope.fetch_optional(sql).await?.is_some() {
        return Ok(false);
    }
    Err(match which {
        Which::Workspace(_) => StoreError::not_found("workspace"),
        Which::DefaultWorkspace => StoreError::Corrupt {
            column: "default_workspace_id",
            reason: "the organization has no default workspace in use".to_owned(),
        },
    })
}

/// Pushes the workspace `t` that `which` names, of the scope's organization and in use:
/// what follows `FROM`, with its conditions.
fn push_workspace(
    sql: &mut Sql,
    scope: &Scope<'_>,
    which: Which<'_>,
) -> Result<(), StoreError> {
    match which {
        Which::Workspace(workspace_id) => {
            sql.push_bind("workspaces AS t WHERE t.id = ", Id(workspace_id))
                .push_bind(" AND t.org_id = ", scope.org()?);
        }
        Which::DefaultWorkspace => {
            sql.push_bind(
                "organizations AS o JOIN workspaces AS t ON t.id = o.default_workspace_id \
                 WHERE o.id = ",
                scope.org()?,
            );
        }
    }
    sql.push_bind(" AND t.delete_at = ", 0_i64);
    Ok(())
}

/// Gives the scope's organization, which is there and nobody's, the name of the team
/// that takes it, and its default workspace the same; returns that workspace.
async fn name_organization(scope: &mut Scope<'_>, team_name: &str) -> Result<String, StoreError> {
    let now = now_millis();
    let mut sql = scope.sql("UPDATE organizations SET name = ");
    sql.bind(team_name)
        .push_bind(", update_at = ", now)
        .push_bind(" WHERE id = ", scope.org()?)
        .push_bind(" AND delete_at = ", 0_i64);
    if scope.execute(sql).await? == 0 {
        return Err(StoreError::not_found("organization"));
    }
    let mut sql = scope.sql("UPDATE workspaces SET name = ");
    sql.bind(team_name)
        .push_bind(", update_at = ", now)
        .push_bind(
            " WHERE id = (SELECT default_workspace_id FROM organizations WHERE id = ",
            scope.org()?,
        )
        .push_bind(") AND org_id = ", scope.org()?)
        .push_bind(" AND delete_at = ", 0_i64)
        .push(" RETURNING id");
    match scope.fetch_optional(sql).await? {
        Some(row) => row.id(0),
        None => Err(StoreError::Corrupt {
            column: "default_workspace_id",
            reason: "the organization has no default workspace in use".to_owned(),
        }),
    }
}

/// Makes the scope's organization, under the scope's identifier and the name of the
/// team, with its first workspace, which is its default one, under the same name.
async fn found_organization(
    scope: &mut Scope<'_>,
    team_name: &str,
    workspace_id: &str,
) -> Result<(), StoreError> {
    let workspace = Workspace {
        id: workspace_id.to_owned(),
        title: team_name.to_owned(),
        ..Workspace::default()
    };
    let org = scope.org()?;
    workspaces::insert_organization(scope, org, team_name, &workspace).await
}
