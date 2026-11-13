//! Groups of nonguest workspace members. Writes share the organization/workspace
//! locks with membership changes, and recheck the administrator under those locks.

use fvoci_model::{Team, TeamMember, new_id};

use crate::db::{Id, Scope};
use crate::{Store, StoreError, Taken, org_admin, workspace_membership};

impl Store {
    /// Lists the workspace's teams and their current nonguest members.
    ///
    /// # Errors
    /// Returns the database error or `NoOrganization` for an unscoped handle.
    pub async fn teams(&self, workspace_id: &str) -> Result<Vec<Team>, StoreError> {
        self.with_conn(async |scope| list(scope, workspace_id).await)
            .await
    }

    /// Creates an empty team; its name is unique in the workspace.
    ///
    /// # Errors
    /// Returns a conflict for a duplicate name, not found when the workspace or its
    /// administrator is no longer present, or the database error.
    pub async fn create_team(
        &self,
        workspace_id: &str,
        name: &str,
        by: &str,
    ) -> Result<String, StoreError> {
        self.transaction(async |scope| {
            managing(scope, workspace_id, by).await?;
            let id = new_id();
            let mut sql = scope.sql("INSERT INTO teams (id, org_id, workspace_id, name) VALUES (");
            sql.bind(Id(&id))
                .push_bind(", ", scope.org()?)
                .push_bind(", ", Id(workspace_id))
                .push_bind(", ", name)
                .push(")");
            scope.execute(sql).await.map_err(name_error)?;
            Ok(id)
        })
        .await
    }

    /// Renames a team within its workspace.
    ///
    /// # Errors
    /// As [`Store::create_team`]; also not found for a team outside the workspace.
    pub async fn rename_team(
        &self,
        workspace_id: &str,
        team_id: &str,
        name: &str,
        by: &str,
    ) -> Result<(), StoreError> {
        self.transaction(async |scope| {
            managing(scope, workspace_id, by).await?;
            let mut sql = scope.sql("UPDATE teams SET name = ");
            sql.bind(name)
                .push_bind(" WHERE id = ", Id(team_id))
                .push_bind(" AND workspace_id = ", Id(workspace_id))
                .push_bind(" AND org_id = ", scope.org()?);
            if scope.execute(sql).await.map_err(name_error)? == 0 {
                return Err(StoreError::not_found("team"));
            }
            Ok(())
        })
        .await
    }

    /// Deletes a team, its members, and every role it was given on a board.
    ///
    /// # Errors
    /// As [`Store::rename_team`].
    pub async fn delete_team(
        &self,
        workspace_id: &str,
        team_id: &str,
        by: &str,
    ) -> Result<(), StoreError> {
        self.transaction(async |scope| {
            managing(scope, workspace_id, by).await?;
            let mut sql = scope.sql("DELETE FROM teams WHERE id = ");
            sql.bind(Id(team_id))
                .push_bind(" AND workspace_id = ", Id(workspace_id))
                .push_bind(" AND org_id = ", scope.org()?);
            if scope.execute(sql).await? == 0 {
                return Err(StoreError::not_found("team"));
            }
            Ok(())
        })
        .await
    }

    /// Adds a nonguest workspace member to a team, idempotently.
    ///
    /// # Errors
    /// As [`Store::rename_team`]; not found for a guest or a person outside the workspace.
    pub async fn add_team_member(
        &self,
        workspace_id: &str,
        team_id: &str,
        user_id: &str,
        by: &str,
    ) -> Result<(), StoreError> {
        self.transaction(async |scope| {
            managing(scope, workspace_id, by).await?;
            require_team(scope, workspace_id, team_id).await?;
            let mut sql = scope.sql("SELECT user_id FROM workspace_members WHERE workspace_id = ");
            sql.bind(Id(workspace_id))
                .push_bind(" AND user_id = ", Id(user_id))
                .push(" AND role IN ('admin', 'member')");
            if scope.fetch_optional(sql).await?.is_none() {
                return Err(StoreError::not_found("workspace member"));
            }
            let mut sql = scope
                .sql("INSERT INTO team_members (team_id, org_id, workspace_id, user_id) VALUES (");
            sql.bind(Id(team_id))
                .push_bind(", ", scope.org()?)
                .push_bind(", ", Id(workspace_id))
                .push_bind(", ", Id(user_id))
                .push(") ON CONFLICT (team_id, user_id, org_id) DO NOTHING");
            scope.execute(sql).await?;
            Ok(())
        })
        .await
    }

    /// Removes a person from a team, idempotently.
    ///
    /// # Errors
    /// As [`Store::rename_team`].
    pub async fn remove_team_member(
        &self,
        workspace_id: &str,
        team_id: &str,
        user_id: &str,
        by: &str,
    ) -> Result<(), StoreError> {
        self.transaction(async |scope| {
            managing(scope, workspace_id, by).await?;
            require_team(scope, workspace_id, team_id).await?;
            let mut sql = scope.sql("DELETE FROM team_members WHERE team_id = ");
            sql.bind(Id(team_id))
                .push_bind(" AND workspace_id = ", Id(workspace_id))
                .push_bind(" AND user_id = ", Id(user_id))
                .push_bind(" AND org_id = ", scope.org()?);
            scope.execute(sql).await?;
            Ok(())
        })
        .await
    }
}

/// Team writes and workspace membership changes take these locks in the same order.
async fn managing(scope: &mut Scope<'_>, workspace_id: &str, by: &str) -> Result<(), StoreError> {
    org_admin::hold_organization_row(scope).await?;
    if !workspace_membership::hold_workspace(scope, workspace_id).await? {
        return Err(StoreError::not_found("workspace"));
    }
    let mut sql = scope.sql("SELECT user_id FROM workspace_members WHERE workspace_id = ");
    sql.bind(Id(workspace_id))
        .push_bind(" AND user_id = ", Id(by))
        .push(" AND role = 'admin'");
    if scope.fetch_optional(sql).await?.is_none() {
        return Err(StoreError::not_found("workspace"));
    }
    Ok(())
}

pub(crate) async fn require_team(
    scope: &mut Scope<'_>,
    workspace_id: &str,
    team_id: &str,
) -> Result<(), StoreError> {
    let mut sql = scope.sql("SELECT id FROM teams WHERE id = ");
    sql.bind(Id(team_id))
        .push_bind(" AND workspace_id = ", Id(workspace_id))
        .push_bind(" AND org_id = ", scope.org()?);
    if scope.fetch_optional(sql).await?.is_none() {
        return Err(StoreError::not_found("team"));
    }
    Ok(())
}

/// Demotion removes the stored links, so promoting the person later restores none.
pub(crate) async fn remove_workspace_teams(
    scope: &mut Scope<'_>,
    workspace_id: &str,
    user_id: &str,
) -> Result<(), StoreError> {
    let mut sql = scope.sql("DELETE FROM team_members WHERE workspace_id = ");
    sql.bind(Id(workspace_id))
        .push_bind(" AND user_id = ", Id(user_id))
        .push_bind(" AND org_id = ", scope.org()?);
    scope.execute(sql).await?;
    Ok(())
}

async fn list(scope: &mut Scope<'_>, workspace_id: &str) -> Result<Vec<Team>, StoreError> {
    let mut sql = scope.sql("SELECT t.id, t.name FROM teams t JOIN workspaces w ON w.id = t.workspace_id AND w.org_id = t.org_id WHERE t.workspace_id = ");
    sql.bind(Id(workspace_id))
        .push_bind(" AND t.org_id = ", scope.org()?)
        .push(" AND w.delete_at = 0 ORDER BY t.name, t.id");
    let rows = scope.fetch_all(sql).await?;
    let mut teams = rows
        .iter()
        .map(|row| {
            Ok(Team {
                id: row.id(0)?,
                workspace_id: workspace_id.to_owned(),
                name: row.text(1)?,
                members: Vec::new(),
            })
        })
        .collect::<Result<Vec<_>, StoreError>>()?;
    let mut sql = scope.sql("SELECT tm.team_id, u.id, u.username, u.display_name FROM team_members tm JOIN workspace_members wm ON wm.workspace_id = tm.workspace_id AND wm.user_id = tm.user_id JOIN users u ON u.id = tm.user_id WHERE tm.workspace_id = ");
    sql.bind(Id(workspace_id))
        .push_bind(" AND tm.org_id = ", scope.org()?)
        .push(" AND wm.role IN ('admin', 'member') ORDER BY tm.team_id, tm.user_id");
    for row in scope.fetch_all(sql).await? {
        let id = row.id(0)?;
        if let Some(team) = teams.iter_mut().find(|team| team.id == id) {
            team.members.push(TeamMember {
                user_id: row.id(1)?,
                username: row.text(2)?,
                display_name: row.text(3)?,
            });
        }
    }
    Ok(teams)
}

fn name_error(error: StoreError) -> StoreError {
    match error {
        StoreError::Database(ref cause)
            if cause
                .as_database_error()
                .is_some_and(sqlx::error::DatabaseError::is_unique_violation) =>
        {
            StoreError::Conflict(Taken::TeamName)
        }
        other => other,
    }
}
