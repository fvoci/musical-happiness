//! Teams: the workspaces of the organization. The first, the root team, is made with it.

use fvoci_model::Team;

use super::App;
use crate::auth::secret::new_secret;
use crate::error::AppError;

impl App {
    /// Returns the root team: the oldest team of the organization, its first workspace,
    /// which is made with the organization.
    ///
    /// # Errors
    ///
    /// Returns the store's error if the teams cannot be read, and a server error if the
    /// organization has none.
    pub async fn root_team(&self) -> Result<Team, AppError> {
        self.store()
            .get_teams()
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| AppError::internal("the organization has no team"))
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
