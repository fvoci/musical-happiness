//! Recovering native accounts with one-hour, single-use reset links.

use std::net::IpAddr;
use std::time::Instant;

use super::mail::{Content, Language, Letter};
use crate::auth::limiter::RetryAfter;

use super::App;
use crate::auth::now_millis;
use crate::auth::secret::{new_secret, token_digest};
use crate::error::AppError;
use crate::metrics::ResetMailDrop;

impl App {
    /// Creates a reset link for an operator to deliver through the administration socket,
    /// recorded as theirs in each organization of the user
    /// ([`fvoci_store::Store::create_password_reset_by_operator`]).
    ///
    /// # Errors
    /// Returns not found for an unavailable account, or the storage error.
    pub async fn password_reset_link(&self, username: &str) -> Result<String, AppError> {
        let user = self.store().get_user_by_username(username).await?;
        self.reset_link_for(&user.id, Asked::ByOperator).await
    }

    async fn reset_link_for(&self, user_id: &str, asked: Asked) -> Result<String, AppError> {
        let token = new_secret();
        let (digest, now) = (token_digest(&token), now_millis());
        let store = self.store();
        let stored = match asked {
            Asked::ByOperator => {
                store
                    .create_password_reset_by_operator(user_id, &digest, now)
                    .await?
            }
            Asked::ByMail => store.create_password_reset(user_id, &digest, now).await?,
        };
        if !stored {
            return Err(AppError::not_found("account with a password"));
        }
        // Fragments never reach HTTP request targets, proxy logs or Referer headers.
        Ok(format!(
            "{}/reset-password#{token}",
            self.config().server_root.trim_end_matches('/')
        ))
    }

    /// Admits recovery identically for existing and absent accounts and returns unstarted work.
    /// The HTTP adapter releases this work only after handing off its response.
    ///
    /// Requests are limited by their address only, as logins are by theirs: a limit on
    /// the email would lock its owner out for whoever asks in their name. What one
    /// account is sent is limited after it is looked up, in the background, where the
    /// answer cannot tell (`mail_reset_per_account_hour`).
    ///
    /// # Errors
    /// Returns 404 without SMTP, 400 for a malformed email, or 429 for too many
    /// requests from this address, independently of login limits.
    pub fn prepare_password_reset(
        &self,
        email: &str,
        language: &str,
        address: Option<IpAddr>,
    ) -> Result<impl Future<Output = ()> + Send + 'static + use<>, AppError> {
        if !self.config().smtp.enabled() {
            return Err(AppError::not_found("route"));
        }
        let email = email.trim().to_lowercase();
        if !fvoci_model::is_email_valid(&email) {
            return Err(
                AppError::bad_request("a valid email address is required").with_field("email")
            );
        }
        // Counted against the address as it is admitted; nothing is counted at the email.
        let _request = self
            .inner
            .reset_limiter
            .admit_from_address(address, Instant::now())
            .map_err(|RetryAfter(wait)| AppError::rate_limited(wait))?;
        let app = self.clone();
        let requested = Language::written_in(language);
        Ok(async move {
            // The cause is the store's: what became of the message is told where it is sent.
            if let Err(error) = app.mail_password_reset(&email, requested).await {
                tracing::warn!(%error, "password recovery delivery failed");
            }
        })
    }

    async fn mail_password_reset(
        &self,
        email: &str,
        requested: Option<Language>,
    ) -> Result<(), AppError> {
        let user = match self.store().get_user_by_email(email).await {
            Ok(user) => user,
            Err(error) if error.is_not_found() => return Ok(()),
            Err(error) => return Err(error.into()),
        };
        if user.password_hash.is_empty() {
            return Ok(());
        }
        let settings = self.user_settings(&user.id).await?;
        let language =
            Language::first_of([Language::written_in(settings.language.as_str()), requested]);
        // The answer was the same either way; a dropped message is told only here.
        let queued = match self.inner.mail.queue_reset(&user.id) {
            Ok(queued) => queued,
            Err(reason) => {
                match reason {
                    ResetMailDrop::AccountLimit => {
                        tracing::info!(
                            "password recovery mail dropped: the account's hourly limit"
                        );
                    }
                    ResetMailDrop::QueueFull => {
                        tracing::warn!("password recovery mail dropped: its queue is full");
                    }
                }
                self.metrics().password_reset_mail_dropped(reason);
                return Ok(());
            }
        };
        // The link is made once the message has its turn: its hour starts as it leaves.
        queued
            .deliver(&self.inner.mail, || async {
                let link = self.reset_link_for(&user.id, Asked::ByMail).await?;
                Ok::<_, AppError>(Letter {
                    to: user.email.clone(),
                    content: Content::reset(&link, language),
                })
            })
            .await?;
        Ok(())
    }

    /// Sets the password proved by a reset token and closes every session and live
    /// connection. Who holds the token proved the mailbox, or was given the link by an
    /// operator, so the sign-in lock that others' guesses at the old password left on
    /// the account ends too, with the wait of this address at it.
    ///
    /// # Errors
    /// Returns 400 for an invalid password or unavailable token, 429 for too many
    /// attempts, or the storage error. An unavailable token never changes a password.
    pub async fn finish_password_reset(
        &self,
        token: &str,
        password: &str,
        address: Option<IpAddr>,
    ) -> Result<(), AppError> {
        let _attempt = self.admit_registration(address)?;
        fvoci_model::check_new_password(password)?;
        let hash = self.hash(password).await?;
        let user_id = self
            .store()
            .consume_password_reset(&token_digest(token), &hash, now_millis())
            .await?
            .ok_or_else(|| {
                AppError::bad_request("the reset link is invalid or expired").with_field("token")
            })?;
        self.ws().close_user(&user_id);
        match self.store().get_user_by_id(&user_id).await {
            Ok(user) => self
                .login_limiter()
                .forget_account(&[&user.username, &user.email], address),
            // The password is set; an account deleted meanwhile has nothing to unlock.
            Err(error) if error.is_not_found() => {}
            // The password is set: what is left of the lock ends on its own.
            Err(error) => tracing::warn!(%error, "the account's sign-in lock was not ended"),
        }
        Ok(())
    }
}

/// Who asked for a reset link: the server's operator, whose act each organization of
/// the user records, or the person by mail, which is no one's act on them.
#[derive(Debug, Clone, Copy)]
enum Asked {
    ByOperator,
    ByMail,
}
