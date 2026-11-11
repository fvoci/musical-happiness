//! Teams: the workspaces of an organization, until the table is renamed.
//!
//! A statement here names the organization itself: the handle's
//! ([`Tenant::Org`](crate::Tenant::Org)). SQLite has no policy that would keep the
//! teams of the system organization out of an answer.

use fvoci_model::Team;

use crate::Store;
use crate::db::{Bound, Id, OptId, Param, Row, Scope, Sql, now_millis};
use crate::error::StoreError;

const TEAM_COLUMNS: &str = "id, name, signup_token, settings, modified_by, update_at";

impl Store {
    /// Stores a new team of the handle's organization, with the current time as its
    /// creation and modification time.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NoOrganization`] if the handle names no organization,
    /// [`StoreError::InvalidId`] if the team's identifier is not one, and
    /// [`StoreError::Database`] if the statement fails, as it does when a team with the
    /// identifier exists.
    pub async fn create_team(&self, team: &Team) -> Result<(), StoreError> {
        self.with_conn(async |scope| {
            let org = scope.org()?;
            insert_team(scope, team, org).await
        })
        .await
    }

    /// Replaces the signup token of a team and records who did it.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NotFound`] if the organization has no such team.
    pub async fn set_team_signup_token(
        &self,
        team_id: &str,
        signup_token: &str,
        modified_by: &str,
    ) -> Result<(), StoreError> {
        self.with_conn(async |scope| {
            let mut sql = scope.sql("UPDATE teams SET signup_token = ");
            sql.bind(signup_token)
                .push_bind(", modified_by = ", OptId(modified_by))
                .push_bind(", update_at = ", now_millis())
                .push_bind(" WHERE id = ", Id(team_id))
                .push_bind(" AND org_id = ", scope.org()?)
                .push_bind(" AND delete_at = ", 0_i64);
            if scope.execute(sql).await? == 0 {
                return Err(team_not_found());
            }
            Ok(())
        })
        .await
    }

    /// Returns the team with the given identifier.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NotFound`] if the organization has no such team.
    pub async fn get_team(&self, id: &str) -> Result<Team, StoreError> {
        self.with_conn(async |scope| {
            let mut sql = select_teams(scope)?;
            sql.push_bind(" AND id = ", Id(id));
            let row = scope
                .fetch_optional(sql)
                .await?
                .ok_or_else(team_not_found)?;
            team_from_row(&row)
        })
        .await
    }

    /// Returns every team of the organization, oldest first. The first is the root
    /// team.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the query fails.
    pub async fn get_teams(&self) -> Result<Vec<Team>, StoreError> {
        self.with_conn(async |scope| {
            let mut sql = select_teams(scope)?;
            sql.push(" ORDER BY create_at, id");
            scope
                .fetch_all(sql)
                .await?
                .iter()
                .map(team_from_row)
                .collect()
        })
        .await
    }
}

/// Stores a new team of the organization `org`.
pub(crate) async fn insert_team(
    scope: &mut Scope<'_>,
    team: &Team,
    org: impl Into<Bound>,
) -> Result<(), StoreError> {
    let now = now_millis();
    let mut sql = scope.sql(
        "INSERT INTO teams (id, org_id, name, signup_token, settings, modified_by, \
         create_at, update_at) VALUES (",
    );
    sql.bind(Id(&team.id))
        .push_bind(", ", org)
        .push_bind(", ", &team.title)
        .push_bind(", ", &team.signup_token)
        .push_bind(", ", Param::json(&team.settings)?)
        .push_bind(", ", OptId(&team.modified_by))
        .push_bind(", ", now)
        .push_bind(", ", now)
        .push(")");
    scope.execute(sql).await?;
    Ok(())
}

fn team_not_found() -> StoreError {
    StoreError::not_found("team")
}

/// The teams of the handle's organization that are in use.
fn select_teams(scope: &Scope<'_>) -> Result<Sql, StoreError> {
    let mut sql = scope.sql(&format!("SELECT {TEAM_COLUMNS} FROM teams WHERE org_id = "));
    sql.bind(scope.org()?).push_bind(" AND delete_at = ", 0_i64);
    Ok(sql)
}

fn team_from_row(row: &Row) -> Result<Team, StoreError> {
    Ok(Team {
        id: row.id(0)?,
        title: row.text(1)?,
        signup_token: row.text(2)?,
        settings: row.json_map(3, "settings")?,
        modified_by: row.id(4)?,
        update_at: row.int(5)?,
    })
}
