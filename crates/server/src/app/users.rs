//! Users and their preferences.

use fvoci_model::{User, UserPreferencesPatch, UserSettings, UserSettingsPatch};
use fvoci_store::{AdminChange, InstanceRole, Preference};

use super::App;
use crate::error::{AppError, Reason};

impl App {
    /// Sets the name the user is shown by; empty for none. Returns the user as stored.
    ///
    /// # Errors
    ///
    /// Returns the store's error, a 404 if there is no such user.
    pub async fn set_display_name(&self, user_id: &str, name: &str) -> Result<User, AppError> {
        Ok(self.store().set_display_name(user_id, name).await?)
    }

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

    /// Makes a user an administrator of the instance, or ends that, and returns the
    /// user as stored now.
    ///
    /// # Errors
    ///
    /// Returns a not-found error if there is no such user, and a conflict (reason
    /// `last_admin`) if the user is the only administrator and would stop being one.
    pub async fn set_user_admin(
        &self,
        user_id: &str,
        role: InstanceRole,
    ) -> Result<User, AppError> {
        match self.store().set_user_admin(user_id, role).await? {
            AdminChange::Done(user) => Ok(*user),
            AdminChange::NoSuchUser => Err(AppError::not_found("user")),
            AdminChange::LastAdmin => Err(AppError::conflict(
                "the instance must keep an administrator",
            )
            .with_reason(Reason::LastAdmin)),
        }
    }

    /// Makes the user with the given name, in any letter case, an administrator of the
    /// instance. For the administration socket: the way back in when no administrator
    /// can sign in.
    ///
    /// # Errors
    ///
    /// Returns a not-found error if no user has that name.
    pub async fn make_admin_by_username(&self, username: &str) -> Result<User, AppError> {
        let user = match self.store().get_user_by_username(username).await {
            Ok(user) => user,
            Err(error) if error.is_not_found() => return Err(AppError::not_found("user")),
            Err(error) => return Err(error.into()),
        };
        self.set_user_admin(&user.id, InstanceRole::Administrator)
            .await
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
    /// TEMPORARY, with `only_organization_scope`: a guest is a role in a workspace now
    /// (`WorkspaceRole::Guest`), which the routes under a workspace ask.
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

    /// Returns the team's users whose login name or display name matches
    /// `search_query`, at most ten.
    ///
    /// # Errors
    ///
    /// Returns the store's error if the query fails.
    pub async fn search_team_users(&self, search_query: &str) -> Result<Vec<User>, AppError> {
        Ok(self.store().search_users_by_team(search_query).await?)
    }

    /// Returns what a user has set about how things are shown to them; what was never
    /// set is at its default.
    ///
    /// # Errors
    ///
    /// Returns the store's error if the settings cannot be read.
    pub async fn user_settings(&self, user_id: &str) -> Result<UserSettings, AppError> {
        Ok(self.store().get_user_settings(user_id).await?)
    }

    /// Changes some of a user's settings and returns all of them as they are afterwards.
    ///
    /// # Errors
    ///
    /// Returns the store's error if the settings cannot be written.
    pub async fn patch_user_settings(
        &self,
        user_id: &str,
        patch: &UserSettingsPatch,
    ) -> Result<UserSettings, AppError> {
        Ok(self.store().patch_user_settings(user_id, patch).await?)
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
    user.password_hash.clear();
}
