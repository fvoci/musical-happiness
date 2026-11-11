//! Login, logout, registration and password changes.

use std::net::IpAddr;
use std::time::Instant;

use fvoci_model::{MAXIMUM_PASSWORD_BYTES, RegisterRequest, Session, User, new_id};
use fvoci_store::{StoreError, Taken};

use super::App;
use crate::auth::hash_queue::QueueError;
use crate::auth::limiter::RetryAfter;
use crate::auth::now_millis;
use crate::auth::password::{Verdict, hash_password, verify_against_no_account, verify_password};
use crate::auth::secret::{new_secret, token_digest};
use crate::error::{AppError, ErrorKind, Reason};
use crate::metrics::LoginOutcome;

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
    /// `user_agent` is what the client said it is, kept with the session. `address` is
    /// where the attempt comes from, for the limits on login attempts.
    ///
    /// A password that is right but stored with outdated hashing parameters is hashed
    /// anew and stored again.
    ///
    /// # Errors
    ///
    /// Returns a 429 with the time to wait if the attempt is one too many: for its
    /// address, for its address at this account, or for the account. Returns one and the
    /// same 401 if no such user exists or the password is wrong, after the same work.
    pub async fn login(
        &self,
        username: &str,
        email: &str,
        password: &str,
        user_agent: &str,
        address: Option<IpAddr>,
    ) -> Result<String, AppError> {
        let result = self
            .check_login(username, email, password, user_agent, address)
            .await;
        self.metrics().login_ended(login_outcome(&result));
        result
    }

    /// The work of [`App::login`], unmeasured.
    async fn check_login(
        &self,
        username: &str,
        email: &str,
        password: &str,
        user_agent: &str,
        address: Option<IpAddr>,
    ) -> Result<String, AppError> {
        // The attempt is counted at the names it gives, and before any of them is
        // looked up: who has to wait must not depend on whether there is an account.
        let mut names: Vec<&str> = [username, email]
            .into_iter()
            .filter(|name| !name.is_empty())
            .collect();
        if names.is_empty() {
            names.push("");
        }
        let attempt = self
            .login_limiter()
            .admit_login(&names, address, Instant::now())
            .map_err(|RetryAfter(wait)| AppError::rate_limited(wait))?;

        // The name the account was found by, for the limiter: the one a success is for.
        let mut found_by = username;
        let mut user: Option<User> = None;
        if !username.is_empty() {
            user = self.find_user_for_login(Lookup::Username(username)).await?;
        }
        if user.is_none() && !email.is_empty() {
            user = self.find_user_for_login(Lookup::Email(email)).await?;
            found_by = email;
        }

        // Nothing this long was ever set as a password; it is not worth a hash.
        if password.len() > MAXIMUM_PASSWORD_BYTES {
            attempt.failed(Instant::now());
            return Err(incorrect_login());
        }
        // One answer for an unknown user and for a wrong password, after the same work.
        let Some(user) = user else {
            let password = password.to_owned();
            self.password_work(move || verify_against_no_account(&password))
                .await?;
            attempt.failed(Instant::now());
            return Err(incorrect_login());
        };
        let verdict = self.verify(&user.password_hash, password).await?;
        if verdict == Verdict::Mismatch {
            tracing::debug!(user_id = %user.id, "invalid password for user");
            attempt.failed(Instant::now());
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
        // Checking a password takes a while. The session is stored only if the hash the
        // password was checked against is still the user's: if the password was set anew
        // in that time, a session opened with the old one must not appear after the
        // reset that ended the others.
        let mut seen_hash = user.password_hash.clone();
        let mut stored = self.create_session(&session, &token, &seen_hash).await?;
        if !stored {
            // The hash changed. That is also what a login does that renews an outdated
            // hash, at the same moment, for the same password: checked against the hash
            // there is now, the password may still be right.
            let current = self
                .store()
                .get_user_by_id(&user.id)
                .await
                .map_err(|error| AppError::from(error).context("reading the user after login"))?;
            if self.verify(&current.password_hash, password).await? != Verdict::Mismatch {
                seen_hash = current.password_hash;
                stored = self.create_session(&session, &token, &seen_hash).await?;
            }
        }
        if !stored {
            return Err(incorrect_login());
        }

        attempt.succeeded(found_by);
        if verdict == Verdict::MatchWithOutdatedHash {
            self.store_current_hash(&user, password).await;
        }
        Ok(token)
    }

    /// Stores a session under its token if the user's password hash is still
    /// `seen_hash`; says whether it was stored.
    async fn create_session(
        &self,
        session: &Session,
        token: &str,
        seen_hash: &str,
    ) -> Result<bool, AppError> {
        self.store()
            .create_session(session, &token_digest(token), seen_hash)
            .await
            .map_err(|error| AppError::from(error).context("unable to create session"))
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
        let replaced = match self.hash(password).await {
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
            .map_err(|error| AppError::from(error).context("unable to delete the session"))?;
        // What the session had open ends with it.
        self.ws().close_session(session_id);
        Ok(())
    }

    /// Returns how many users are registered, deleted ones not counted.
    ///
    /// # Errors
    ///
    /// Returns the store's error if the count cannot be read.
    pub async fn registered_user_count(&self) -> Result<i64, AppError> {
        Ok(self.store().get_registered_user_count().await?)
    }

    /// Creates a user and returns them. The caller has checked the request and, if the
    /// registration came with a signup token, the token. The first user of the instance
    /// becomes its administrator.
    ///
    /// # Errors
    ///
    /// Returns a conflict error if the name or the email address is taken, in any letter
    /// case; a permission error (reason `signup_token_required`) if the registration
    /// came without a token and is not the first; and the store's error if the user
    /// cannot be stored.
    pub async fn register_user(
        &self,
        request: &RegisterRequest,
        admission: Admission,
        address: Option<IpAddr>,
    ) -> Result<User, AppError> {
        let username = request.username.as_str();
        let email = request.email.as_str();
        // A registration costs a hash like a login, and is counted against where it
        // comes from like one. The turn is held until the user is stored.
        let _attempt = self
            .login_limiter()
            .admit_registration(address, Instant::now())
            .map_err(|RetryAfter(wait)| AppError::rate_limited(wait))?;
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
            display_name: request.display_name.clone(),
            password_hash: self.hash(&request.password).await?,
            ..User::default()
        };
        let stored = match admission {
            Admission::WithToken => self.store().create_user(&mut user).await,
            Admission::FirstUserOnly => self.store().create_first_user(&mut user).await,
        };
        match stored {
            Ok(()) => {
                self.metrics().registered();
                if user.is_admin {
                    tracing::info!(user_id = %user.id, "the first user is the administrator");
                }
                Ok(user)
            }
            Err(StoreError::Conflict(Taken::Email)) => Err(email_taken()),
            Err(StoreError::Conflict(Taken::Username)) => Err(username_taken()),
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
            .reset_user_password(username, &self.hash(password).await?)
            .await?;
        self.ws().close_user(&user_id);
        Ok(())
    }

    /// Changes a user's password after checking the old one, and ends the user's other
    /// sessions: whoever else knew the old password is signed out. The session the
    /// change came through stays, under a new token, which is returned: what the session
    /// can do has changed, and the token it had is no longer accepted.
    ///
    /// # Errors
    ///
    /// Returns a 429 with the time to wait if the old password was got wrong too often,
    /// as for a login to the account by its name: a session is not the password, and
    /// whoever holds one must not get to guess at it without end. Returns a bad-request
    /// error if the old password is wrong, a not-found error if there is no such user, a
    /// conflict if the password was set anew while this change was under way, or the
    /// store's error.
    pub async fn change_password(
        &self,
        session: &Session,
        old_password: &str,
        new_password: &str,
        address: Option<IpAddr>,
    ) -> Result<String, AppError> {
        let user = self.store().get_user_by_id(&session.user_id).await?;
        // Counted with the logins to the account by its name, and one at a time from an
        // address, like them.
        let attempt = self
            .login_limiter()
            .admit_login(&[&user.username], address, Instant::now())
            .map_err(|RetryAfter(wait)| AppError::rate_limited(wait))?;
        let right = old_password.len() <= MAXIMUM_PASSWORD_BYTES
            && self.verify(&user.password_hash, old_password).await? != Verdict::Mismatch;
        if !right {
            tracing::debug!(user_id = %user.id, "invalid password for user");
            attempt.failed(Instant::now());
            return Err(AppError::bad_request("the old password is wrong")
                .with_reason(Reason::InvalidCredentials)
                .with_field("oldPassword"));
        }
        attempt.succeeded(&user.username);
        let token = new_secret();
        let changed = self
            .store()
            .change_user_password(
                &user.id,
                &user.password_hash,
                &self.hash(new_password).await?,
                &session.id,
                &token_digest(&token),
            )
            .await
            .map_err(|error| AppError::from(error).context("unable to update password"))?;
        if !changed {
            // Hashing takes a while. The password that was checked above is not the
            // user's any more: an administrator set another one, or another request
            // changed it. This change must not undo that.
            return Err(AppError::conflict(
                "the password was changed in the meantime; sign in again",
            ));
        }
        self.ws()
            .close_other_sessions_of_user(&user.id, &session.id);
        Ok(token)
    }
}

/// What a login looks a user up by.
#[derive(Debug, Clone, Copy)]
enum Lookup<'a> {
    Username(&'a str),
    Email(&'a str),
}

/// The refusal of a login, whatever was wrong.
/// How a login that answered `result` ended, for the metrics: by the kind of its error,
/// which is the same for an unknown account and a wrong password.
fn login_outcome<T>(result: &Result<T, AppError>) -> LoginOutcome {
    match result {
        Ok(_) => LoginOutcome::Succeeded,
        Err(error) => match error.kind() {
            ErrorKind::Unauthorized => LoginOutcome::InvalidCredentials,
            ErrorKind::RateLimited => LoginOutcome::Limited,
            _ => LoginOutcome::Failed,
        },
    }
}

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

impl App {
    /// Runs password work in the queue for it. What does not fit in the queue is
    /// answered 429.
    async fn password_work<T: Send + 'static>(
        &self,
        work: impl FnOnce() -> T + Send + 'static,
    ) -> Result<T, AppError> {
        match self.inner_password_work().run(work).await {
            Ok(result) => Ok(result),
            Err(QueueError::Full(wait)) => Err(AppError::rate_limited(wait)),
            Err(QueueError::Failed) => Err(AppError::internal("password work did not finish")),
        }
    }

    async fn hash(&self, password: &str) -> Result<String, AppError> {
        let password = password.to_owned();
        self.password_work(move || hash_password(&password))
            .await?
            .map_err(|error| AppError::internal(format_args!("{error}")))
    }

    async fn verify(&self, hash: &str, password: &str) -> Result<Verdict, AppError> {
        let (hash, password) = (hash.to_owned(), password.to_owned());
        self.password_work(move || verify_password(&hash, &password))
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_login_ends_in_one_of_four_outcomes_by_the_kind_of_its_error() {
        let ended = |result: Result<(), AppError>| login_outcome(&result);
        assert_eq!(ended(Ok(())), LoginOutcome::Succeeded);
        assert_eq!(
            ended(Err(incorrect_login())),
            LoginOutcome::InvalidCredentials
        );
        assert_eq!(
            ended(Err(AppError::rate_limited(std::time::Duration::from_secs(
                1
            )))),
            LoginOutcome::Limited
        );
        assert_eq!(
            ended(Err(AppError::internal("the database is gone"))),
            LoginOutcome::Failed
        );
    }
}
