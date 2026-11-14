//! Login, logout, registration and password changes.

use std::net::IpAddr;
use std::time::Instant;

use fvoci_model::{MAXIMUM_PASSWORD_BYTES, NewAccount, RegisterRequest, Session, User, new_id};
use fvoci_store::{Founding, SignupLookup, StoreError, Taken};

use super::App;
use crate::auth::hash_queue::QueueError;
use crate::auth::limiter::RetryAfter;
use crate::auth::now_millis;
use crate::auth::password::{Verdict, hash_password, verify_against_no_account, verify_password};
use crate::auth::secret::{new_secret, token_digest};
use crate::error::{AppError, ErrorKind, Reason};
use crate::metrics::{LoginOutcome, Metrics};

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
        let mut ending = LoginEnding {
            metrics: self.metrics(),
            outcome: LoginOutcome::Abandoned,
        };
        let result = self
            .check_login(username, email, password, user_agent, address)
            .await;
        ending.outcome = login_outcome(&result);
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

        let (user, found_by) = self.find_account(username, email).await?;
        // Asked whatever was found, as the email address is: the work of a login must
        // not tell whether there is an account or a registration of that name.
        let signup = self.find_signup(username, email).await?;

        // Nothing this long was ever set as a password; it is not worth a hash.
        if password.len() > MAXIMUM_PASSWORD_BYTES {
            attempt.failed(Instant::now());
            return Err(incorrect_login());
        }
        // One answer for an unknown user, for a wrong password and for an account that
        // has no password (one made through a provider), after the same work.
        let Some(user) = user.filter(|user| !user.password_hash.is_empty()) else {
            if let Some(signup) = signup {
                return Err(self.refuse_unconfirmed(&signup, password, attempt).await);
            }
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
                .map_err(|error| {
                    // Deactivated meanwhile: the login is refused as any other.
                    if error.is_not_found() {
                        incorrect_login()
                    } else {
                        AppError::from(error).context("reading the user after login")
                    }
                })?;
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

    /// The refusal of a login that names a registration whose address is not confirmed
    /// yet: told so to who knows its password, and to nobody else, after the work of
    /// any login.
    async fn refuse_unconfirmed(
        &self,
        signup: &fvoci_store::Signup,
        password: &str,
        attempt: crate::auth::limiter::Attempt,
    ) -> AppError {
        match self.verify(&signup.password_hash, password).await {
            Ok(Verdict::Mismatch) => {
                attempt.failed(Instant::now());
                incorrect_login()
            }
            Ok(_) => email_unverified(),
            Err(error) => error,
        }
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

    /// The account a login names, by its name or else by its email address, and the
    /// one of the two it was found by (the name if none): for the limiter, the one a
    /// success is for.
    ///
    /// The email address is looked up whenever it is given, also after the name found
    /// an account: the work of a login, which the metrics of the database count, must
    /// not tell whether there is an account of that name.
    async fn find_account<'n>(
        &self,
        username: &'n str,
        email: &'n str,
    ) -> Result<(Option<User>, &'n str), AppError> {
        let by_name = match username {
            "" => None,
            name => self.find_user_for_login(Lookup::Username(name)).await?,
        };
        let by_email = match email {
            "" => None,
            email => self.find_user_for_login(Lookup::Email(email)).await?,
        };
        Ok(match (by_name, by_email) {
            (Some(user), _) => (Some(user), username),
            (None, Some(user)) => (Some(user), email),
            (None, None) => (None, username),
        })
    }

    /// The registration whose address is not confirmed yet that a login names, by its
    /// name or else by its email address; both are asked whenever they are given, as
    /// [`App::find_account`] asks of the accounts.
    pub(super) async fn find_signup(
        &self,
        username: &str,
        email: &str,
    ) -> Result<Option<fvoci_store::Signup>, AppError> {
        let now = now_millis();
        let mut found = None;
        for lookup in [SignupLookup::Username(username), SignupLookup::Email(email)] {
            if matches!(lookup, SignupLookup::Username("") | SignupLookup::Email("")) {
                continue;
            }
            let signup = self
                .store()
                .live_signup(lookup, now)
                .await
                .map_err(|error| AppError::from(error).context("looking up a registration"))?;
            found = found.or(signup);
        }
        Ok(found)
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

    /// Whether a registration can be taken at all: on a server that takes several
    /// organizations always, on one that takes one until it has its first user.
    ///
    /// # Errors
    ///
    /// Returns the store's error if the instance cannot be read.
    pub async fn takes_registrations(&self) -> Result<bool, AppError> {
        Ok(self.config().takes_several_organizations() || !self.store().is_bootstrapped().await?)
    }

    /// Makes the account of somebody who signs up, and the organization they start by
    /// it, and returns the account: the user administers the organization and its
    /// first workspace, which both have the name of the request. The first user of the
    /// instance creates its first organization and administers the instance;
    /// everybody after starts a new organization, where the server takes several. The
    /// caller has tidied and checked the request (`RegisterRequest::tidied`,
    /// `RegisterRequest::validate`).
    ///
    /// # Errors
    ///
    /// Returns a conflict error if the name or the email address is taken, in any letter
    /// case; a permission error (reason `invitation_required`) if the instance has its
    /// first user and takes one organization; a 429 if there are too many registrations
    /// from the address; and the store's error if the user cannot be stored.
    pub async fn sign_up(
        &self,
        request: &RegisterRequest,
        address: Option<IpAddr>,
    ) -> Result<User, AppError> {
        // A registration costs a hash like a login, and is counted against where it
        // comes from like one. The turn is held until the user is stored.
        let _attempt = self.admit_registration(address)?;
        let name = &request.organization_name;
        let mut user = self.new_account(&request.account()).await?;
        let mut stored = if self.store().is_bootstrapped().await? {
            Err(StoreError::NotFirstUser)
        } else {
            #[cfg(feature = "test-util")]
            self.signups_meet().await;
            self.store().sign_up(&mut user, name, Founding::First).await
        };
        // Another registration was first after all, or one was long ago: an
        // organization of its own, where the server takes several.
        if matches!(stored, Err(StoreError::NotFirstUser))
            && self.config().takes_several_organizations()
        {
            stored = self.store().sign_up(&mut user, name, Founding::New).await;
        }
        match stored {
            Ok(workspace_id) => {
                self.metrics().registered();
                tracing::info!(
                    user_id = %user.id,
                    workspace_id,
                    first = user.is_admin,
                    "a user signed up and started an organization"
                );
                Ok(user)
            }
            Err(error) => Err(account_error(error)),
        }
    }

    /// Holds the next `count` sign-ups that find the instance without its first user
    /// until all of them have, and lets them on to the store together: so that two
    /// first sign-ups are under way at once, for the tests of who is first.
    #[cfg(feature = "test-util")]
    #[doc(hidden)]
    pub fn meet_next_signups(&self, count: usize) {
        *self
            .inner
            .signups_meet
            .lock()
            .expect("the sign-ups' meeting lock") =
            Some(std::sync::Arc::new(tokio::sync::Barrier::new(count)));
    }

    #[cfg(feature = "test-util")]
    async fn signups_meet(&self) {
        let meeting = self
            .inner
            .signups_meet
            .lock()
            .expect("the sign-ups' meeting lock")
            .clone();
        if let Some(meeting) = meeting
            && meeting.wait().await.is_leader()
        {
            // Met: the sign-ups after these are not held.
            self.inner
                .signups_meet
                .lock()
                .expect("the sign-ups' meeting lock")
                .take();
        }
    }

    /// Asks whether a registration from `address` may go ahead now, and holds its
    /// turn: of the registrations of an address, with an organization or by an invitation, one
    /// is under way at a time.
    pub(crate) fn admit_registration(
        &self,
        address: Option<IpAddr>,
    ) -> Result<crate::auth::limiter::Attempt, AppError> {
        self.login_limiter()
            .admit_registration(address, Instant::now())
            .map_err(|RetryAfter(wait)| AppError::rate_limited(wait))
    }

    /// The account `account` asks for, not stored yet: under a new identifier, with
    /// the hash of its password. The caller has checked and tidied the request
    /// (`NewAccount::validate`, `NewAccount::tidied`).
    ///
    /// # Errors
    ///
    /// Returns a conflict error if the name or the email address is taken, in any letter
    /// case, and a 429 if the server has too much password work to do.
    pub(crate) async fn new_account(&self, account: &NewAccount) -> Result<User, AppError> {
        // Asked first so that a taken name costs no hash. Two registrations at once
        // both pass these, and then the unique indexes decide: the same conflict.
        // A registration whose address is not confirmed yet holds its name and address
        // as an account does, and is told as one.
        if self
            .find_user_for_login(Lookup::Username(&account.username))
            .await?
            .is_some()
            || self.find_signup(&account.username, "").await?.is_some()
        {
            return Err(username_taken());
        }
        if self
            .find_user_for_login(Lookup::Email(&account.email))
            .await?
            .is_some()
            || self.find_signup("", &account.email).await?.is_some()
        {
            return Err(email_taken());
        }
        Ok(User {
            id: new_id(),
            username: account.username.clone(),
            email: account.email.clone(),
            display_name: account.display_name.clone(),
            password_hash: self.hash(&account.password).await?,
            ..User::default()
        })
    }

    /// Sets the password of the user with the given name, ends that user's sessions and
    /// closes their websocket connections, so that nobody stays signed in with the old
    /// password. For a password the server's operator sets, recorded as theirs in each
    /// organization of the user ([`fvoci_store::Store::reset_user_password`]); the
    /// caller has checked it.
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
        self.confirm_password(
            &user,
            old_password,
            WrongPassword {
                words: "the old password is wrong",
                field: "oldPassword",
            },
            address,
        )
        .await?;
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

/// How a password that does not confirm the account is refused: in which words, and
/// naming which member of the request.
#[derive(Debug, Clone, Copy)]
pub(crate) struct WrongPassword {
    pub(crate) words: &'static str,
    pub(crate) field: &'static str,
}

impl App {
    /// Checks that whoever holds a session of `user` knows the account's password,
    /// before something is done that the session alone is not to do.
    ///
    /// # Errors
    ///
    /// Returns a 429 with the time to wait if the password was got wrong too often, as
    /// for a login to the account by its name: a session is not the password, and
    /// whoever holds one must not get to guess at it without end. Returns 400 with the
    /// reason `invalid_credentials`, naming `wrong.field`, if the password is wrong.
    pub(crate) async fn confirm_password(
        &self,
        user: &User,
        password: &str,
        wrong: WrongPassword,
        address: Option<IpAddr>,
    ) -> Result<(), AppError> {
        // Counted with the logins to the account by its name, and one at a time from an
        // address, like them.
        let attempt = self
            .login_limiter()
            .admit_login(&[&user.username], address, Instant::now())
            .map_err(|RetryAfter(wait)| AppError::rate_limited(wait))?;
        let right = password.len() <= MAXIMUM_PASSWORD_BYTES
            && self.verify(&user.password_hash, password).await? != Verdict::Mismatch;
        if !right {
            tracing::debug!(user_id = %user.id, "invalid password for user");
            attempt.failed(Instant::now());
            return Err(AppError::bad_request(wrong.words)
                .with_reason(Reason::InvalidCredentials)
                .with_field(wrong.field));
        }
        attempt.succeeded(&user.username);
        Ok(())
    }
}

/// What a login looks a user up by.
#[derive(Debug, Clone, Copy)]
enum Lookup<'a> {
    Username(&'a str),
    Email(&'a str),
}

/// How a login that answered `result` ended, for the metrics: by the kind of its error,
/// which is the same for an unknown account and a wrong password.
fn login_outcome<T>(result: &Result<T, AppError>) -> LoginOutcome {
    match result {
        Ok(_) => LoginOutcome::Succeeded,
        Err(error) if error.reason() == Some(Reason::EmailUnverified) => {
            LoginOutcome::EmailUnverified
        }
        Err(error) => match error.kind() {
            ErrorKind::Unauthorized => LoginOutcome::InvalidCredentials,
            ErrorKind::RateLimited => LoginOutcome::Limited,
            _ => LoginOutcome::Failed,
        },
    }
}

/// Counts a login into the metrics when it is dropped: as it ended, or as given up if
/// its task was stopped first (the server shut down).
struct LoginEnding<'m> {
    metrics: &'m Metrics,
    outcome: LoginOutcome,
}

impl Drop for LoginEnding<'_> {
    fn drop(&mut self) {
        self.metrics.login_ended(self.outcome);
    }
}

/// The refusal of a login, whatever was wrong.
fn incorrect_login() -> AppError {
    AppError::unauthorized("incorrect login").with_reason(Reason::InvalidCredentials)
}

/// The refusal of a login whose password is right, of a registration whose address is
/// not confirmed yet.
fn email_unverified() -> AppError {
    AppError::forbidden("the email address is not confirmed yet")
        .with_reason(Reason::EmailUnverified)
}

/// What the store's refusal of a new account is to the client.
pub(crate) fn account_error(error: StoreError) -> AppError {
    match error {
        StoreError::Conflict(Taken::Email) => email_taken(),
        StoreError::Conflict(Taken::Username) => username_taken(),
        // The organization's name is its first workspace's, and a name is one
        // workspace's among those an organization has in use.
        StoreError::Conflict(Taken::WorkspaceName) => {
            AppError::conflict("another workspace of the organization has the name")
                .with_reason(Reason::WorkspaceNameTaken)
                .with_field("organizationName")
        }
        error @ StoreError::NotFirstUser => error.into(),
        error => AppError::from(error).context("unable to store the new account"),
    }
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

    pub(super) async fn hash(&self, password: &str) -> Result<String, AppError> {
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
    fn a_login_ends_in_one_of_five_outcomes_by_the_kind_of_its_error() {
        let ended = |result: Result<(), AppError>| login_outcome(&result);
        assert_eq!(ended(Ok(())), LoginOutcome::Succeeded);
        assert_eq!(
            ended(Err(incorrect_login())),
            LoginOutcome::InvalidCredentials
        );
        assert_eq!(
            ended(Err(email_unverified())),
            LoginOutcome::EmailUnverified
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
