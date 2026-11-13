//! Recovering native accounts with one-hour, single-use reset links.

use std::net::IpAddr;
use std::time::Instant;

use super::mail::{Content, Language};
use crate::auth::limiter::RetryAfter;

use super::App;
use crate::auth::now_millis;
use crate::auth::secret::{new_secret, token_digest};
use crate::error::AppError;

impl App {
    /// Creates a reset link for an operator to deliver through the administration socket.
    ///
    /// # Errors
    /// Returns not found for an unavailable account, or the storage error.
    pub async fn password_reset_link(&self, username: &str) -> Result<String, AppError> {
        let user = self.store().get_user_by_username(username).await?;
        self.reset_link_for(&user.id).await
    }

    async fn reset_link_for(&self, user_id: &str) -> Result<String, AppError> {
        let token = new_secret();
        if !self
            .store()
            .create_password_reset(user_id, &token_digest(&token), now_millis())
            .await?
        {
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
    /// # Errors
    /// Returns 404 without SMTP, 400 for a malformed email, or 429 for too many
    /// requests from this address or for this email, independently of login limits.
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
        let attempt = self
            .inner
            .reset_limiter
            .admit_login(&[&email], address, Instant::now())
            .map_err(|RetryAfter(wait)| AppError::rate_limited(wait))?;
        // Count every request equally. Success never resets the email's request count.
        attempt.failed(Instant::now());
        let app = self.clone();
        let language = Language::of(language);
        Ok(async move {
            if app.mail_password_reset(&email, language).await.is_err() {
                tracing::warn!("password recovery delivery failed");
            }
        })
    }

    async fn mail_password_reset(&self, email: &str, fallback: Language) -> Result<(), AppError> {
        let user = match self.store().get_user_by_email(email).await {
            Ok(user) => user,
            Err(error) if error.is_not_found() => return Ok(()),
            Err(error) => return Err(error.into()),
        };
        if user.password_hash.is_empty() {
            return Ok(());
        }
        let settings = self.user_settings(&user.id).await?;
        let language = Language::recipient(settings.language.as_str(), fallback);
        let delivery = self
            .inner
            .mail
            .delivery()
            .await
            .map_err(|_| AppError::internal("mail delivery is unavailable"))?;
        let link = self.reset_link_for(&user.id).await?;
        delivery
            .send(&user.email, Content::reset(&link, language))
            .await
            .map_err(|_| AppError::internal("SMTP delivery failed after three attempts"))
    }

    /// Sets the password proved by a reset token and closes every session and live connection.
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
        Ok(())
    }
}
