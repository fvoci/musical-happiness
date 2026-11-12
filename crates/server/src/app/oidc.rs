//! Signing in through an organization's `OpenID Connect` provider: the provider as its
//! administrators set it, what a login page shows, and a sign-in from its start to
//! the session.
//!
//! A person is known by the issuer of the provider and the subject it gives them. An
//! account is never found by its email address: a person who signs in for the first
//! time gets an account of their own, and if their address is another account's the
//! sign-in is refused rather than joined to it.

use std::net::IpAddr;
use std::time::Instant;

use fvoci_model::{
    LoginOptions, OidcProvider, OidcProviderSettings, RefusalReason, Refusals, Session, User,
    display_name, is_email_valid, new_id,
};
use fvoci_store::{Identity, Join, LoginFlow, StoreError, StoredProvider, Taken};
use subtle::ConstantTimeEq as _;

use super::App;
use crate::auth::limiter::RetryAfter;
use crate::auth::now_millis;
use crate::auth::oidc::provider::{
    self, Client, Discovery, Expected, Grant, IdClaims, ProviderError,
};
use crate::auth::secret::{new_secret, token_digest};
use crate::error::{AppError, ErrorKind, Reason};

/// The path, under the server's address, that a provider sends a browser back to: one
/// for every organization.
pub const CALLBACK_PATH: &str = "/api/v1/auth/oidc/callback";

/// How long a sign-in may take from its start to its return, in milliseconds.
pub const LOGIN_FLOW_LIFETIME_MILLIS: i64 = 10 * 60 * 1000;

/// The scopes a sign-in asks for: the ID token, with the email address and the name.
const SCOPES: &str = "openid email profile";

/// The longest user name made from what a provider says.
const USERNAME_MAX_CHARS: usize = 64;

/// Why a sign-in through a provider did not end in a session: what the login page is
/// told, by the name of the problem.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoginFailure {
    /// The reason, one of the `Oidc…` ones.
    Reason(Reason),
    /// Too many sign-ins from where this one comes from.
    RateLimited,
}

impl LoginFailure {
    /// The name the login page gets in its `oidc_error` parameter.
    pub fn name(self) -> String {
        match self {
            Self::Reason(reason) => reason.name(),
            Self::RateLimited => ErrorKind::RateLimited.name().to_owned(),
        }
    }
}

impl From<Reason> for LoginFailure {
    fn from(reason: Reason) -> Self {
        Self::Reason(reason)
    }
}

/// A sign-in that was started: where the browser goes, and the secret its cookie keeps
/// so that only this browser can finish it.
#[derive(Clone)]
pub struct StartedLogin {
    /// The provider's address to send the browser to.
    pub authorization_url: String,
    /// The secret for the browser's cookie.
    pub browser_secret: String,
}

impl std::fmt::Debug for StartedLogin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StartedLogin").finish_non_exhaustive()
    }
}

/// What a browser brings back from the provider.
#[derive(Clone, Default)]
pub struct Callback {
    /// The `state` parameter.
    pub state: String,
    /// The `code` parameter.
    pub code: String,
    /// The `error` parameter: the provider did not let the person sign in.
    pub error: Option<String>,
    /// The secret of the browser's cookie.
    pub browser_secret: String,
}

impl std::fmt::Debug for Callback {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Callback")
            .field("error", &self.error)
            .finish_non_exhaustive()
    }
}

/// A sign-in that ended in a session: the token of the session.
#[derive(Clone)]
pub struct FinishedLogin {
    /// The token of the new session.
    pub token: String,
}

impl std::fmt::Debug for FinishedLogin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FinishedLogin").finish_non_exhaustive()
    }
}

impl App {
    /// The address a provider sends a browser back to: from the server's own address
    /// (`serverRoot`), never from what a request says its host is.
    pub fn oidc_redirect_uri(&self) -> String {
        format!(
            "{}{CALLBACK_PATH}",
            self.config().server_root.trim_end_matches('/')
        )
    }

    /// The provider of the handle's organization, for its administrators.
    ///
    /// # Errors
    ///
    /// Returns the store's error.
    pub async fn oidc_provider(&self) -> Result<Option<OidcProvider>, AppError> {
        Ok(self
            .store()
            .oidc_provider()
            .await?
            .map(|stored| self.provider_view(&stored)))
    }

    /// Sets the provider of the organization `org_id`, which the handle reaches, after
    /// reading its discovery document to see that the issuer is what it says.
    ///
    /// # Errors
    ///
    /// Returns 400 for settings that are refused or an issuer that is not verified,
    /// 409 (`secret_key_required`) if the server has no key to seal the secret with,
    /// and the store's error.
    pub async fn put_oidc_provider(
        &self,
        org_id: &str,
        settings: &OidcProviderSettings,
    ) -> Result<OidcProvider, AppError> {
        settings.validate()?;
        let Some(key) = &self.config().secret_key else {
            return Err(AppError::conflict(
                "the server has no secret_key to seal the client secret with",
            )
            .with_reason(Reason::SecretKeyRequired));
        };
        if let Err(error) = provider::discover(self.oidc_fetcher(), &settings.issuer).await {
            tracing::info!(%error, "the issuer of a provider could not be verified");
            return Err(AppError::bad_request(format_args!(
                "the issuer could not be verified: {error}"
            ))
            .with_reason(Reason::OidcIssuerUnverified)
            .with_field("issuer"));
        }
        let existing = self.store().oidc_provider().await?;
        let sealed_client_secret = match settings.client_secret.as_deref() {
            // The secret stays with the client it was given for: one that went on to
            // another issuer or client would be sent to it.
            None => match existing {
                Some(existing)
                    if existing.issuer == settings.issuer
                        && existing.client_id == settings.client_id =>
                {
                    existing.sealed_client_secret
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
            Some(secret) => key.seal(
                secret,
                &secret_context(org_id, &settings.issuer, &settings.client_id),
            ),
        };
        let stored = self
            .store()
            .put_oidc_provider(&StoredProvider {
                id: String::new(),
                org_id: org_id.to_owned(),
                issuer: settings.issuer.clone(),
                client_id: settings.client_id.clone(),
                sealed_client_secret,
                auto_join: settings.auto_join.unwrap_or(true),
                enabled: settings.enabled.unwrap_or(true),
                create_at: 0,
                update_at: 0,
            })
            .await?;
        tracing::info!(org_id, issuer = %stored.issuer, "the provider of an organization was set");
        Ok(self.provider_view(&stored))
    }

    /// Removes the provider of the handle's organization, and the sign-ins under way
    /// through it.
    ///
    /// # Errors
    ///
    /// Returns the store's error.
    pub async fn delete_oidc_provider(&self) -> Result<(), AppError> {
        if self.store().delete_oidc_provider().await? {
            tracing::info!("the provider of an organization was removed");
        }
        Ok(())
    }

    fn provider_view(&self, stored: &StoredProvider) -> OidcProvider {
        OidcProvider {
            issuer: stored.issuer.clone(),
            client_id: stored.client_id.clone(),
            has_client_secret: !stored.sealed_client_secret.is_empty(),
            auto_join: stored.auto_join,
            enabled: stored.enabled,
            redirect_uri: self.oidc_redirect_uri(),
            create_at: stored.create_at,
            update_at: stored.update_at,
        }
    }

    /// What the login page shows: the organization `org_id`, or without one the only
    /// organization of a server that holds one. Nothing of an organization is told to
    /// whoever does not name it, but on a server that holds only it.
    ///
    /// # Errors
    ///
    /// Returns the store's error.
    pub async fn login_options(&self, org_id: Option<&str>) -> Result<LoginOptions, AppError> {
        let organization = match org_id {
            Some(org_id) if fvoci_model::is_id(org_id) => {
                self.store().login_organization(Some(org_id)).await?
            }
            Some(_) => None,
            None if self.config().takes_several_organizations() => None,
            None => self.store().login_organization(None).await?,
        };
        Ok(LoginOptions { organization })
    }

    /// Starts a sign-in through the provider of the organization `org_id`: stores what
    /// the browser must bring back, and says where to send the browser.
    ///
    /// # Errors
    ///
    /// Returns [`Reason::OidcUnavailable`] if the organization has no provider that is
    /// on or it cannot be reached, and [`LoginFailure::RateLimited`] for too many
    /// sign-ins from `address`.
    pub async fn start_oidc_login(
        &self,
        org_id: &str,
        address: Option<IpAddr>,
    ) -> Result<StartedLogin, LoginFailure> {
        // Counted against the address as a login is, and let go at once: the requests
        // to the provider below are no password work, and must not keep the address's
        // other logins waiting.
        drop(
            self.login_limiter()
                .admit_registration(address, Instant::now())
                .map_err(|RetryAfter(_)| LoginFailure::RateLimited)?,
        );
        let unavailable = LoginFailure::Reason(Reason::OidcUnavailable);
        self.organization_in_use(org_id).await?;
        let org = self.enter_org_for_sign_in(org_id).ok_or(unavailable)?;
        let stored = org
            .store()
            .oidc_provider()
            .await
            .map_err(|error| logged(&error, "reading the provider"))?
            .filter(|stored| stored.enabled)
            .ok_or(unavailable)?;
        let discovery = provider::discover(self.oidc_fetcher(), &stored.issuer)
            .await
            .map_err(|error| provider_failure(&error, Reason::OidcUnavailable))?;

        let now = now_millis();
        if let Err(error) = self.store().delete_ended_oidc_login_flows(now).await {
            tracing::warn!(%error, "unable to remove the sign-ins that ended");
        }
        let (state, nonce, verifier, browser_secret) =
            (new_secret(), new_secret(), new_secret(), new_secret());
        org.store()
            .create_oidc_login_flow(&LoginFlow {
                state_hash: token_digest(&state),
                browser_hash: token_digest(&browser_secret),
                nonce: nonce.clone(),
                pkce_verifier: verifier.clone(),
                org_id: org_id.to_owned(),
                provider_id: stored.id.clone(),
                create_at: now,
                expires_at: now.saturating_add(LOGIN_FLOW_LIFETIME_MILLIS),
            })
            .await
            .map_err(|error| logged(&error, "storing a sign-in"))?;

        let mut url = reqwest::Url::parse(&discovery.authorization_endpoint).map_err(|_| {
            provider_failure(&ProviderError::EndpointNotHttps, Reason::OidcUnavailable)
        })?;
        url.query_pairs_mut()
            .append_pair("response_type", "code")
            .append_pair("client_id", &stored.client_id)
            .append_pair("redirect_uri", &self.oidc_redirect_uri())
            .append_pair("scope", SCOPES)
            .append_pair("state", &state)
            .append_pair("nonce", &nonce)
            .append_pair("code_challenge", &pkce_challenge(&verifier))
            .append_pair("code_challenge_method", "S256");
        Ok(StartedLogin {
            authorization_url: url.into(),
            browser_secret,
        })
    }

    /// Finishes a sign-in that comes back from a provider: takes the sign-in it is,
    /// once, checks that this browser started it, exchanges the code, checks the ID
    /// token, finds or makes the account, and opens a session made through the
    /// provider. Returns the organization of the sign-in with the result: the login
    /// page of a failure is that organization's. Not counted against the address: only
    /// a sign-in that was started comes back, and the start is counted.
    pub async fn finish_oidc_login(
        &self,
        callback: &Callback,
        user_agent: &str,
    ) -> (Option<String>, Result<FinishedLogin, LoginFailure>) {
        let flow = match self.take_flow(callback).await {
            Ok(flow) => flow,
            Err(failure) => return (None, Err(failure)),
        };
        let result = self.finish_flow(&flow, callback, user_agent).await;
        if let Err(failure) = &result {
            tracing::info!(org_id = %flow.org_id, failure = %failure.name(), "a sign-in through a provider failed");
        }
        (Some(flow.org_id), result)
    }

    /// The sign-in a callback is for, taken so that it is not taken again, if this
    /// browser started it and it has not ended.
    async fn take_flow(&self, callback: &Callback) -> Result<LoginFlow, LoginFailure> {
        let invalid = LoginFailure::Reason(Reason::OidcStateInvalid);
        if callback.state.is_empty() {
            return Err(invalid);
        }
        let flow = self
            .store()
            .take_oidc_login_flow(&token_digest(&callback.state), now_millis())
            .await
            .map_err(|error| logged(&error, "taking a sign-in"))?
            .ok_or(invalid)?;
        let same_browser: bool = token_digest(&callback.browser_secret)
            .as_bytes()
            .ct_eq(flow.browser_hash.as_bytes())
            .into();
        if callback.browser_secret.is_empty() || !same_browser {
            tracing::info!(org_id = %flow.org_id, "a sign-in came back to another browser than the one that started it");
            return Err(invalid);
        }
        Ok(flow)
    }

    async fn finish_flow(
        &self,
        flow: &LoginFlow,
        callback: &Callback,
        user_agent: &str,
    ) -> Result<FinishedLogin, LoginFailure> {
        if let Some(error) = &callback.error {
            // The provider's code of the error, short, for the log only.
            let error: String = error.chars().take(64).collect();
            tracing::info!(org_id = %flow.org_id, error, "the provider did not let the person sign in");
            return Err(Reason::OidcDenied.into());
        }
        if callback.code.is_empty() {
            return Err(Reason::OidcTokenRefused.into());
        }
        let unavailable = LoginFailure::Reason(Reason::OidcUnavailable);
        self.organization_in_use(&flow.org_id).await?;
        let org = self
            .enter_org_for_sign_in(&flow.org_id)
            .ok_or(unavailable)?;
        let stored = org
            .store()
            .oidc_provider()
            .await
            .map_err(|error| logged(&error, "reading the provider"))?
            .filter(|stored| stored.enabled && stored.id == flow.provider_id)
            .ok_or(unavailable)?;
        let secret = self.open_client_secret(&stored)?;
        let discovery = provider::discover(self.oidc_fetcher(), &stored.issuer)
            .await
            .map_err(|error| provider_failure(&error, Reason::OidcUnavailable))?;
        let claims = self
            .checked_claims(&discovery, &stored, secret.as_deref(), flow, &callback.code)
            .await?;
        let user = org.account_of(&claims, &stored).await?;

        let token = new_secret();
        let now = now_millis();
        let session = Session {
            id: new_id(),
            user_id: user.id.clone(),
            created_at: now,
            expires_at: now.saturating_add(self.config().session_max_millis()),
            user_agent: user_agent.to_owned(),
            login_provider_id: Some(stored.id.clone()),
            ..Session::default()
        };
        let stored_session = self
            .store()
            .create_session(&session, &token_digest(&token), &user.password_hash)
            .await
            .map_err(|error| logged(&error, "storing the session"))?;
        if !stored_session {
            // Deactivated, or given a password, while signing in.
            return Err(unavailable);
        }
        Ok(FinishedLogin { token })
    }

    /// Refuses a sign-in for an organization that is not in use, or is the system's.
    async fn organization_in_use(&self, org_id: &str) -> Result<(), LoginFailure> {
        if !fvoci_model::is_id(org_id) {
            return Err(Reason::OidcUnavailable.into());
        }
        self.store()
            .login_organization(Some(org_id))
            .await
            .map_err(|error| logged(&error, "reading the organization"))?
            .map(|_| ())
            .ok_or(Reason::OidcUnavailable.into())
    }

    /// The client secret of a provider, opened; `None` for a client without one.
    fn open_client_secret(&self, stored: &StoredProvider) -> Result<Option<String>, LoginFailure> {
        if stored.sealed_client_secret.is_empty() {
            return Ok(None);
        }
        let opened = self.config().secret_key.as_ref().map(|key| {
            key.open(
                &stored.sealed_client_secret,
                &secret_context(&stored.org_id, &stored.issuer, &stored.client_id),
            )
        });
        if let Some(Ok(secret)) = opened {
            return Ok(Some(secret));
        }
        tracing::error!(org_id = %stored.org_id, "the client secret of the provider does not open with the server's secret_key");
        Err(Reason::OidcUnavailable.into())
    }

    /// Exchanges the code and checks the ID token that comes for it.
    async fn checked_claims(
        &self,
        discovery: &Discovery,
        stored: &StoredProvider,
        secret: Option<&str>,
        flow: &LoginFlow,
        code: &str,
    ) -> Result<IdClaims, LoginFailure> {
        let redirect_uri = self.oidc_redirect_uri();
        let id_token = provider::exchange_code(
            self.oidc_fetcher(),
            discovery,
            Client {
                id: &stored.client_id,
                secret,
            },
            Grant {
                code,
                redirect_uri: &redirect_uri,
                verifier: &flow.pkce_verifier,
            },
        )
        .await
        .map_err(|error| provider_failure(&error, Reason::OidcTokenRefused))?;
        let keys = provider::keys(self.oidc_fetcher(), discovery)
            .await
            .map_err(|error| provider_failure(&error, Reason::OidcTokenRefused))?;
        provider::verify_id_token(
            &id_token,
            &keys,
            Expected {
                issuer: &stored.issuer,
                client_id: &stored.client_id,
                nonce: &flow.nonce,
            },
        )
        .map_err(|error| provider_failure(&error, Reason::OidcTokenInvalid))
    }

    /// The account the provider vouches for, a member of the handle's organization:
    /// the one of the identity, which joins the organization if it is no member and the
    /// provider lets newcomers in, or else a new account.
    async fn account_of(
        &self,
        claims: &IdClaims,
        stored: &StoredProvider,
    ) -> Result<User, LoginFailure> {
        let identity = Identity {
            issuer: &claims.issuer,
            subject: &claims.subject,
        };
        let found = self
            .store()
            .user_of_identity(identity)
            .await
            .map_err(|error| logged(&error, "looking up an identity"))?;
        if let Some(user) = found {
            self.let_in(&user, stored).await?;
            return Ok(user);
        }
        if !stored.auto_join {
            return Err(Reason::OidcJoinClosed.into());
        }
        let email = claims
            .email
            .as_deref()
            .map(str::trim)
            .filter(|email| is_email_valid(email))
            .ok_or(Reason::OidcEmailMissing)?;
        if claims.email_verified == Some(false) {
            return Err(Reason::OidcEmailUnverified.into());
        }
        match self.store().get_user_by_email(email).await {
            Ok(_) => return Err(Reason::OidcEmailTaken.into()),
            Err(error) if error.is_not_found() => {}
            Err(error) => return Err(logged(&error, "looking up an email address")),
        }
        self.make_account(claims, identity, email, stored).await
    }

    /// Makes a member of the handle's organization of an account that is no member,
    /// if the provider lets newcomers in.
    async fn let_in(&self, user: &User, stored: &StoredProvider) -> Result<(), LoginFailure> {
        let member = self
            .store()
            .is_organization_member(&user.id)
            .await
            .map_err(|error| logged(&error, "reading a membership"))?;
        if member {
            return Ok(());
        }
        if !stored.auto_join {
            return Err(Reason::OidcJoinClosed.into());
        }
        if self
            .store()
            .join_organization_through_oidc(&user.id)
            .await
            .map_err(|error| logged(&error, "letting an account into an organization"))?
        {
            tracing::info!(user_id = %user.id, org_id = %stored.org_id, "an account joined the organization by signing in through its provider");
        }
        Ok(())
    }

    /// Makes the account of a person who signs in for the first time, a member of the
    /// handle's organization and its default workspace.
    async fn make_account(
        &self,
        claims: &IdClaims,
        identity: Identity<'_>,
        email: &str,
        stored: &StoredProvider,
    ) -> Result<User, LoginFailure> {
        let base = username_base(claims, email);
        for attempt in 0..3 {
            let username = if attempt == 0 {
                base.clone()
            } else {
                format!("{base}-{}", new_secret()[..6].to_ascii_lowercase())
            };
            match self.store().get_user_by_username(&username).await {
                Ok(_) => continue,
                Err(error) if error.is_not_found() => {}
                Err(error) => return Err(logged(&error, "looking up a user name")),
            }
            let mut user = User {
                id: new_id(),
                username,
                email: email.to_owned(),
                display_name: claims
                    .name
                    .as_deref()
                    .and_then(|name| display_name(name).ok())
                    .unwrap_or_default(),
                ..User::default()
            };
            match self
                .store()
                .create_oidc_user(&mut user, identity, Join::Organization)
                .await
            {
                Ok(()) => {
                    self.metrics().registered();
                    tracing::info!(user_id = %user.id, org_id = %stored.org_id, issuer = %stored.issuer, "an account was made at its first sign-in through a provider, and joined the organization");
                    return Ok(user);
                }
                Err(StoreError::Conflict(Taken::Username)) => {}
                Err(StoreError::Conflict(Taken::Email)) => {
                    return Err(Reason::OidcEmailTaken.into());
                }
                // Another sign-in of the same person made the account a moment ago.
                Err(StoreError::Conflict(Taken::Identity)) => {
                    let user = self
                        .store()
                        .user_of_identity(identity)
                        .await
                        .map_err(|error| logged(&error, "looking up an identity"))?
                        .ok_or(Reason::OidcUnavailable)?;
                    self.let_in(&user, stored).await?;
                    return Ok(user);
                }
                Err(error) => return Err(logged(&error, "storing a new account")),
            }
        }
        tracing::warn!("no free user name was found for a new account");
        Err(Reason::OidcUnavailable.into())
    }
}

/// What a sealed client secret is bound to: the organization, the issuer and the client
/// it is for. Moved to another row, or kept for another issuer, it does not open.
pub fn secret_context(org_id: &str, issuer: &str, client_id: &str) -> Vec<u8> {
    // Each part is length-prefixed, so no two triples give the same bytes.
    let mut context = b"oidc client secret".to_vec();
    for part in [org_id, issuer, client_id] {
        context.extend_from_slice(&part.len().to_be_bytes());
        context.extend_from_slice(part.as_bytes());
    }
    context
}

/// The PKCE challenge of a verifier: its SHA-256 in unpadded base64url (S256).
fn pkce_challenge(verifier: &str) -> String {
    use base64::Engine as _;
    use sha2::{Digest as _, Sha256};
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

/// The user name of a new account, before it is seen to be free: what the provider
/// says the person goes by, or else the part of the email address before the `@`, of
/// letters, digits, `.`, `_` and `-` only.
fn username_base(claims: &IdClaims, email: &str) -> String {
    let clean = |text: &str| -> String {
        text.chars()
            .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
            .take(USERNAME_MAX_CHARS)
            .collect()
    };
    let local = email.split('@').next().unwrap_or_default();
    [
        claims.preferred_username.as_deref().unwrap_or_default(),
        local,
    ]
    .into_iter()
    .map(clean)
    .find(|name| !name.is_empty())
    .unwrap_or_else(|| "user".to_owned())
}

/// The failure of a sign-in that the server is to blame for, logged.
fn logged(error: &StoreError, doing: &str) -> LoginFailure {
    tracing::error!(%error, doing, "a sign-in through a provider failed");
    Reason::OidcUnavailable.into()
}

/// The failure of a sign-in that the provider's answer is to blame for, logged in the
/// server's words.
fn provider_failure(error: &ProviderError, reason: Reason) -> LoginFailure {
    tracing::info!(%error, "a provider's answer was not taken");
    reason.into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claims(preferred: Option<&str>) -> IdClaims {
        IdClaims {
            issuer: String::new(),
            subject: String::new(),
            email: None,
            email_verified: None,
            name: None,
            preferred_username: preferred.map(str::to_owned),
        }
    }

    #[test]
    fn a_user_name_is_made_of_what_the_provider_says_or_of_the_address() {
        assert_eq!(username_base(&claims(Some("ada.l")), "x@y.z"), "ada.l");
        assert_eq!(username_base(&claims(Some("한글")), "ada+1@y.z"), "ada1");
        assert_eq!(username_base(&claims(None), "@y.z"), "user");
    }

    #[test]
    fn the_pkce_challenge_is_the_s256_of_the_verifier() {
        // RFC 7636, appendix B.
        assert_eq!(
            pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }
}
