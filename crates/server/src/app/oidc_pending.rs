//! The pending settings of an organization's provider, as its administrators keep them:
//! saved aside while the organization takes no password, tried by a sign-in through
//! them that makes them the provider's when it ends well, or thrown away. Settings that
//! do not work so lock nobody out: until the trial ends well, people sign in through
//! the provider as it was. The trial's start and end are with the other sign-ins, in
//! `oidc.rs`; the store's part is `Store::finish_oidc_trial`.

use std::net::IpAddr;

use fvoci_model::{OidcProvider, Session};
use fvoci_store::{Applied, PendingSettings, StoredProvider};

use super::App;
use super::oidc::{Start, StartedLogin};
use super::sso;
use crate::error::AppError;

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
        Ok(self.provider_view(&stored, Some(&pending), session))
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
        };
        self.start_oidc_flow(org_id, address, start).await
    }

    /// Follows the pending settings that became the provider's: the sessions of the
    /// issuer before end with their connections, and where the organization takes
    /// only the provider, now another one, who it shuts out is shut out.
    pub(super) async fn follow_applied(&self, applied: &Applied) {
        let (before, provider) = (&applied.before, &applied.provider);
        tracing::info!(org_id = %provider.org_id, issuer = %provider.issuer, "the pending settings of a provider were tried and taken");
        if before.issuer != provider.issuer {
            self.close_sessions_of_origin(&before.issuer);
        }
        if provider.sso_required {
            if provider.id != before.id {
                self.collab().put_out_shut_out(self, &provider.id).await;
            }
            self.close_connections_shut_out(provider).await;
        }
    }
}
