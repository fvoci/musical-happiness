//! Users and their preferences.

use fvoci_model::{User, UserPreferencesPatch};
use fvoci_store::Preference;

use super::App;
use crate::error::AppError;

impl App {
    /// Returns the user with the given identifier.
    ///
    /// # Errors
    ///
    /// Returns an error if the identifier is empty or no such user exists.
    pub async fn user(&self, user_id: &str) -> Result<User, AppError> {
        if user_id.is_empty() {
            return Err(AppError::internal("no user ID"));
        }
        self.store()
            .get_user_by_id(user_id)
            .await
            .map_err(|error| AppError::from(error).context("unable to find user"))
    }

    /// Returns the users with the given identifiers.
    ///
    /// # Errors
    ///
    /// Returns an error if the list is empty or not every user exists.
    pub async fn users_list(&self, user_ids: &[String]) -> Result<Vec<User>, AppError> {
        if user_ids.is_empty() {
            return Err(AppError::internal("No User IDs"));
        }
        self.store()
            .get_users_list(user_ids)
            .await
            .map_err(|error| AppError::from(error).context("unable to find users"))
    }

    /// Returns whether a user is a guest. A standalone server has no guests, but the user
    /// must exist.
    ///
    /// # Errors
    ///
    /// Returns the store's error if the user cannot be read.
    pub async fn user_is_guest(&self, user_id: &str) -> Result<bool, AppError> {
        Ok(self.store().get_user_by_id(user_id).await?.is_guest)
    }

    /// Returns whether one user may see another. Every user who exists sees every other.
    ///
    /// # Errors
    ///
    /// Returns the store's error if the seeing user cannot be read.
    pub async fn can_see_user(&self, seer_id: &str) -> Result<bool, AppError> {
        // A guest would be limited to users they share a board with; there are no guests.
        self.user_is_guest(seer_id).await?;
        Ok(true)
    }

    /// Returns the team's users whose name matches `search_query`, at most ten.
    ///
    /// # Errors
    ///
    /// Returns the store's error if the query fails.
    pub async fn search_team_users(&self, search_query: &str) -> Result<Vec<User>, AppError> {
        Ok(self.store().search_users_by_team(search_query).await?)
    }

    /// Applies a patch to a user's preferences and returns all of them.
    ///
    /// # Errors
    ///
    /// Returns the store's error if the preferences cannot be written or read.
    pub async fn update_user_config(
        &self,
        user_id: &str,
        patch: &UserPreferencesPatch,
    ) -> Result<Vec<Preference>, AppError> {
        Ok(self.store().patch_user_preferences(user_id, patch).await?)
    }

    /// Returns a user's preferences.
    ///
    /// # Errors
    ///
    /// Returns the store's error if the preferences cannot be read.
    pub async fn user_preferences(&self, user_id: &str) -> Result<Vec<Preference>, AppError> {
        Ok(self.store().get_user_preferences(user_id).await?)
    }

    /// Blanks what another user must not see of a profile: the email address and the
    /// full name, unless the configuration shows them.
    pub fn sanitize_profile(&self, user: &mut User) {
        sanitize(user);
        if !self.config().show_email_address {
            user.email.clear();
        }
        if !self.config().show_full_name {
            user.first_name.clear();
            user.last_name.clear();
        }
    }
}

/// Blanks the secrets of a user record.
pub fn sanitize(user: &mut User) {
    user.password.clear();
    user.mfa_secret.clear();
}
