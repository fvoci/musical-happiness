//! An organization's provider as its administrators set it: reading it, saving it
//! (after reading the discovery document of a new issuer), removing it, and the client
//! secret, which is sealed for the organization and never answered. Signing in through
//! it is in `oidc.rs`; its pending settings, in `oidc_pending.rs`.

use fvoci_model::{
    OidcPendingProvider, OidcProvider, OidcProviderSettings, RefusalReason, Refusals, Session,
};
use fvoci_store::{PendingProvider, Removal, StoredProvider};

use super::App;
use super::sso;
use crate::auth::oidc::provider;
use crate::auth::sealed::SecretKey;
use crate::error::{AppError, Reason};

impl App {
    /// The provider of the handle's organization, for its administrators.
    ///
    /// # Errors
    ///
    /// Returns the store's error.
    pub async fn oidc_provider(&self, session: &Session) -> Result<Option<OidcProvider>, AppError> {
        let Some(stored) = self.store().oidc_provider().await? else {
            return Ok(None);
        };
        let pending = self.store().oidc_pending_provider().await?;
        Ok(Some(self.provider_view(&stored, pending.as_ref(), session)))
    }

    /// Sets the provider of the organization `org_id`, which the handle reaches, after
    /// reading the discovery document of an issuer that is new to the organization, to
    /// see that it is what it says.
    ///
    /// # Errors
    ///
    /// Returns 400 for settings that are refused or an issuer that is not verified,
    /// 409 (`secret_key_required`) if the server has no key to seal the secret with,
    /// 409 (`sso_unproven`, `sso_enforced`) for what the organization's taking no
    /// password does not let `session` do (`sso::check_change`), and the store's
    /// error.
    pub async fn put_oidc_provider(
        &self,
        org_id: &str,
        session: &Session,
        settings: &OidcProviderSettings,
    ) -> Result<OidcProvider, AppError> {
        settings.validate()?;
        let Some(key) = &self.config().secret_key else {
            return Err(AppError::conflict(
                "the server has no secret_key to seal the client secret with",
            )
            .with_reason(Reason::SecretKeyRequired));
        };
        let existing = self.store().oidc_provider().await?;
        // The issuer that stays was read when it was set. Not read again: a change of
        // what else there is, taking a password again among it, does not wait for a
        // provider that does not answer.
        let issuer_stays = existing
            .as_ref()
            .is_some_and(|existing| existing.issuer == settings.issuer);
        if !issuer_stays
            && let Err(error) = provider::discover(self.oidc_fetcher(), &settings.issuer).await
        {
            tracing::info!(%error, "the issuer of a provider could not be verified");
            return Err(AppError::bad_request(format_args!(
                "the issuer could not be verified: {error}"
            ))
            .with_reason(Reason::OidcIssuerUnverified)
            .with_field("issuer"));
        }
        let kept = existing
            .as_ref()
            .map(|existing| (existing.auto_join, existing.enabled));
        let sealed_client_secret = sealed_secret(key, org_id, settings, existing.as_ref())?;
        let next = StoredProvider {
            id: String::new(),
            org_id: org_id.to_owned(),
            issuer: settings.issuer.clone(),
            client_id: settings.client_id.clone(),
            sealed_client_secret,
            // Left out, what is stored stays, as the secret does, also for another
            // client; on when the organization has none.
            auto_join: settings
                .auto_join
                .or(kept.map(|kept| kept.0))
                .unwrap_or(true),
            enabled: settings.enabled.or(kept.map(|kept| kept.1)).unwrap_or(true),
            create_at: 0,
            update_at: 0,
            // Left out, it stays, also for another client, which is then refused: off
            // when the organization has none.
            sso_required: settings
                .sso_required
                .or(existing.as_ref().map(|existing| existing.sso_required))
                .unwrap_or(false),
            revision: 0,
        };
        // While the organization takes no password and goes on taking none, other
        // settings of signing in are kept aside, to be tried before they are taken.
        if let Some(enforced) = existing
            .as_ref()
            .filter(|existing| existing.sso_required && next.sso_required)
            && !is_same_sign_in(enforced, &next)
        {
            return self.save_pending(session, enforced, next).await;
        }
        // Decided from the provider as it was read; the store takes the change only if
        // that is still the provider, so no other change slips between the two.
        sso::check_change(session, existing.as_ref(), &next)?;
        let stored = self
            .store()
            .put_oidc_provider_as_read(&next, existing.as_ref(), &session.user_id)
            .await?
            // Changed by another request since it was read here: what was kept from it
            // may be kept no more.
            .ok_or_else(|| {
                AppError::conflict("the provider was changed meanwhile; read it and try again")
            })?;
        tracing::info!(org_id, issuer = %stored.issuer, "the provider of an organization was set");
        if let Some(existing) = &existing
            && existing.issuer != stored.issuer
        {
            self.close_sessions_of_origin(existing);
        }
        sso::record_if_changed(existing.as_ref(), &stored, &session.user_id);
        if stored.sso_required {
            if !existing
                .as_ref()
                .is_some_and(|existing| existing.sso_required)
            {
                // Begun now: who it shuts out is out of the rooms of its cards' bodies
                // before the request answers. A connection in one may follow no workspace.
                self.collab().put_out_shut_out(self, &stored.id).await;
            }
            self.close_connections_shut_out(&stored).await;
        }
        // Pending settings go with a change of what signing in takes.
        let pending = self.store().oidc_pending_provider().await?;
        Ok(self.provider_view(&stored, pending.as_ref(), session))
    }

    /// Removes the provider of the handle's organization, and the sign-ins under way
    /// through it.
    ///
    /// # Errors
    ///
    /// Returns 409 with the reason `sso_enforced` while the organization takes no
    /// password, and the store's error.
    pub async fn delete_oidc_provider(&self, actor_id: &str) -> Result<(), AppError> {
        match self.store().delete_oidc_provider(actor_id).await? {
            Removal::Removed { provider } => {
                // The store ended the sessions made through it: what they had open, their
                // seats in the rooms among it, ends with them.
                self.close_sessions_of_origin(&provider);
                tracing::info!("the provider of an organization was removed");
            }
            Removal::NoneThere => {}
            Removal::Enforced => return Err(sso::enforced()),
        }
        Ok(())
    }

    /// Closes the connections of the sessions made through a provider of `provider`'s
    /// organization under its issuer, which the store ended with it.
    pub(super) fn close_sessions_of_origin(&self, provider: &StoredProvider) {
        self.ws()
            .close_sessions_of_origin(&provider.org_id, &provider.issuer);
    }

    pub(super) fn provider_view(
        &self,
        stored: &StoredProvider,
        pending: Option<&PendingProvider>,
        session: &Session,
    ) -> OidcProvider {
        OidcProvider {
            issuer: stored.issuer.clone(),
            client_id: stored.client_id.clone(),
            has_client_secret: !stored.sealed_client_secret.is_empty(),
            auto_join: stored.auto_join,
            enabled: stored.enabled,
            sso_required: stored.sso_required,
            can_require_sso: sso::proves(session, stored),
            redirect_uri: self.oidc_redirect_uri(&stored.org_id),
            create_at: stored.create_at,
            update_at: stored.update_at,
            pending: pending
                .filter(|pending| pending.provider_id == stored.id)
                .map(|pending| OidcPendingProvider {
                    issuer: pending.issuer.clone(),
                    client_id: pending.client_id.clone(),
                    has_client_secret: !pending.sealed_client_secret.is_empty(),
                    create_at: pending.create_at,
                }),
        }
    }
}

/// Whether signing in through `next` takes the same issuer, client and secret as
/// through `stored`.
fn is_same_sign_in(stored: &StoredProvider, next: &StoredProvider) -> bool {
    stored.issuer == next.issuer
        && stored.client_id == next.client_id
        && stored.sealed_client_secret == next.sealed_client_secret
}

/// The client secret to store with the settings, sealed for the organization: the one
/// given; or, left out or given as it is stored, the stored one while the issuer and
/// the client stay, so that the provider's revision stays with it.
///
/// # Errors
///
/// Returns 400 naming `clientSecret` if it is left out for another issuer or client
/// while one is stored: the stored secret is the old client's.
fn sealed_secret(
    key: &SecretKey,
    org_id: &str,
    settings: &OidcProviderSettings,
    existing: Option<&StoredProvider>,
) -> Result<String, AppError> {
    Ok(match settings.client_secret.as_deref() {
        // The secret stays with the client it was given for: one that went on to
        // another issuer or client would be sent to it.
        None => match existing {
            Some(existing)
                if existing.issuer == settings.issuer
                    && existing.client_id == settings.client_id =>
            {
                existing.sealed_client_secret.clone()
            }
            Some(existing) if !existing.sealed_client_secret.is_empty() => {
                let mut refused = Refusals::new();
                refused.refuse(
                    "clientSecret",
                    RefusalReason::Required,
                    "clientSecret is required with another issuer or client",
                );
                return Err(refused.into());
            }
            _ => String::new(),
        },
        Some("") => String::new(),
        Some(secret) => match existing {
            Some(existing) if is_stored_secret(key, existing, settings, secret) => {
                existing.sealed_client_secret.clone()
            }
            _ => key.seal(
                secret,
                &secret_context(org_id, &settings.issuer, &settings.client_id),
            ),
        },
    })
}

/// Whether `secret` is the one `existing` keeps for the issuer and client of
/// `settings`: given again, it is no change.
fn is_stored_secret(
    key: &SecretKey,
    existing: &StoredProvider,
    settings: &OidcProviderSettings,
    secret: &str,
) -> bool {
    existing.issuer == settings.issuer
        && existing.client_id == settings.client_id
        && !existing.sealed_client_secret.is_empty()
        && key
            .open(
                &existing.sealed_client_secret,
                &secret_context(&existing.org_id, &existing.issuer, &existing.client_id),
            )
            .is_ok_and(|stored| stored == secret)
}

/// What a sealed client secret is bound to: the organization, the issuer and the client
/// it is for. Moved to another row, or kept for another issuer, it does not open.
pub fn secret_context(org_id: &str, issuer: &str, client_id: &str) -> Vec<u8> {
    // Each part is length-prefixed, so no two triples give the same bytes; the length
    // is eight bytes on every platform.
    let mut context = b"oidc client secret".to_vec();
    for part in [org_id, issuer, client_id] {
        let length = u64::try_from(part.len()).unwrap_or(u64::MAX);
        context.extend_from_slice(&length.to_be_bytes());
        context.extend_from_slice(part.as_bytes());
    }
    context
}
