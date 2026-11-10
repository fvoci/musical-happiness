//! Login, logout, registration and password changes.

use std::sync::LazyLock;

use fvoci_model::{MAXIMUM_PASSWORD_BYTES, Session, User, new_id};
use fvoci_store::StoreError;
use tokio::sync::Semaphore;

use super::App;
use crate::auth::now_millis;
use crate::auth::password::{Verdict, hash_password, verify_password};
use crate::auth::secret::{new_secret, token_digest};
use crate::error::{AppError, Reason};

/// How many passwords are hashed or checked at a time. One hash takes 19 MiB and tens of
/// milliseconds of a processor; without a limit a burst of logins would take a
/// processor each and the memory to go with it.
const PASSWORD_WORK_AT_A_TIME: usize = 4;

static PASSWORD_WORK: LazyLock<Semaphore> =
    LazyLock::new(|| Semaphore::new(PASSWORD_WORK_AT_A_TIME));

/// What a registration came with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Admission {
    /// The team's signup token, which the caller has checked.
    WithToken,
    /// Nothing: only the first user of the instance gets in so.
    FirstUserOnly,
}

impl App {
    /// Checks a user's password and returns the token of a new session. The token is
    /// handed out here and nowhere else: the store keeps its digest only.
    ///
    /// The user is looked up by name and, if no user has that name, by email address.
    /// `user_agent` is what the client said it is, kept with the session.
    ///
    /// A password that is right but stored with outdated hashing parameters is hashed
    /// anew and stored again.
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
        user_agent: &str,
    ) -> Result<String, AppError> {
        // Nothing this long was ever set as a password; it is not worth a hash.
        if password.len() > MAXIMUM_PASSWORD_BYTES {
            return Err(incorrect_login());
        }
        let mut user: Option<User> = None;
        if !username.is_empty() {
            user = self.find_user_for_login(Lookup::Username(username)).await?;
        }
        if user.is_none() && !email.is_empty() {
            user = self.find_user_for_login(Lookup::Email(email)).await?;
        }
        // One answer for an unknown user and for a wrong password.
        let Some(user) = user else {
            return Err(incorrect_login());
        };
        let verdict = verify(&user.password_hash, password).await?;
        if verdict == Verdict::Mismatch {
            tracing::debug!(user_id = %user.id, "invalid password for user");
            return Err(incorrect_login());
        }

        let token = new_secret();
        let now = now_millis();
        let session = Session {
            id: new_id(),
            user_id: user.id.clone(),
            created_at: now,
            // The absolute limit: the session ends then, however much it is used.
            expires_at: now.saturating_add(self.config().session_max_millis()),
            user_agent: user_agent.to_owned(),
            ..Session::default()
        };
        self.store()
            .create_session(&session, &token_digest(&token))
            .await
            .map_err(|error| AppError::from(error).context("unable to create session"))?;

        // Checking a password takes a while. If the password was set anew in that time,
        // the reset has ended the user's sessions before this one existed, and this one
        // was opened with the old password: it must not outlive the reset. Whichever of
        // the two wrote last, the session is gone: here, or by the reset.
        let current = self
            .store()
            .get_user_by_id(&user.id)
            .await
            .map_err(|error| AppError::from(error).context("reading the user after login"))?;
        if current.password_hash != user.password_hash {
            self.store()
                .delete_session(&session.id)
                .await
                .map_err(|error| AppError::from(error).context("ending a stale session"))?;
            return Err(incorrect_login());
        }

        if verdict == Verdict::MatchWithOutdatedHash {
            self.store_current_hash(&user, password).await;
        }
        Ok(token)
    }

    /// Looks a user up for a login; a user who is not there is `None`.
    async fn find_user_for_login(&self, lookup: Lookup<'_>) -> Result<Option<User>, AppError> {
        let found = match lookup {
            Lookup::Username(name) => self.store().get_user_by_username(name).await,
            Lookup::Email(email) => self.store().get_user_by_email(email).await,
        };
        match found {
            Ok(user) => Ok(Some(user)),
            Err(error) if error.is_not_found() => Ok(None),
            Err(error) => Err(AppError::from(error).context("looking up a user for login")),
        }
    }

    /// Hashes the password a user just signed in with under the current parameters and
    /// stores that in place of the outdated hash. A failure is logged and the login
    /// stands: the old hash still verifies, and the next login tries again.
    async fn store_current_hash(&self, user: &User, password: &str) {
        let replaced = match hash(password).await {
            Ok(new_hash) => self
                .store()
                .replace_password_hash(&user.id, &user.password_hash, &new_hash)
                .await
                .map_err(AppError::from),
            Err(error) => Err(error),
        };
        match replaced {
            Ok(true) => {
                tracing::info!(user_id = %user.id, "stored a password hash with the current parameters");
            }
            // The password was changed in the meantime; the new one has a current hash.
            Ok(false) => {}
            Err(error) => {
                tracing::warn!(user_id = %user.id, %error, "could not renew the password hash");
            }
        }
    }

    /// Ends a session.
    ///
    /// # Errors
    ///
    /// Returns the store's error if the session cannot be deleted.
    pub async fn logout(&self, session_id: &str) -> Result<(), AppError> {
        self.store()
            .delete_session(session_id)
            .await
            .map_err(|error| AppError::from(error).context("unable to delete the session"))
    }

    /// Returns how many users are registered, deleted ones not counted.
    ///
    /// # Errors
    ///
    /// Returns the store's error if the count cannot be read.
    pub async fn registered_user_count(&self) -> Result<i64, AppError> {
        Ok(self.store().get_registered_user_count().await?)
    }

    /// Creates a user. The caller has checked the request and, if the registration
    /// came with a signup token, the token. The first user of the instance becomes its
    /// administrator.
    ///
    /// # Errors
    ///
    /// Returns a conflict error if the name or the email address is taken, in any letter
    /// case; a permission error (reason `signup_token_required`) if the registration
    /// came without a token and is not the first; and the store's error if the user
    /// cannot be stored.
    pub async fn register_user(
        &self,
        username: &str,
        email: &str,
        password: &str,
        admission: Admission,
    ) -> Result<(), AppError> {
        // Asked first so that a taken name costs no hash. Two registrations at once
        // both pass these, and then the unique indexes decide: the same conflict.
        if self
            .find_user_for_login(Lookup::Username(username))
            .await?
            .is_some()
        {
            return Err(username_taken());
        }
        if self
            .find_user_for_login(Lookup::Email(email))
            .await?
            .is_some()
        {
            return Err(email_taken());
        }

        let mut user = User {
            id: new_id(),
            username: username.to_owned(),
            email: email.to_owned(),
            password_hash: hash(password).await?,
            ..User::default()
        };
        let stored = match admission {
            Admission::WithToken => self.store().create_user(&mut user).await,
            Admission::FirstUserOnly => self.store().create_first_user(&mut user).await,
        };
        match stored {
            Ok(()) => {
                if user.is_admin {
                    tracing::info!(user_id = %user.id, "the first user is the administrator");
                }
                Ok(())
            }
            Err(StoreError::Conflict(what)) if what.contains("email") => Err(email_taken()),
            Err(StoreError::Conflict(_)) => Err(username_taken()),
            // Another registration without a token was first after all.
            Err(error @ StoreError::NotFirstUser) => Err(error.into()),
            Err(error) => Err(AppError::from(error).context("Unable to create the new user")),
        }
    }

    /// Sets the password of the user with the given name, ends that user's sessions and
    /// closes their websocket connections, so that nobody stays signed in with the old
    /// password. For a password an administrator sets; the caller has checked it.
    ///
    /// # Errors
    ///
    /// Returns a not-found error if no user has that name; neither the password nor any
    /// session changes then.
    pub async fn reset_user_password(
        &self,
        username: &str,
        password: &str,
    ) -> Result<(), AppError> {
        let user_id = self
            .store()
            .reset_user_password(username, &hash(password).await?)
            .await?;
        self.ws().close_user(&user_id);
        Ok(())
    }

    /// Changes a user's password after checking the old one, and ends the user's other
    /// sessions: whoever else knew the old password is signed out. The session the
    /// change came through stays.
    ///
    /// # Errors
    ///
    /// Returns a bad-request error if the old password is wrong, a not-found error if
    /// there is no such user, or the store's error.
    pub async fn change_password(
        &self,
        session: &Session,
        old_password: &str,
        new_password: &str,
    ) -> Result<(), AppError> {
        let user = self.store().get_user_by_id(&session.user_id).await?;
        let right = old_password.len() <= MAXIMUM_PASSWORD_BYTES
            && verify(&user.password_hash, old_password).await? != Verdict::Mismatch;
        if !right {
            tracing::debug!(user_id = %user.id, "invalid password for user");
            return Err(AppError::bad_request("the old password is wrong")
                .with_reason(Reason::InvalidCredentials)
                .with_field("oldPassword"));
        }
        self.store()
            .change_user_password(&user.id, &hash(new_password).await?, &session.id)
            .await
            .map_err(|error| AppError::from(error).context("unable to update password"))?;
        self.ws()
            .close_other_sessions_of_user(&user.id, &session.id);
        Ok(())
    }
}

/// What a login looks a user up by.
#[derive(Debug, Clone, Copy)]
enum Lookup<'a> {
    Username(&'a str),
    Email(&'a str),
}

/// The refusal of a login, whatever was wrong.
fn incorrect_login() -> AppError {
    AppError::unauthorized("incorrect login").with_reason(Reason::InvalidCredentials)
}

fn username_taken() -> AppError {
    AppError::conflict("the username already exists")
        .with_reason(Reason::UsernameTaken)
        .with_field("username")
}

fn email_taken() -> AppError {
    AppError::conflict("the email already exists")
        .with_reason(Reason::EmailTaken)
        .with_field("email")
}

/// Runs password work on the blocking pool, a bounded number at a time.
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
        .map_err(|error| AppError::internal(format_args!("{error}")))
}

async fn verify(hash: &str, password: &str) -> Result<Verdict, AppError> {
    let (hash, password) = (hash.to_owned(), password.to_owned());
    off_the_runtime(move || verify_password(&hash, &password)).await
}
