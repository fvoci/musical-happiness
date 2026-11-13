//! Users and their preferences.

use fvoci_model::{User, UserPreferencesPatch, UserSettings, UserSettingsPatch, WorkspaceRole};
use fvoci_store::{AdminChange, InstanceRole, Looking, Preference, Sight};

use super::App;
use crate::error::{AppError, Reason};
use crate::permissions;

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

    /// Makes a user an administrator of the instance, or ends that, on behalf of
    /// `actor_id`, and returns the user as stored now.
    ///
    /// # Errors
    ///
    /// Returns a not-found error if there is no such user, a 403 (reason
    /// `admin_required`) if `actor_id` stopped administering the instance before the
    /// change was made, and a conflict (reason `last_admin`) if the user is the only
    /// administrator, of the instance or of its organization, and would stop being one.
    pub async fn set_user_admin(
        &self,
        user_id: &str,
        role: InstanceRole,
        actor_id: &str,
    ) -> Result<User, AppError> {
        match self.store().set_user_admin(user_id, role, actor_id).await? {
            AdminChange::Done(user) => {
                if role != InstanceRole::Administrator {
                    // The workspaces they joined as the organization's administrator
                    // are theirs no more (`Store::set_user_admin`).
                    self.ws().follow_only_own_workspaces(user_id).await;
                }
                Ok(*user)
            }
            AdminChange::NoSuchUser => Err(AppError::not_found("user")),
            AdminChange::NotAnAdministrator => Err(AppError::forbidden(
                "only an administrator of the instance may do this",
            )
            .with_reason(Reason::AdminRequired)),
            AdminChange::LastAdmin => Err(AppError::conflict(
                "the instance must keep an administrator",
            )
            .with_reason(Reason::LastAdmin)),
        }
    }

    /// Resolves an operator log's target without exposing a name or request body.
    /// A missing account or unavailable lookup leaves the identifier absent; the command
    /// still performs its own checks and returns their outcome.
    pub(crate) async fn operator_user_id(&self, username: &str) -> Option<String> {
        self.store()
            .get_user_by_username(username)
            .await
            .ok()
            .map(|user| user.id)
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
        match self.store().make_admin_by_operator(&user.id).await? {
            AdminChange::Done(user) => Ok(*user),
            AdminChange::NoSuchUser => Err(AppError::not_found("user")),
            change @ (AdminChange::NotAnAdministrator | AdminChange::LastAdmin) => {
                Err(AppError::internal(format!(
                    "the operator's word made no administrator: {change:?}"
                )))
            }
        }
    }

    /// The users with the given identifiers that the user `seer_id`, in `role` in the
    /// workspace, sees there by name (`permissions::sees_person`): also who left it and
    /// wrote on a board the seer reads. Oldest account first, each once; whom the seer
    /// does not see there is left out, as one who does not exist.
    ///
    /// # Errors
    ///
    /// Returns the store's error if the users cannot be read.
    pub async fn users_in_workspace(
        &self,
        seer_id: &str,
        workspace_id: &str,
        role: WorkspaceRole,
        user_ids: &[String],
    ) -> Result<Vec<User>, AppError> {
        if user_ids.is_empty() {
            return Ok(Vec::new());
        }
        let sight = Sight::InWorkspace {
            seer_id,
            workspace_id,
            reach: permissions::people_reach(role),
            looking: Looking::ByName,
        };
        Ok(self.store().get_users_in_sight(sight, user_ids).await?)
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

    /// The users the user `seer_id`, in `role` in the workspace, sees among its people
    /// (`permissions::sees_person`: members of it now) whose login name or display name
    /// matches `search_query`: at most ten, oldest account first.
    ///
    /// # Errors
    ///
    /// Returns the store's error if the query fails.
    pub async fn search_users_in_workspace(
        &self,
        seer_id: &str,
        workspace_id: &str,
        role: WorkspaceRole,
        search_query: &str,
    ) -> Result<Vec<User>, AppError> {
        let sight = Sight::InWorkspace {
            seer_id,
            workspace_id,
            reach: permissions::people_reach(role),
            looking: Looking::AmongPeople,
        };
        Ok(self
            .store()
            .search_users_in_sight(sight, search_query)
            .await?)
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
