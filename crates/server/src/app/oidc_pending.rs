//! The pending settings of an organization's provider, as its administrators keep them:
//! saved aside while the organization takes no password, tried by a sign-in through
//! them that makes them the provider's when it ends well, or thrown away. Settings that
//! do not work so lock nobody out: until the trial ends well, people sign in through
//! the provider as it was. The trial's start and end are with the other sign-ins, in
//! `oidc.rs`; the store's part is `Store::finish_oidc_trial`.

use std::net::IpAddr;

use fvoci_model::{OidcProvider, Session};
use fvoci_store::{
    Applied, Binding, Identity, LinkingSession, PendingSettings, StoredProvider, Trial, Tried,
};

use super::App;
use super::oidc::{LoginFailure, Start, StartedLogin, TrialEnd, logged, refused};
use super::oidc_settings::Beside;
use super::sso;
use crate::auth::now_millis;
use crate::auth::oidc::provider::IdClaims;
use crate::auth::secret::token_digest;
use crate::error::{AppError, Reason};

/// How long before a trial comes back its administrator may have signed in through
/// the provider, at the most, for the trial to tie an identity that is nobody's to
/// their account, in milliseconds: who only holds an older session binds nothing.
pub(super) const FRESH_SIGN_IN_MILLIS: i64 = 5 * 60 * 1000;

impl App {
    /// Saves the sign-in of `next` (its issuer, client and secret) as the pending
    /// settings of `enforced`, the provider as it was read, which takes no password;
    /// what else `next` changes (whether newcomers join) is stored at once.
    ///
    /// # Errors
    ///
    /// Returns 409 (`sso_enforced`, naming `enabled`) for a provider switched off with
    /// it, 409 if the provider changed meanwhile, and the store's error.
    pub(super) async fn save_pending(
        &self,
        session: &Session,
        enforced: &StoredProvider,
        next: StoredProvider,
    ) -> Result<OidcProvider, AppError> {
        if !next.enabled {
            return Err(sso::enforced().with_field("enabled"));
        }
        let settings = PendingSettings {
            issuer: next.issuer.clone(),
            client_id: next.client_id.clone(),
            sealed_client_secret: next.sealed_client_secret.clone(),
        };
        let (stored, pending) = self
            .store()
            .put_oidc_pending_provider(&settings, &next, enforced, &session.user_id)
            .await?
            .ok_or_else(|| {
                AppError::conflict("the provider was changed meanwhile; read it and try again")
            })?;
        tracing::info!(org_id = %stored.org_id, issuer = %pending.issuer, "settings of a provider were saved aside, to be tried");
        let former = self.store().oidc_former_provider(now_millis()).await?;
        let beside = Beside {
            pending: Some(&pending),
            former: former.as_ref(),
        };
        Ok(self.provider_view(&stored, beside, session))
    }

    /// Throws the pending settings of the handle's organization's provider away, as
    /// the administrator `actor_id`. Throwing none away changes nothing.
    ///
    /// # Errors
    ///
    /// Returns the store's error.
    pub async fn discard_oidc_pending(&self, actor_id: &str) -> Result<(), AppError> {
        if self.store().discard_oidc_pending_provider(actor_id).await? {
            tracing::info!("the pending settings of a provider were thrown away");
        }
        Ok(())
    }

    /// Starts a sign-in through the pending settings of the provider of the
    /// organization `org_id`, which the handle reaches, for the administrator of
    /// `session`: when it ends well, in their account, they become the provider's, and
    /// the session it makes proves them.
    ///
    /// # Errors
    ///
    /// Returns 409 with the reason `oidc_unavailable` if there are none, the provider is
    /// off, or their issuer cannot be reached, and 429 for too many sign-ins from
    /// `address`.
    pub async fn start_oidc_trial(
        &self,
        org_id: &str,
        session: &Session,
        address: Option<IpAddr>,
    ) -> Result<StartedLogin, AppError> {
        let start = Start::Trial {
            user_id: &session.user_id,
            session_id: &session.id,
        };
        self.start_oidc_flow(org_id, address, start).await
    }

    /// The trial that `end` ends, of the pending settings of `stored`, as the store is
    /// asked to end it.
    pub(super) fn trial_of<'a>(&self, stored: &'a StoredProvider, end: TrialEnd<'a>) -> Trial<'a> {
        Trial {
            provider_id: &stored.id,
            pending_at: end.pending_at,
            trier_id: end.trier_id,
            keep_former: self.limits().former_provider_kept,
        }
    }

    /// Ends a trial of the pending settings `stored` (the provider with them) that
    /// vouched, by `claims`, for an identity tied to no account, on the handle of
    /// its organization: the identity is tied to the account of the administrator
    /// who started the trial, and the settings applied, if the browser came back
    /// with the session the trial was started from and that session was made through
    /// the provider as it is no more than [`FRESH_SIGN_IN_MILLIS`] before this
    /// moment: the trial is to come back within them. What decides is read where it is stored
    /// (`Store::finish_oidc_trial`). No address is compared: who signed in through
    /// the provider a moment ago is who tries.
    pub(super) async fn bind_by_trial(
        &self,
        claims: &IdClaims,
        stored: &StoredProvider,
        end: TrialEnd<'_>,
        session: &mut Session,
        digest: &str,
    ) -> Result<Applied, LoginFailure> {
        let Some(token) = end.came_with.filter(|token| !token.is_empty()) else {
            tracing::info!(org_id = %stored.org_id, "a trial of pending settings vouched for an identity tied to no account, and came back without a session");
            return Err(Reason::OidcTrialStale.into());
        };
        let now = now_millis();
        let binding = Binding {
            identity: Identity {
                issuer: &claims.issuer,
                subject: &claims.subject,
            },
            provider_email: claims.email.as_deref().unwrap_or_default(),
            session: LinkingSession {
                session_id: end.session_id,
                token_digest: &token_digest(token),
                now,
                idle_since: now.saturating_sub(self.config().session_idle_millis()),
            },
            made_since: now.saturating_sub(FRESH_SIGN_IN_MILLIS),
        };
        let applied = self
            .store()
            .finish_oidc_trial(
                self.trial_of(stored, end),
                Tried::Unbound(binding),
                session,
                digest,
            )
            .await
            .map_err(|error| logged(&error, "storing a trial"))?
            .map_err(|refusal| refused(refusal, stored))?;
        tracing::info!(user_id = %end.trier_id, org_id = %stored.org_id, issuer = %claims.issuer, "an administrator tied their identity at another issuer to their account by a trial of its settings");
        Ok(applied)
    }

    /// Follows the pending settings that became the provider's: the sessions of the
    /// issuer before end with their connections, and where the organization takes
    /// only the provider, now another one, who it shuts out is shut out.
    pub(super) async fn follow_applied(&self, applied: &Applied) {
        let (before, provider) = (&applied.before, &applied.provider);
        tracing::info!(org_id = %provider.org_id, issuer = %provider.issuer, "the pending settings of a provider were tried and taken");
        if before.issuer != provider.issuer {
            self.close_sessions_of_origin(before);
        }
        if provider.sso_required {
            if provider.id != before.id {
                self.collab().put_out_shut_out(self, &provider.id).await;
            }
            self.end_subscriptions_shut_out(provider).await;
        }
    }
}
