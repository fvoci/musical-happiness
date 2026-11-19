//! Users and their preferences.

use fvoci_model::{
    ContainerKinds, ContainerMember, PersonNameChange, Session, User, UserPreferencesPatch,
    UserSettings, UserSettingsPatch, WorkspaceRole,
};
use fvoci_store::{AdminChange, InstanceRole, Looking, Preference, Sight};

use super::App;
use crate::collab::AccessScope;
use crate::error::{AppError, Reason};
use crate::permissions;

impl App {
    /// The session's user's memberships of boards and wikis in their live workspaces,
    /// across the organizations the session opens: those of one that takes only another
    /// provider's sessions are left out, as the others still open
    /// (`sso::require_session_opens`).
    ///
    /// # Errors
    ///
    /// Returns the store's error if the memberships cannot be read.
    pub async fn container_memberships(
        &self,
        session: &Session,
    ) -> Result<Vec<ContainerMember>, AppError> {
        Ok(self
            .store()
            .container_memberships_of_user(
                &session.user_id,
                session.login_provider_id(),
                ContainerKinds::Every,
            )
            .await?)
    }

    /// Changes what `change` names of the user's name, and so the name they are shown
    /// by. Returns the user as stored.
    ///
    /// # Errors
    ///
    /// Returns the store's error, a 404 if there is no such user.
    pub async fn change_person_name(
        &self,
        user_id: &str,
        change: &PersonNameChange,
    ) -> Result<User, AppError> {
        Ok(self.store().change_person_name(user_id, change).await?)
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
                    self.left_instance_org_workspaces(user_id).await;
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
        // Whatever the account's state: a command may be about one that waits to be
        // deleted, which has its name still.
        match self.store().account_id_by_username(username).await {
            Ok(user_id) => user_id,
            Err(error) => {
                tracing::warn!(%error, "the target of an operator's command could not be read");
                None
            }
        }
    }

    /// Stops what `user_id` follows of the workspaces of the instance's own organization
    /// they joined as its administrator, which are theirs no more
    /// (`Store::set_user_admin`), and has the rooms there ask about them again. The
    /// change is stored: a directory that cannot be read now leaves the rooms to their
    /// periodic asking, and the subscriptions to the checks at dispatch.
    async fn left_instance_org_workspaces(&self, user_id: &str) {
        match self.instance_org().await {
            Ok(Some(org)) => {
                org.ws().follow_only_own_workspaces(user_id).await;
                org.collab()
                    .recheck_access(&org, AccessScope::organization().of_user(user_id))
                    .await;
            }
            Ok(None) => self.ws().follow_only_own_workspaces(user_id).await,
            Err(error) => {
                tracing::warn!(%error, user_id, "the instance's organization could not be read after its administrator was let go");
                self.ws().follow_only_own_workspaces(user_id).await;
            }
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

    /// The users the user `seer_id`, in `role` in the workspace, sees among its people
    /// (`permissions::sees_person`: members of it now) whose login name or name
    /// (`PersonName::contains`) matches `search_query`: at most ten, oldest account first.
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

    /// Blanks what another user must not see of a profile: the email address, unless
    /// the configuration shows it. The name, all of its parts, is seen by whoever is
    /// shown the user.
    pub fn sanitize_profile(&self, user: &mut User) {
        sanitize(user);
        if !self.config().show_email_addresses {
            user.email.clear();
        }
    }
}

/// Blanks the secrets of a user record.
pub fn sanitize(user: &mut User) {
    user.password_hash.clear();
}
