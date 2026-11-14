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

use fvoci_model::{LoginOptions, Session, User, display_name, is_email_valid, new_id};
use fvoci_store::{
    Applied, FlowPurpose, Identity, LoginFlow, OidcAccount, OidcRefusal, PendingProvider,
    StoreError, StoredProvider, Taken, Trial,
};

use super::App;
use super::oidc_settings::secret_context;
use crate::auth::limiter::RetryAfter;
use crate::auth::now_millis;
use crate::auth::oidc::provider::{
    self, Client, Discovery, Expected, Grant, IdClaims, ProviderError,
};
use crate::auth::secret::{new_secret, token_digest};
use crate::error::{AppError, Reason};

/// The path, under the server's address, below which a provider sends a browser back:
/// each organization has an address of its own there, `…/callback/{orgId}`.
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
}

impl LoginFailure {
    /// The name the login page gets in its `oidc_error` parameter.
    pub fn name(self) -> String {
        match self {
            Self::Reason(reason) => reason.name(),
        }
    }
}

impl From<Reason> for LoginFailure {
    fn from(reason: Reason) -> Self {
        Self::Reason(reason)
    }
}

/// What came of a sign-in that came back from a provider, and where the browser goes
/// to be told.
#[derive(Debug)]
pub struct Returned {
    /// The organization of the sign-in, where there is one.
    pub org_id: Option<String>,
    /// The page that tells a failure.
    pub back_to: BackTo,
    /// How it ended.
    pub result: Result<FinishedLogin, LoginFailure>,
}

/// The page a sign-in that came back goes on to with a failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackTo {
    /// The login page of the organization.
    LoginPage,
    /// The organization's page: a trial of its pending settings was started there by
    /// an administrator who is signed in.
    Organization,
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
    /// The organization the address it came back to names: each organization has an
    /// address of its own (`…/callback/{orgId}`).
    pub org_id: String,
    /// The `iss` parameter (RFC 9207): the issuer that answered, if it says.
    pub issuer: Option<String>,
    /// The token of the session cookie the browser came back with, if it has one: a
    /// sign-in that links ends only for the session it was started from.
    pub session_token: Option<String>,
}

impl std::fmt::Debug for Callback {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Callback")
            .field("error", &self.error)
            .finish_non_exhaustive()
    }
}

/// How a sign-in through a provider ended well.
#[derive(Clone)]
pub enum FinishedLogin {
    /// In a session.
    SignedIn {
        /// The token of the new session.
        token: String,
    },
    /// In an identity tied to the account of the session it was started from: no
    /// session is made, and the one there goes on.
    Linked,
    /// In a session made through the pending settings, which are the provider's now.
    Applied {
        /// The token of the new session.
        token: String,
    },
}

impl std::fmt::Debug for FinishedLogin {
    /// Leaves out the token.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::SignedIn { .. } => "SignedIn",
            Self::Linked => "Linked",
            Self::Applied { .. } => "Applied",
        })
    }
}

impl App {
    /// The address the provider of the organization `org_id` sends a browser back to:
    /// from the server's own address (`serverRoot`), never from what a request says its
    /// host is. Each organization has its own, so that what one organization's provider
    /// sends back is never taken for another's (a mix-up of providers); it is known
    /// before the provider is set, and stays when the provider is changed.
    pub fn oidc_redirect_uri(&self, org_id: &str) -> String {
        format!(
            "{}{CALLBACK_PATH}/{org_id}",
            self.config().server_root.trim_end_matches('/')
        )
    }

    /// What the login page shows: the organization `org_id`, or without one the only
    /// organization of a server that holds one, and whether a registration is taken
    /// without an invitation. Nothing of an organization is told to whoever does not
    /// name it, but on a server that holds only it.
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
        Ok(LoginOptions {
            organization,
            registration_open: self.takes_registrations().await?,
            mail_enabled: self.config().mail_enabled(),
        })
    }

    /// Starts a sign-in through the provider of the organization `org_id`: stores what
    /// the browser must bring back, and says where to send the browser.
    ///
    /// # Errors
    ///
    /// Returns 409 with [`Reason::OidcUnavailable`] if the organization has no provider
    /// that is on or it cannot be reached, and 429 for too many sign-ins from `address`.
    pub async fn start_oidc_login(
        &self,
        org_id: &str,
        address: Option<IpAddr>,
    ) -> Result<StartedLogin, AppError> {
        self.start_oidc_flow(org_id, address, Start::SignIn).await
    }

    /// Starts a sign-in as [`Self::start_oidc_login`] does: to sign in, to link
    /// (`App::start_oidc_link`), or to try the pending settings
    /// (`App::start_oidc_trial`), through them.
    pub(super) async fn start_oidc_flow(
        &self,
        org_id: &str,
        address: Option<IpAddr>,
        start: Start<'_>,
    ) -> Result<StartedLogin, AppError> {
        // Counted against the address as a login is, and let go at once: the requests
        // to the provider below are no password work, and must not keep the address's
        // other logins waiting.
        drop(
            self.login_limiter()
                .admit_registration(address, Instant::now())
                .map_err(|RetryAfter(wait)| AppError::rate_limited(wait))?,
        );
        // What is in the way is the provider's (409); the store's failures are the
        // server's own (500).
        let unavailable = || {
            AppError::conflict("no sign-in through the organization's provider can start now")
                .with_reason(Reason::OidcUnavailable)
        };
        if !self
            .is_organization_in_use(org_id)
            .await
            .map_err(|error| AppError::from(error).context("reading the organization"))?
        {
            return Err(unavailable());
        }
        let org = self.enter_org_for_sign_in(org_id).ok_or_else(unavailable)?;
        let (stored, purpose) = org
            .provider_to_start(start)
            .await
            .map_err(|error| AppError::from(error).context("reading the provider"))?
            .ok_or_else(unavailable)?;
        let discovery = provider::discover(self.oidc_fetcher(), &stored.issuer)
            .await
            .map_err(|error| {
                provider_failure(&error, Reason::OidcUnavailable);
                unavailable()
            })?;

        let (state, nonce, verifier, browser_secret) =
            (new_secret(), new_secret(), new_secret(), new_secret());
        // The address first: a provider whose address is none starts nothing.
        let mut url = reqwest::Url::parse(&discovery.authorization_endpoint).map_err(|_| {
            tracing::warn!(
                org_id,
                "the authorization endpoint of a provider is no address"
            );
            unavailable()
        })?;
        url.query_pairs_mut()
            .append_pair("response_type", "code")
            .append_pair("client_id", &stored.client_id)
            .append_pair("redirect_uri", &self.oidc_redirect_uri(org_id))
            .append_pair("scope", SCOPES)
            .append_pair("state", &state)
            .append_pair("nonce", &nonce)
            .append_pair("code_challenge", &pkce_challenge(&verifier))
            .append_pair("code_challenge_method", "S256");

        let now = now_millis();
        if let Err(error) = self.store().delete_ended_oidc_login_flows(now).await {
            tracing::warn!(%error, "unable to remove the sign-ins that ended");
        }
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
                purpose,
            })
            .await
            .map_err(|error| AppError::from(error).context("storing a sign-in"))?;
        Ok(StartedLogin {
            authorization_url: url.into(),
            browser_secret,
        })
    }

    /// What a sign-in that starts as `start` goes through, on the handle of its
    /// organization: the provider, if it is on; for a trial, with its pending settings,
    /// if there are. `None` if there is nothing to go through.
    async fn provider_to_start(
        &self,
        start: Start<'_>,
    ) -> Result<Option<(StoredProvider, FlowPurpose)>, StoreError> {
        let Some(stored) = self
            .store()
            .oidc_provider()
            .await?
            .filter(|stored| stored.enabled)
        else {
            return Ok(None);
        };
        Ok(match start {
            Start::SignIn => Some((stored, FlowPurpose::SignIn)),
            Start::Link { session_id } => {
                let session_id = session_id.to_owned();
                Some((stored, FlowPurpose::Link { session_id }))
            }
            Start::Trial { user_id } => {
                self.store()
                    .oidc_pending_provider()
                    .await?
                    .and_then(|pending| {
                        let purpose = FlowPurpose::Trial {
                            pending_at: pending.create_at,
                            user_id: user_id.to_owned(),
                        };
                        trying(&stored, &pending).map(|trying| (trying, purpose))
                    })
            }
        })
    }

    /// Finishes a sign-in that comes back from a provider: takes the sign-in it is,
    /// once, checks that this browser started it, exchanges the code, checks the ID
    /// token, finds or makes the account, and opens a session made through the
    /// provider. Returns the organization of the sign-in with the result: the login
    /// page of a failure is that organization's. Not counted against the address: only
    /// a sign-in that was started comes back, and the start is counted.
    pub async fn finish_oidc_login(&self, callback: &Callback, user_agent: &str) -> Returned {
        let flow = match self.take_flow(callback).await {
            Ok(flow) => flow,
            Err((org_id, failure)) => {
                return Returned {
                    org_id,
                    back_to: BackTo::LoginPage,
                    result: Err(failure),
                };
            }
        };
        let result = self.finish_flow(&flow, callback, user_agent).await;
        if let Err(failure) = &result {
            tracing::info!(org_id = %flow.org_id, failure = %failure.name(), "a sign-in through a provider failed");
        }
        Returned {
            back_to: match flow.purpose {
                FlowPurpose::Trial { .. } => BackTo::Organization,
                FlowPurpose::SignIn | FlowPurpose::Link { .. } => BackTo::LoginPage,
            },
            org_id: Some(flow.org_id),
            result,
        }
    }

    /// The sign-in a callback is for, taken so that it is not taken again, if this
    /// browser started it and it has not ended. Another browser takes nothing: one who
    /// learned the `state` cannot end the sign-in of the browser that started it. A
    /// refusal names the organization of the sign-in, where there is one, for the login
    /// page to go back to.
    async fn take_flow(
        &self,
        callback: &Callback,
    ) -> Result<LoginFlow, (Option<String>, LoginFailure)> {
        let invalid = LoginFailure::Reason(Reason::OidcStateInvalid);
        if callback.state.is_empty() || callback.browser_secret.is_empty() {
            return Err((None, invalid));
        }
        let state_hash = token_digest(&callback.state);
        let taken = self
            .store()
            .take_oidc_login_flow(&state_hash, &token_digest(&callback.browser_secret))
            .await
            .map_err(|error| (None, logged(&error, "taking a sign-in")))?;
        if let Some(flow) = taken {
            if flow.expires_at <= now_millis() {
                return Err((Some(flow.org_id), invalid));
            }
            return Ok(flow);
        }
        let org_id = self
            .store()
            .oidc_login_flow_org(&state_hash)
            .await
            .map_err(|error| (None, logged(&error, "reading a sign-in")))?;
        if let Some(org_id) = &org_id {
            tracing::info!(org_id = %org_id, "a sign-in came back to another browser than the one that started it");
        }
        Err((org_id, invalid))
    }

    async fn finish_flow(
        &self,
        flow: &LoginFlow,
        callback: &Callback,
        user_agent: &str,
    ) -> Result<FinishedLogin, LoginFailure> {
        // Came back to the address of another organization than the one it was started
        // for: an answer of one organization's provider handed to another's. Nothing is
        // exchanged.
        if callback.org_id != flow.org_id {
            tracing::info!(org_id = %flow.org_id, "a sign-in came back to the address of another organization");
            return Err(Reason::OidcStateInvalid.into());
        }
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
        if !self
            .is_organization_in_use(&flow.org_id)
            .await
            .map_err(|error| logged(&error, "reading the organization"))?
        {
            return Err(unavailable);
        }
        let org = self
            .enter_org_for_sign_in(&flow.org_id)
            .ok_or(unavailable)?;
        let stored = org.provider_of_flow(flow).await?.ok_or(unavailable)?;
        let secret = self.open_client_secret(&stored)?;
        let discovery = provider::discover(self.oidc_fetcher(), &stored.issuer)
            .await
            .map_err(|error| provider_failure(&error, Reason::OidcUnavailable))?;
        // RFC 9207: the issuer that answered, where it says, is the one asked; a
        // provider that says it always says must have.
        let answered_by_it = match &callback.issuer {
            Some(issuer) => *issuer == stored.issuer,
            None => !discovery.authorization_response_iss_parameter_supported,
        };
        if !answered_by_it {
            tracing::info!(org_id = %flow.org_id, "a sign-in came back with another issuer than its provider's");
            return Err(Reason::OidcStateInvalid.into());
        }
        // A link that comes back without a session ends here, before the provider is
        // asked for anything: which session it is, is asked where the link is stored.
        if matches!(flow.purpose, FlowPurpose::Link { .. }) && callback.session_token.is_none() {
            tracing::info!(org_id = %flow.org_id, "a sign-in to link came back without a session");
            return Err(Reason::OidcStateInvalid.into());
        }
        let claims = self
            .checked_claims(&discovery, &stored, secret.as_deref(), flow, &callback.code)
            .await?;
        let ending = match &flow.purpose {
            FlowPurpose::Link { session_id } => {
                org.link_through(&claims, &stored, session_id, callback)
                    .await?;
                return Ok(FinishedLogin::Linked);
            }
            FlowPurpose::SignIn => Ending::SignIn,
            FlowPurpose::Trial {
                pending_at,
                user_id,
            } => Ending::Trial {
                pending_at: *pending_at,
                trier_id: user_id,
            },
        };
        let token = new_secret();
        let now = now_millis();
        let mut session = Session {
            id: new_id(),
            created_at: now,
            expires_at: now.saturating_add(self.config().session_max_millis()),
            user_agent: user_agent.to_owned(),
            ..Session::default()
        };
        // Boxed: making the account and applying the pending settings make a large
        // future, which every caller of a callback would otherwise carry.
        let applied = Box::pin(org.sign_in_through(
            &claims,
            &stored,
            ending,
            &mut session,
            &token_digest(&token),
        ))
        .await?;
        Ok(match applied {
            Some(applied) => {
                org.follow_applied(&applied).await;
                FinishedLogin::Applied { token }
            }
            None => FinishedLogin::SignedIn { token },
        })
    }

    /// The provider a sign-in that came back goes through, on the handle of its
    /// organization: the one it was started through, still on; for a trial, with the
    /// pending settings it was started for, and no other. `None` if that is gone.
    async fn provider_of_flow(
        &self,
        flow: &LoginFlow,
    ) -> Result<Option<StoredProvider>, LoginFailure> {
        let Some(stored) = self
            .store()
            .oidc_provider()
            .await
            .map_err(|error| logged(&error, "reading the provider"))?
            .filter(|stored| stored.enabled && stored.id == flow.provider_id)
        else {
            return Ok(None);
        };
        let FlowPurpose::Trial { pending_at, .. } = &flow.purpose else {
            return Ok(Some(stored));
        };
        Ok(self
            .store()
            .oidc_pending_provider()
            .await
            .map_err(|error| logged(&error, "reading the provider"))?
            .filter(|pending| pending.create_at == *pending_at)
            .and_then(|pending| trying(&stored, &pending)))
    }

    /// Whether `org_id` names an organization in use that is not the system's: none
    /// other is signed in to.
    async fn is_organization_in_use(&self, org_id: &str) -> Result<bool, StoreError> {
        if !fvoci_model::is_id(org_id) {
            return Ok(false);
        }
        Ok(self
            .store()
            .login_organization(Some(org_id))
            .await?
            .is_some())
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
        let redirect_uri = self.oidc_redirect_uri(&stored.org_id);
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

    /// Signs the person the provider vouches for in, a member of the handle's
    /// organization: to the account of the identity, which joins the organization if it
    /// is no member and the provider lets newcomers in, or else to a new account; and
    /// stores `session` for it under `digest`. What decides is read again when that is
    /// stored (`Store::finish_oidc_sign_in`): the provider may have changed while the
    /// person was at it.
    async fn sign_in_through(
        &self,
        claims: &IdClaims,
        stored: &StoredProvider,
        ending: Ending<'_>,
        session: &mut Session,
        digest: &str,
    ) -> Result<Option<Applied>, LoginFailure> {
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
            return self
                .let_in(&user, identity, stored, ending, session, digest)
                .await;
        }
        // A trial ends in the account of the administrator who started it, which the
        // identity is not: no account is made for it, and no identity tied. (Tying one
        // by a trial would let whoever saved the issuer, or holds the session, tie an
        // identity of their own to the trier's account.)
        if matches!(ending, Ending::Trial { .. }) {
            tracing::info!(org_id = %stored.org_id, "a trial of pending settings signed in somebody without an account");
            return Err(Reason::OidcTrialAccount.into());
        }
        // The identity of an account that is deleted: not let in, and not made anew.
        if self
            .store()
            .is_identity_known(identity)
            .await
            .map_err(|error| logged(&error, "looking up an identity"))?
        {
            tracing::info!(issuer = %claims.issuer, "a sign-in through a provider was for a deleted account");
            return Err(Reason::OidcUnavailable.into());
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
        // An account whose own address this is, one with a password: most likely the
        // same person, who is to sign in to it and link the provider to it, not to have
        // two accounts. What a provider gave for another account is no account's
        // address, and stands in nobody's way.
        match self.store().get_user_by_email(email).await {
            Ok(_) => return Err(Reason::OidcEmailTaken.into()),
            Err(error) if error.is_not_found() => {}
            Err(error) => return Err(logged(&error, "looking up an email address")),
        }
        self.make_account(claims, identity, email, stored, ending, session, digest)
            .await
    }

    /// Signs in to the account of the identity, which joins the handle's organization
    /// if it is no member and the provider lets newcomers in.
    async fn let_in(
        &self,
        user: &User,
        identity: Identity<'_>,
        stored: &StoredProvider,
        ending: Ending<'_>,
        session: &mut Session,
        digest: &str,
    ) -> Result<Option<Applied>, LoginFailure> {
        let account = OidcAccount::Known {
            user_id: &user.id,
            password_hash: &user.password_hash,
            identity,
        };
        let (joined, applied) = self
            .finish_in_store(stored, ending, account, session, digest)
            .await
            .map_err(|error| logged(&error, "storing a sign-in"))?
            .map_err(|refusal| refused(refusal, stored))?;
        if joined {
            tracing::info!(user_id = %user.id, org_id = %stored.org_id, "an account joined the organization by signing in through its provider");
        }
        Ok(applied)
    }

    /// Stores the end of a sign-in as `ending` says: a session through the provider as
    /// it is, or through the pending settings, which become the provider's.
    async fn finish_in_store(
        &self,
        stored: &StoredProvider,
        ending: Ending<'_>,
        account: OidcAccount<'_>,
        session: &mut Session,
        digest: &str,
    ) -> Result<Result<(bool, Option<Applied>), OidcRefusal>, StoreError> {
        Ok(match ending {
            Ending::SignIn => self
                .store()
                .finish_oidc_sign_in(&stored.id, stored.revision, account, session, digest)
                .await?
                .map(|joined| (joined, None)),
            Ending::Trial {
                pending_at,
                trier_id,
            } => self
                .store()
                .finish_oidc_trial(
                    Trial {
                        provider_id: &stored.id,
                        pending_at,
                        trier_id,
                        keep_former: self.limits().former_provider_kept,
                    },
                    account,
                    session,
                    digest,
                )
                .await?
                .map(|applied| (applied.joined, Some(applied))),
        })
    }

    /// Makes the account of a person who signs in for the first time, a member of the
    /// handle's organization and its default workspace, and signs in to it.
    #[expect(
        clippy::too_many_arguments,
        reason = "the sign-in's parts, each of its own kind, passed down once"
    )]
    async fn make_account(
        &self,
        claims: &IdClaims,
        identity: Identity<'_>,
        email: &str,
        stored: &StoredProvider,
        ending: Ending<'_>,
        session: &mut Session,
        digest: &str,
    ) -> Result<Option<Applied>, LoginFailure> {
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
            // No address of its own: the provider's is kept with the identity.
            let mut user = User {
                id: new_id(),
                username,
                display_name: claims
                    .name
                    .as_deref()
                    .and_then(|name| display_name(name).ok())
                    .unwrap_or_default(),
                ..User::default()
            };
            let account = OidcAccount::New {
                user: &mut user,
                identity,
                provider_email: email,
            };
            match self
                .finish_in_store(stored, ending, account, session, digest)
                .await
            {
                Ok(Ok((_, applied))) => {
                    self.metrics().registered();
                    tracing::info!(user_id = %user.id, org_id = %stored.org_id, issuer = %stored.issuer, "an account was made at its first sign-in through a provider, and joined the organization");
                    return Ok(applied);
                }
                Ok(Err(refusal)) => return Err(refused(refusal, stored)),
                Err(StoreError::Conflict(Taken::Username)) => {}
                // Another sign-in of the same person made the account a moment ago.
                Err(StoreError::Conflict(Taken::Identity)) => {
                    let user = self
                        .store()
                        .user_of_identity(identity)
                        .await
                        .map_err(|error| logged(&error, "looking up an identity"))?
                        .ok_or(Reason::OidcUnavailable)?;
                    return self
                        .let_in(&user, identity, stored, ending, session, digest)
                        .await;
                }
                Err(error) => return Err(logged(&error, "storing a new account")),
            }
        }
        tracing::warn!("no free user name was found for a new account");
        Err(Reason::OidcUnavailable.into())
    }
}

/// What a sign-in is started for, as [`App::start_oidc_flow`] is asked.
#[derive(Debug, Clone, Copy)]
pub(super) enum Start<'a> {
    /// To sign in through the provider as it is.
    SignIn,
    /// To link, from the session.
    Link {
        /// The session it is started from.
        session_id: &'a str,
    },
    /// To try the pending settings, by the administrator `user_id`.
    Trial {
        /// The administrator whose account alone the trial may end in.
        user_id: &'a str,
    },
}

/// How a sign-in that ends in a session is stored.
#[derive(Debug, Clone, Copy)]
enum Ending<'a> {
    /// Through the provider as it is.
    SignIn,
    /// Through the pending settings saved at `pending_at`, which become the provider's,
    /// in the account of the administrator `trier_id` only.
    Trial {
        /// When they were saved.
        pending_at: i64,
        /// The administrator who started it.
        trier_id: &'a str,
    },
}

/// The provider as a trial signs in through it: with the pending settings' issuer,
/// client and secret, if they are of this very provider.
fn trying(stored: &StoredProvider, pending: &PendingProvider) -> Option<StoredProvider> {
    (pending.provider_id == stored.id).then(|| pending.applied_to(stored))
}

/// The failure of a sign-in that the store refused at its end, logged.
fn refused(refusal: OidcRefusal, stored: &StoredProvider) -> LoginFailure {
    match refusal {
        OidcRefusal::ProviderGone => {
            tracing::info!(org_id = %stored.org_id, "the provider was switched off or changed while a sign-in through it was under way");
            Reason::OidcUnavailable.into()
        }
        OidcRefusal::JoinClosed => Reason::OidcJoinClosed.into(),
        OidcRefusal::NotTheTrier => {
            tracing::info!(org_id = %stored.org_id, "a trial of pending settings ended in another account than its administrator's");
            Reason::OidcTrialAccount.into()
        }
        OidcRefusal::AccountChanged => {
            tracing::info!(org_id = %stored.org_id, "an account was deleted, given a password or unlinked from the identity while signing in through a provider");
            Reason::OidcUnavailable.into()
        }
    }
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
