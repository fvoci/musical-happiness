//! Teams. A standalone server has one, the root team with identifier `0`.

use fvoci_model::{GLOBAL_TEAM_ID, IdKind, Team, new_id};

use super::App;
use crate::error::AppError;

impl App {
    /// Returns the root team, creating it with a fresh signup token on first use.
    ///
    /// # Errors
    ///
    /// Returns the store's error if the team can be neither read nor created.
    pub async fn root_team(&self) -> Result<Team, AppError> {
        // The original takes any failure of this read for "no team yet" and then writes
        // a new signup token, which silently invalidates every invitation after a
        // passing database error. Only a team that is not there is created.
        match self.store().get_team(GLOBAL_TEAM_ID).await {
            Ok(team) => return Ok(team),
            Err(error) if error.is_not_found() => {}
            Err(error) => return Err(error.into()),
        }
        let team = Team {
            id: GLOBAL_TEAM_ID.to_owned(),
            signup_token: new_id(IdKind::Token),
            ..Team::default()
        };
        self.store().upsert_team_signup_token(&team).await?;
        let team = self.store().get_team(GLOBAL_TEAM_ID).await?;
        tracing::info!("initialized team");
        Ok(team)
    }

    /// Returns every team; on a standalone server, the root team.
    ///
    /// # Errors
    ///
    /// Returns the store's error if the query fails.
    pub async fn teams(&self) -> Result<Vec<Team>, AppError> {
        Ok(self.store().get_all_teams().await?)
    }

    /// Replaces the root team's signup token with a fresh one.
    ///
    /// # Errors
    ///
    /// Returns the store's error if the team cannot be read or written.
    pub async fn regenerate_signup_token(&self) -> Result<(), AppError> {
        let mut team = self.root_team().await?;
        team.signup_token = new_id(IdKind::Token);
        Ok(self.store().upsert_team_signup_token(&team).await?)
    }
}
