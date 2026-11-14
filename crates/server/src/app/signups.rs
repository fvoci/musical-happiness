//! Registrations that wait for their address to be confirmed (`email_verification`):
//! the link mailed to the address makes the account, starts what the registration
//! starts and signs its person in. Until then there is no account.

use std::net::IpAddr;
use std::time::Instant;

use fvoci_model::{RegisterRequest, Session, User, new_id};
use fvoci_store::{
    Accepted, Confirmed, Founding, InvitedConfirmation, Proof, SIGNUP_LIFETIME_MILLIS, Signup,
    SignupStart, StoreError,
};

use super::App;
use super::auth::account_error;
use super::mail::{Content, Language, Letter};
use crate::auth::limiter::RetryAfter;
use crate::auth::now_millis;
use crate::auth::secret::{new_secret, token_digest};
use crate::collab::AccessScope;
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
    /// mailed to its address, and mails that link in the background, in the language of
    /// the screen it came from: nothing is made of it until then. The caller has tidied
    /// and checked the request, as for [`App::sign_up`].
    ///
    /// # Errors
    ///
    /// As [`App::sign_up`]: a name or an address that an account or another
    /// registration holds is the same conflict.
    pub async fn sign_up_to_confirm(
        &self,
        request: &RegisterRequest,
        address: Option<IpAddr>,
    ) -> Result<(), AppError> {
        let _attempt = self.admit_registration(address)?;
        let user = self.new_account(&request.account()).await?;
        let now = now_millis();
        let start = SignupStart::Organization(request.organization_name.clone());
        let signup = waiting(&user, &request.language, start, now);
        self.store()
            .create_signup(&signup, now)
            .await
            .map_err(account_error)?;
        tracing::info!(signup_id = %signup.id, "a registration waits for its address");
        let app = self.clone();
        self.spawn_background(async move { app.mail_signup_link(&signup, None).await });
        Ok(())
    }

    /// The registration of `user`, an account asked for by the link `invitation_id` from
    /// a screen in `language`, to be stored as that link's (`reserve_invitation`). The
    /// registrations that ended go first: they hold no name, but their rows do.
    pub(super) async fn signup_by_link(
        &self,
        user: &User,
        invitation_id: &str,
        language: &str,
        now: i64,
    ) -> Result<Signup, AppError> {
        self.store().remove_ended_signups(now).await?;
        let start = SignupStart::Invitation {
            id: invitation_id.to_owned(),
            // The link's organization, which the store sets as it holds the link.
            org_id: String::new(),
        };
        Ok(waiting(user, language, start, now))
    }

    /// Admits a request for a new confirmation link identically whether or not a
    /// registration has the name or the address, and returns unstarted work: the HTTP
    /// adapter releases it only after handing off its response. The registration's
    /// address is sent no more than an account's recovery messages
    /// (`mail_reset_per_account_hour`, the first link counted), and further requests are
    /// answered the same and lock nothing. Requests are limited by where they come from
    /// (`address`), with the requests for a reset link. A registration made while
    /// `email_verification` was on still gets its link once it is off, while mail is sent.
    ///
    /// # Errors
    ///
    /// Returns 400 for a request that names neither a name nor an address, 404 where no
    /// mail is sent, and 429 for too many requests from
    /// `address`.
    pub fn prepare_signup_mail(
        &self,
        username: &str,
        email: &str,
        language: &str,
        address: Option<IpAddr>,
    ) -> Result<impl Future<Output = ()> + Send + 'static + use<>, AppError> {
        let (username, email) = (username.trim().to_owned(), email.trim().to_owned());
        if username.is_empty() && email.is_empty() {
            return Err(AppError::bad_request(
                "the user name or the email address of the registration is required",
            )
            .with_field("email"));
        }
        if !self.config().mail_enabled() {
            return Err(AppError::not_found("route"));
        }
        let _request = self
            .inner
            .reset_limiter
            .admit_from_address(address, Instant::now())
            .map_err(|RetryAfter(wait)| AppError::rate_limited(wait))?;
        let app = self.clone();
        let requested = Language::written_in(language);
        Ok(async move {
            match app.find_signup(&username, &email).await {
                Ok(Some(signup)) => app.mail_signup_link(&signup, requested).await,
                Ok(None) => {}
                Err(error) => tracing::warn!(%error, "a confirmation link was not resent"),
            }
        })
    }

    /// Removes the registrations that ended unconfirmed by `now` and the links that
    /// ended: their names and addresses are free again. For the housekeeping.
    ///
    /// # Errors
    ///
    /// Returns the store's error; nothing is removed then.
    pub async fn remove_ended_signups(&self, now: i64) -> Result<u64, AppError> {
        Ok(self.store().remove_ended_signups(now).await?)
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
        // The digest of the link the message carries, once it is written: a retry is
        // sent only while that link still confirms the registration.
        let digest = std::sync::Mutex::new(String::new());
        let delivered = queued
            .deliver(
                &self.inner.mail,
                || async {
                    let token = new_secret();
                    let link = self.renew_signup_link(&signup.id, &token).await?;
                    *digest
                        .lock()
                        .expect("the link's digest is never held across a panic") =
                        token_digest(&token);
                    Ok::<_, AppError>(Letter {
                        to: signup.email.clone(),
                        content: Content::verification(&link, &signup.username, language),
                    })
                },
                || async {
                    let digest = digest
                        .lock()
                        .expect("the link's digest is never held across a panic")
                        .clone();
                    Ok(self
                        .store()
                        .signup_link_stands(&signup.id, &digest, now_millis())
                        .await?)
                },
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

    /// A new link for the registration, of `token`, made as its message has its turn.
    async fn renew_signup_link(&self, signup_id: &str, token: &str) -> Result<String, AppError> {
        let renewed = self
            .store()
            .renew_signup_link(signup_id, &token_digest(token), now_millis())
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

    /// Confirms the registration with the user name `username`, in any letter case, on
    /// the word of the server's operator (the administration socket), for a person whose
    /// mail does not arrive: as its link would, but signing nobody in. A registration
    /// made by a link for members is recorded as the operator's in the organization it
    /// joins; one of its own starts its organization only now, and a guest is no member
    /// of the organization, so neither is recorded. Returns the account's identifier.
    ///
    /// # Errors
    ///
    /// Returns 404 if no registration that waits has the name, the 404 of
    /// [`App::accept_invitation`] if its link lets nobody in any more (and the
    /// registration is removed), and as [`App::confirm_signup`] otherwise.
    pub async fn confirm_signup_by_operator(&self, username: &str) -> Result<String, AppError> {
        let confirmed = self
            .confirm(Proof::Operator(username))
            .await?
            .ok_or_else(|| AppError::not_found("registration"))?;
        Ok(confirmed.user.id)
    }

    /// Confirms the registration `proof` names: starts its organization, or lets its
    /// account in by the link it was made by.
    async fn confirm(&self, proof: Proof<'_>) -> Result<Option<Confirmed>, AppError> {
        let now = now_millis();
        let Some(signup) = self.store().signup_of(proof, now).await? else {
            return Ok(None);
        };
        let confirmed = match &signup.start {
            SignupStart::Organization(_) => self.confirm_founding(&signup, proof, now).await?,
            SignupStart::Invitation { id, org_id } => {
                self.confirm_invited(&signup, proof, (id, org_id), now)
                    .await?
            }
        };
        if let Some(confirmed) = &confirmed {
            self.metrics().registered();
            tracing::info!(
                user_id = %confirmed.user.id,
                workspace_id = %confirmed.workspace_id,
                first = confirmed.user.is_admin,
                "a registration was confirmed"
            );
        }
        Ok(confirmed)
    }

    /// Confirms a registration of its own, starting its organization as the first of the
    /// server's, or a new one where the server takes several.
    async fn confirm_founding(
        &self,
        signup: &Signup,
        proof: Proof<'_>,
        now: i64,
    ) -> Result<Option<Confirmed>, AppError> {
        let store = self.store();
        let mut confirmed = if store.is_bootstrapped().await? {
            Err(StoreError::NotFirstUser)
        } else {
            store
                .confirm_signup(signup, proof, Founding::First, now)
                .await
        };
        let one_organization = !self.config().takes_several_organizations();
        if matches!(confirmed, Err(StoreError::NotFirstUser)) {
            if one_organization {
                // Nobody else gets in without an invitation now: the registration is of
                // no use, and its name and address are free again. A link of none
                // confirms nothing, as anywhere.
                if !store.discard_signup(proof, now).await? {
                    return Ok(None);
                }
                return Err(AppError::invitation_required());
            }
            confirmed = store
                .confirm_signup(signup, proof, Founding::New, now)
                .await;
        } else if one_organization && confirmed.as_ref().is_ok_and(Option::is_some) {
            // The first user is in: no other registration of its own can start anything
            // on this server, and their names and addresses are free for invitations.
            let removed = store.remove_founding_signups().await?;
            if removed > 0 {
                tracing::info!(
                    removed,
                    "registrations that could start nothing were removed"
                );
            }
        }
        confirmed.map_err(account_error)
    }

    /// Confirms a registration made by the link `link` (its identifier and its
    /// organization's), letting its account in as an acceptance would.
    ///
    /// # Errors
    ///
    /// Returns 404 (`invitation_unavailable`) if the link lets nobody in any more: the
    /// registration is removed, and its use of the link given back.
    async fn confirm_invited(
        &self,
        signup: &Signup,
        proof: Proof<'_>,
        (link_id, org_id): (&str, &str),
        now: i64,
    ) -> Result<Option<Confirmed>, AppError> {
        let Some(app) = self.enter_org_for_sign_in(org_id) else {
            return Err(AppError::internal(
                "the organization of a registration as the store has it is no identifier",
            ));
        };
        for _ in 0..super::invitations::ATTEMPTS {
            let found = app.maker_of_link(link_id).await?;
            let inviter = found
                .as_ref()
                .map(|(invitation, maker)| maker.inviter(&invitation.created_by));
            let outcome = app
                .store()
                .confirm_invited_signup(signup, proof, inviter, now)
                .await
                .map_err(account_error)?;
            match outcome {
                InvitedConfirmation::Confirmed(confirmed) => {
                    let accepted = Accepted {
                        user_id: confirmed.user.id.clone(),
                        workspace_id: confirmed.workspace_id.clone(),
                        board_id: confirmed.board_id.clone(),
                        used: true,
                    };
                    app.collab()
                        .recheck_access(
                            &app,
                            AccessScope::workspace(&accepted.workspace_id)
                                .of_user(&accepted.user_id),
                        )
                        .await;
                    app.announce_admission(&accepted).await;
                    return Ok(Some(*confirmed));
                }
                InvitedConfirmation::Unavailable => {
                    return Err(AppError::invitation_unavailable());
                }
                InvitedConfirmation::Gone => return Ok(None),
                // Whether the maker may still invite is asked again, of what they are now.
                InvitedConfirmation::InviterChanged => {}
            }
        }
        // Changed every time: as good as no link for now, but the registration stays,
        // to be confirmed again once its maker holds still.
        Err(AppError::invitation_unavailable())
    }
}

/// The registration of `user`, made now from a screen in `language`, to start `start`.
fn waiting(user: &User, language: &str, start: SignupStart, now: i64) -> Signup {
    Signup {
        id: user.id.clone(),
        username: user.username.clone(),
        email: user.email.clone(),
        display_name: user.display_name.clone(),
        password_hash: user.password_hash.clone(),
        language: Language::written_in(language)
            .map(Language::tag)
            .unwrap_or_default()
            .to_owned(),
        start,
        create_at: now,
        expires_at: now.saturating_add(SIGNUP_LIFETIME_MILLIS),
    }
}
