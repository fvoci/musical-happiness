//! Login, logout, registration and password changes.

use std::num::NonZero;
use std::sync::LazyLock;

use fvoci_model::{IdKind, JsonMap, Session, User, new_id};
use tokio::sync::Semaphore;

use super::App;
use crate::auth::password::{PasswordSettings, check_password, compare_password, hash_password};
use crate::error::AppError;

/// How many passwords are hashed or checked at a time: one per processor. Hashing is
/// deliberately expensive; without a limit, a burst of logins would fill the pool of
/// blocking threads.
static PASSWORD_WORK: LazyLock<Semaphore> =
    LazyLock::new(|| Semaphore::new(std::thread::available_parallelism().map_or(2, NonZero::get)));

/// The shortest password accepted at registration, after the request's own check.
const REGISTER_MINIMUM_PASSWORD_LENGTH: usize = 6;

impl App {
    /// Checks a user's password and returns the token of a new session.
    ///
    /// The user is looked up by name and, if no user has that name, by email address.
    ///
    /// # Errors
    ///
    /// Returns an error if no such user exists, the password is wrong, or the session
    /// cannot be created. The caller reports all of these to the client the same way.
    pub async fn login(
        &self,
        username: &str,
        email: &str,
        password: &str,
    ) -> Result<String, AppError> {
        let mut user: Option<User> = None;
        if !username.is_empty() {
            user = match self.store().get_user_by_username(username).await {
                Ok(user) => Some(user),
                Err(error) if error.is_not_found() => None,
                Err(error) => {
                    tracing::error!(%error, "looking up a user for login failed");
                    return Err(AppError::from(error).context("invalid username or password"));
                }
            };
        }
        if user.is_none() && !email.is_empty() {
            // Preserved quirk: the original only reports "not found" from this lookup and
            // carries on after any other failure, which then ends as "invalid username or
            // password" all the same.
            user = match self.store().get_user_by_email(email).await {
                Ok(user) => Some(user),
                Err(error) => {
                    if !error.is_not_found() {
                        tracing::error!(%error, "looking up a user for login failed");
                    }
                    None
                }
            };
        }
        let Some(user) = user else {
            return Err(AppError::internal("invalid username or password"));
        };
        if !password_matches(&user.password, password).await? {
            tracing::debug!(user_id = %user.id, "invalid password for user");
            return Err(AppError::internal("invalid username or password"));
        }

        let session = Session {
            id: new_id(IdKind::Session),
            token: new_id(IdKind::Token),
            user_id: user.id,
            auth_service: if user.auth_service.is_empty() {
                "native".to_owned()
            } else {
                user.auth_service
            },
            props: JsonMap::new(),
            create_at: 0,
            update_at: 0,
        };
        self.store()
            .create_session(&session)
            .await
            .map_err(|error| {
                tracing::error!(%error, "unable to create session");
                AppError::from(error).context("unable to create session")
            })?;
        Ok(session.token)
    }

    /// Ends a session.
    ///
    /// # Errors
    ///
    /// Returns an error if the session cannot be deleted.
    pub async fn logout(&self, session_id: &str) -> Result<(), AppError> {
        self.store()
            .delete_session(session_id)
            .await
            .map_err(|error| {
                tracing::error!(%error, "unable to delete the session");
                AppError::from(error).context("unable to delete the session")
            })
    }

    /// Returns the number of registered users.
    ///
    /// # Errors
    ///
    /// Returns the store's error if the query fails.
    pub async fn registered_user_count(&self) -> Result<i64, AppError> {
        Ok(self.store().get_registered_user_count().await?)
    }

    /// Creates a user account.
    ///
    /// # Errors
    ///
    /// Returns an error whose message is meant for the client if the name or the email
    /// address is taken, the password breaks the rules, or the user cannot be stored.
    pub async fn register_user(
        &self,
        username: &str,
        email: &str,
        password: &str,
    ) -> Result<(), AppError> {
        if !username.is_empty() {
            match self.store().get_user_by_username(username).await {
                Ok(_) => return Err(AppError::internal("The username already exists")),
                Err(error) if error.is_not_found() => {}
                Err(error) => return Err(error.into()),
            }
        }
        if !email.is_empty() {
            match self.store().get_user_by_email(email).await {
                Ok(_) => return Err(AppError::internal("The email already exists")),
                Err(error) if error.is_not_found() => {}
                Err(error) => return Err(error.into()),
            }
        }

        let settings = PasswordSettings {
            minimum_length: REGISTER_MINIMUM_PASSWORD_LENGTH,
            required: &[],
        };
        check_password(password, settings)
            .map_err(|error| AppError::internal(format_args!("Invalid password: {error}")))?;

        let mut user = User {
            id: new_id(IdKind::User),
            username: username.to_owned(),
            email: email.to_owned(),
            password: hash(password).await?,
            auth_service: self.config().auth_mode.clone(),
            ..User::default()
        };
        self.store()
            .create_user(&mut user)
            .await
            .map_err(|error| AppError::from(error).context("Unable to create the new user"))
    }

    /// Sets the password of every user with the given name. For the administration
    /// socket.
    ///
    /// # Errors
    ///
    /// Returns an error if no user has that name or the password cannot be stored.
    pub async fn update_user_password(
        &self,
        username: &str,
        password: &str,
    ) -> Result<(), AppError> {
        Ok(self
            .store()
            .update_user_password(username, &hash(password).await?)
            .await?)
    }

    /// Changes a user's password after checking the old one.
    ///
    /// # Errors
    ///
    /// Returns an error whose message is meant for the client if the user does not exist,
    /// the old password is wrong, or the new one cannot be stored.
    pub async fn change_password(
        &self,
        user_id: &str,
        old_password: &str,
        new_password: &str,
    ) -> Result<(), AppError> {
        if user_id.is_empty() {
            return Err(AppError::internal("invalid username or password"));
        }
        let user = self
            .store()
            .get_user_by_id(user_id)
            .await
            .map_err(|error| AppError::from(error).context("invalid username or password"))?;
        if !password_matches(&user.password, old_password).await? {
            tracing::debug!(user_id = %user.id, "invalid password for user");
            return Err(AppError::internal("invalid username or password"));
        }
        self.store()
            .update_user_password_by_id(user_id, &hash(new_password).await?)
            .await
            .map_err(|error| AppError::from(error).context("unable to update password"))
    }
}

/// Runs password work off the async worker threads, which it would otherwise block for
/// tens of milliseconds per password.
async fn off_the_runtime<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
) -> Result<T, AppError> {
    let _permit = PASSWORD_WORK
        .acquire()
        .await
        .map_err(|error| AppError::internal(format_args!("password work: {error}")))?;
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|error| AppError::internal(format_args!("password work: {error}")))
}

async fn hash(password: &str) -> Result<String, AppError> {
    let password = password.to_owned();
    off_the_runtime(move || hash_password(&password))
        .await?
        .map_err(|error| AppError::internal(format_args!("hashing a password: {error}")))
}

async fn password_matches(hash: &str, password: &str) -> Result<bool, AppError> {
    let (hash, password) = (hash.to_owned(), password.to_owned());
    off_the_runtime(move || compare_password(&hash, &password)).await
}
