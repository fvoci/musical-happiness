//! Teams. A server has one for now, the root team, which is made at the first start.

use fvoci_model::{Team, new_id};

use super::App;
use crate::auth::secret::new_secret;
use crate::error::AppError;

impl App {
    /// Returns the root team: the oldest team, created with a fresh signup token when
    /// there is none yet.
    ///
    /// # Errors
    ///
    /// Returns the store's error if the team can be neither read nor created.
    pub async fn root_team(&self) -> Result<Team, AppError> {
        // Only a team that is not there is created. A read that fails is an error, not
        // "no team yet": taken for that, it would end in a second team.
        if let Some(team) = self.store().get_teams().await?.into_iter().next() {
            return Ok(team);
        }
        let team = Team {
            id: new_id(),
            signup_token: new_secret(),
            ..Team::default()
        };
        self.store().create_team(&team).await?;
        tracing::info!(team_id = %team.id, "created the root team");
        // Read back: two servers that start on one empty database each create a team,
        // and both must agree on which one is the root.
        self.store()
            .get_teams()
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| AppError::internal("the root team is not there after its creation"))
    }

    /// Returns the team with the given identifier.
    ///
    /// # Errors
    ///
    /// Returns a 404 if there is no such team, and the store's error if the query fails.
    pub async fn team(&self, team_id: &str) -> Result<Team, AppError> {
        match self.store().get_team(team_id).await {
            Ok(team) => Ok(team),
            Err(error) if error.is_not_found() => Err(AppError::not_found("team")),
            Err(error) => Err(error.into()),
        }
    }

    /// Returns every team, oldest first.
    ///
    /// # Errors
    ///
    /// Returns the store's error if the query fails.
    pub async fn teams(&self) -> Result<Vec<Team>, AppError> {
        Ok(self.store().get_teams().await?)
    }

    /// Replaces a team's signup token with a fresh one, on behalf of a user, and
    /// returns the team with it. The old token lets nobody in any more.
    ///
    /// # Errors
    ///
    /// Returns a 404 if there is no such team, and the store's error if the write fails.
    pub async fn regenerate_signup_token(
        &self,
        team_id: &str,
        user_id: &str,
    ) -> Result<Team, AppError> {
        match self
            .store()
            .set_team_signup_token(team_id, &new_secret(), user_id)
            .await
        {
            Ok(()) => self.team(team_id).await,
            Err(error) if error.is_not_found() => Err(AppError::not_found("team")),
            Err(error) => Err(error.into()),
        }
    }
}
