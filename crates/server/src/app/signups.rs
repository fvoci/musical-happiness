//! Registrations that wait for their address to be confirmed (`email_verification`):
//! the link mailed to the address makes the account, starts what the registration
//! starts and signs its person in. Until then there is no account.

use std::net::IpAddr;

use fvoci_model::{RegisterRequest, Session, new_id};
use fvoci_store::{
    Confirmed, Founding, Proof, SIGNUP_LIFETIME_MILLIS, Signup, SignupStart, StoreError,
};

use super::App;
use super::auth::account_error;
use super::mail::{Content, Language, Letter};
use crate::auth::now_millis;
use crate::auth::secret::{new_secret, token_digest};
use crate::error::AppError;
use crate::metrics::ResetMailDrop;

impl App {
    /// Whether a registration opens nothing until the link mailed to its address
    /// confirms it (`email_verification`).
    #[must_use]
    pub fn verifies_email(&self) -> bool {
        self.config().mail.email_verification
    }

    /// Stores the registration of somebody who signs up, to be confirmed by the link
    /// mailed to its address, and mails that link in the background: nothing is made
    /// of it until then. `language` is the language of the screen it came from. The
    /// caller has tidied and checked the request, as for [`App::sign_up`].
    ///
    /// # Errors
    ///
    /// As [`App::sign_up`]: a name or an address that an account or another
    /// registration holds is the same conflict.
    pub async fn sign_up_to_confirm(
        &self,
        request: &RegisterRequest,
        language: &str,
        address: Option<IpAddr>,
    ) -> Result<(), AppError> {
        let _attempt = self.admit_registration(address)?;
        let user = self.new_account(&request.account()).await?;
        let now = now_millis();
        let signup = Signup {
            id: user.id,
            username: user.username,
            email: user.email,
            display_name: user.display_name,
            password_hash: user.password_hash,
            language: Language::written_in(language)
                .map(Language::tag)
                .unwrap_or_default()
                .to_owned(),
            start: SignupStart::Organization(request.organization_name.clone()),
            create_at: now,
            expires_at: now.saturating_add(SIGNUP_LIFETIME_MILLIS),
        };
        self.store()
            .create_signup(&signup, now)
            .await
            .map_err(account_error)?;
        tracing::info!(signup_id = %signup.id, "a registration waits for its address");
        let app = self.clone();
        self.spawn_background(async move { app.mail_signup_link(&signup, None).await });
        Ok(())
    }

    /// Mails the registration a new link that confirms it: the link it had confirms
    /// nothing any more. What became of the message is logged, never told: the answer
    /// that started it is the same whatever it was.
    pub(super) async fn mail_signup_link(&self, signup: &Signup, requested: Option<Language>) {
        let queued = match self.inner.mail.queue_verification(&signup.email) {
            Ok(queued) => queued,
            Err(ResetMailDrop::AccountLimit) => {
                tracing::info!("confirmation mail dropped: the address's hourly limit");
                return;
            }
            Err(ResetMailDrop::QueueFull) => {
                tracing::warn!("confirmation mail dropped: its queue is full");
                return;
            }
        };
        let language = Language::first_of([Language::written_in(&signup.language), requested]);
        let delivered = queued
            .deliver(
                &self.inner.mail,
                || async {
                    let link = self.renew_signup_link(&signup.id).await?;
                    Ok::<_, AppError>(Letter {
                        to: signup.email.clone(),
                        content: Content::verification(&link, language),
                    })
                },
                || async { Ok(true) },
            )
            .await;
        match delivered {
            Ok(_) => {}
            // Confirmed or ended while the message waited for its turn.
            Err(error) if error.is_not_found() => {
                tracing::debug!("confirmation mail not written: the registration is gone");
            }
            Err(error) => tracing::warn!(%error, "confirmation mail failed"),
        }
    }

    /// A new link for the registration, made as its message has its turn.
    async fn renew_signup_link(&self, signup_id: &str) -> Result<String, AppError> {
        let token = new_secret();
        let renewed = self
            .store()
            .renew_signup_link(signup_id, &token_digest(&token), now_millis())
            .await?;
        if !renewed {
            return Err(AppError::not_found("registration"));
        }
        // Fragments never reach HTTP request targets, proxy logs or Referer headers.
        Ok(format!(
            "{}/verify-email#{token}",
            self.config().server_root.trim_end_matches('/')
        ))
    }

    /// Confirms the registration of the link `token` is of: makes its account, starts
    /// its organization, and signs it in, whatever browser the link was opened in (the
    /// mailbox is the proof). Returns the token of the new session and where the
    /// account was let in.
    ///
    /// # Errors
    ///
    /// Returns 400 naming `token` for a link that confirms nothing (none, ended, used,
    /// or replaced by a newer one); 403 (`invitation_required`) if the server takes one
    /// organization and it has its first user now, which removes the registration;
    /// 429 for too many attempts from `address`; or the store's error.
    pub async fn confirm_signup(
        &self,
        token: &str,
        user_agent: &str,
        address: Option<IpAddr>,
    ) -> Result<(String, Confirmed), AppError> {
        let _attempt = self.admit_registration(address)?;
        let digest = token_digest(token);
        let confirmed = self.confirm(Proof::Link(&digest)).await?.ok_or_else(|| {
            AppError::bad_request("the confirmation link is invalid or expired").with_field("token")
        })?;
        let session_token = new_secret();
        let now = now_millis();
        let session = Session {
            id: new_id(),
            user_id: confirmed.user.id.clone(),
            created_at: now,
            expires_at: now.saturating_add(self.config().session_max_millis()),
            user_agent: user_agent.to_owned(),
            ..Session::default()
        };
        let stored = self
            .store()
            .create_session(
                &session,
                &token_digest(&session_token),
                &confirmed.user.password_hash,
            )
            .await
            .map_err(|error| AppError::from(error).context("unable to create session"))?;
        if !stored {
            // The password was set anew in the instant since: the account is there.
            return Err(AppError::conflict(
                "the account was changed in the meantime; sign in",
            ));
        }
        Ok((session_token, confirmed))
    }

    /// Confirms the registration `proof` names, starting its organization as the first
    /// of the server's, or a new one where the server takes several.
    async fn confirm(&self, proof: Proof<'_>) -> Result<Option<Confirmed>, AppError> {
        let now = now_millis();
        let mut confirmed = if self.store().is_bootstrapped().await? {
            Err(StoreError::NotFirstUser)
        } else {
            self.store()
                .confirm_signup(proof, Founding::First, now)
                .await
        };
        if matches!(confirmed, Err(StoreError::NotFirstUser)) {
            if !self.config().takes_several_organizations() {
                // Nobody else gets in without an invitation now: the registration is of
                // no use, and its name and address are free again. A link of none
                // confirms nothing, as anywhere.
                if !self.store().discard_signup(proof, now).await? {
                    return Ok(None);
                }
                return Err(AppError::invitation_required());
            }
            confirmed = self.store().confirm_signup(proof, Founding::New, now).await;
        }
        let confirmed = confirmed.map_err(account_error)?;
        if let Some(confirmed) = &confirmed {
            self.metrics().registered();
            tracing::info!(
                user_id = %confirmed.user.id,
                workspace_id = %confirmed.workspace_id,
                first = confirmed.user.is_admin,
                "a registration was confirmed and started an organization"
            );
        }
        Ok(confirmed)
    }
}
