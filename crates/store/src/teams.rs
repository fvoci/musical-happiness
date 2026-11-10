//! Teams. A standalone server has one, with identifier `0`.

use fvoci_model::{IdKind, Team, new_id};

use crate::Store;
use crate::db::{Param, Row, Scope, Sql, now_millis};
use crate::error::StoreError;

/// The columns of a team, in the order [`team_from_row`] reads them.
const TEAM_COLUMNS: &str = "id, signup_token, COALESCE(settings, '{}'), modified_by, update_at";

impl Store {
    /// Sets the signup token of a team, creating the team if it does not exist.
    ///
    /// Stores `team`'s signup token and modifier with the current time as modification
    /// time. The settings of an existing team are kept; a new team has none.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the statement fails.
    pub async fn upsert_team_signup_token(&self, team: &Team) -> Result<(), StoreError> {
        self.with_conn(async |scope| {
            let mut sql = scope.sql(&format!(
                "INSERT INTO {} (id, signup_token, modified_by, update_at) VALUES (",
                scope.table("teams")
            ));
            sql.bind(&team.id)
                .push_bind(", ", &team.signup_token)
                .push_bind(", ", &team.modified_by)
                .push_bind(", ", now_millis())
                .push(
                    ") ON CONFLICT (id) DO UPDATE SET signup_token = EXCLUDED.signup_token, \
                     modified_by = EXCLUDED.modified_by, update_at = EXCLUDED.update_at",
                );
            scope.execute(sql).await?;
            Ok(())
        })
        .await
    }

    /// Sets the settings of a team, creating the team if it does not exist.
    ///
    /// Stores `team`'s settings and modifier with the current time as modification time.
    /// The signup token of `team` is not used: an existing team keeps its token and a
    /// new team gets a freshly generated one.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the statement fails.
    pub async fn upsert_team_settings(&self, team: &Team) -> Result<(), StoreError> {
        self.with_conn(async |scope| {
            // Preserved quirk: the token of the given team is ignored in favor of a new
            // random one, which is only stored if the team does not exist yet
            // (`server/services/store/sqlstore/team.go:58`).
            let signup_token = new_id(IdKind::Token);
            let mut sql = scope.sql(&format!(
                "INSERT INTO {} (id, signup_token, settings, modified_by, update_at) VALUES (",
                scope.table("teams")
            ));
            sql.bind(&team.id)
                .push_bind(", ", signup_token)
                .push_bind(", ", Param::json(&team.settings)?)
                .push_bind(", ", &team.modified_by)
                .push_bind(", ", now_millis())
                .push(
                    ") ON CONFLICT (id) DO UPDATE SET settings = EXCLUDED.settings, \
                     modified_by = EXCLUDED.modified_by, update_at = EXCLUDED.update_at",
                );
            scope.execute(sql).await?;
            Ok(())
        })
        .await
    }

    /// Returns the team with the given identifier. Its title is empty: a standalone
    /// server stores none.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NoRows`] if there is no such team.
    pub async fn get_team(&self, id: &str) -> Result<Team, StoreError> {
        self.with_conn(async |scope| get_team(scope, id).await)
            .await
    }

    /// Returns every team, in no particular order.
    ///
    /// This is also what the original returns as "the teams of a user": its standalone
    /// store does not look at the user.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the query fails.
    pub async fn get_all_teams(&self) -> Result<Vec<Team>, StoreError> {
        self.with_conn(async |scope| {
            // Preserved quirk: asked for the teams of a user, the original returns all
            // teams (`server/services/store/sqlstore/team.go:130-132`).
            let sql = select_teams(scope);
            scope
                .fetch_all(sql)
                .await?
                .iter()
                .map(team_from_row)
                .collect()
        })
        .await
    }

    /// Returns the number of teams.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the query fails.
    pub async fn get_team_count(&self) -> Result<i64, StoreError> {
        self.with_conn(async |scope| {
            let sql = scope.sql(&format!("SELECT COUNT(*) FROM {}", scope.table("teams")));
            match scope.fetch_optional(sql).await? {
                Some(row) => row.int(0),
                None => Ok(0),
            }
        })
        .await
    }
}

/// Starts `SELECT <team columns> FROM <teams table>`.
fn select_teams(scope: &Scope<'_>) -> Sql {
    scope.sql(&format!(
        "SELECT {TEAM_COLUMNS} FROM {}",
        scope.table("teams")
    ))
}

fn team_from_row(row: &Row) -> Result<Team, StoreError> {
    Ok(Team {
        id: row.text(0)?,
        signup_token: row.text(1)?,
        settings: row.json_map(2, "settings")?,
        modified_by: row.text(3)?,
        update_at: row.int(4)?,
        ..Team::default()
    })
}

pub(crate) async fn get_team(scope: &mut Scope<'_>, id: &str) -> Result<Team, StoreError> {
    let mut sql = select_teams(scope);
    sql.push_bind(" WHERE id = ", id);
    // Preserved quirk: a missing team is reported with the database layer's "no rows"
    // text (`server/services/store/sqlstore/team.go:110-119`).
    let row = scope.fetch_optional(sql).await?.ok_or(StoreError::NoRows)?;
    team_from_row(&row)
}
