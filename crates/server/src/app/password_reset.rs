//! Recovering native accounts with one-hour, single-use reset links.

use std::net::IpAddr;

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
        let token = new_secret();
        if !self
            .store()
            .create_password_reset(&user.id, &token_digest(&token), now_millis())
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
