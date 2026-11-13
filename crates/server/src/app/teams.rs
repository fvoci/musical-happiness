//! Workspace teams. Authorization is checked at the route and again by writes under
//! the workspace lock; names are trimmed here before they reach storage.

use super::App;
use crate::error::AppError;
use fvoci_model::{Team, is_id};

fn name(text: &str) -> Result<&str, AppError> {
    fvoci_model::team_name(text).map_err(|error| AppError::bad_request(error).with_field("name"))
}

impl App {
    /// Lists teams, including their nonguest members.
    ///
    /// # Errors
    /// Returns the store error if the list cannot be read.
    pub async fn teams(&self, workspace_id: &str) -> Result<Vec<Team>, AppError> {
        Ok(self.store().teams(workspace_id).await?)
    }

    /// Reads a team within its workspace.
    ///
    /// # Errors
    /// Returns 404 if the team is absent or belongs to a different workspace.
    pub async fn team(&self, workspace_id: &str, team_id: &str) -> Result<Team, AppError> {
        if !is_id(team_id) {
            return Err(AppError::not_found("team"));
        }
        self.teams(workspace_id)
            .await?
            .into_iter()
            .find(|team| team.id == team_id)
            .ok_or_else(|| AppError::not_found("team"))
    }

    /// Creates an empty team for the workspace's administrator.
    ///
    /// # Errors
    /// Returns 400 for an empty name, 409 for a duplicate, or the store error.
    pub async fn create_team(
        &self,
        workspace_id: &str,
        text: &str,
        by: &str,
    ) -> Result<Team, AppError> {
        let id = self
            .store()
            .create_team(workspace_id, name(text)?, by)
            .await?;
        self.team(workspace_id, &id).await
    }

    /// Renames a team for the workspace's administrator.
    ///
    /// # Errors
    /// As [`App::create_team`], and 404 if the team is absent.
    pub async fn rename_team(
        &self,
        workspace_id: &str,
        team_id: &str,
        text: &str,
        by: &str,
    ) -> Result<Team, AppError> {
        self.store()
            .rename_team(workspace_id, team_id, name(text)?, by)
            .await?;
        self.team(workspace_id, team_id).await
    }

    /// Deletes a team and all its board roles.
    ///
    /// # Errors
    /// Returns 404 for an absent team or the store error.
    pub async fn delete_team(
        &self,
        workspace_id: &str,
        team_id: &str,
        by: &str,
    ) -> Result<(), AppError> {
        Ok(self.store().delete_team(workspace_id, team_id, by).await?)
    }

    /// Adds a nonguest workspace member to a team.
    ///
    /// # Errors
    /// Returns 404 for an absent team or a person who is no nonguest workspace member.
    pub async fn add_team_member(
        &self,
        workspace_id: &str,
        team_id: &str,
        user_id: &str,
        by: &str,
    ) -> Result<(), AppError> {
        if !is_id(user_id) {
            return Err(AppError::not_found("workspace member"));
        }
        Ok(self
            .store()
            .add_team_member(workspace_id, team_id, user_id, by)
            .await?)
    }

    /// Removes a person from a team.
    ///
    /// # Errors
    /// Returns 404 for an absent team or the store error.
    pub async fn remove_team_member(
        &self,
        workspace_id: &str,
        team_id: &str,
        user_id: &str,
        by: &str,
    ) -> Result<(), AppError> {
        if !is_id(user_id) {
            return Err(AppError::not_found("workspace member"));
        }
        Ok(self
            .store()
            .remove_team_member(workspace_id, team_id, user_id, by)
            .await?)
    }
}
