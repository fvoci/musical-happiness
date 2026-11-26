//! Personal API tokens as a request presents them: told from a session token by their
//! prefix before anything is looked up, made of as many random bytes as a session
//! token, and stored by their digest alone.
//!
//! A token is a credential of its own kind. The lookups of sessions refuse it before
//! they look anything up ([`refused_as_session`]), so that a route that takes a session
//! takes no token: only the routes that say they do (`Delegated` of `crate::api`, and
//! the MCP server, `TokenCaller` of `crate::api::mcp`, which takes nothing else) take
//! one, and only from the `Authorization` header.
//!
//! How many requests a token sends is counted in memory ([`TokenLimits`]): of each
//! token, and of the tokens that reach one organization together.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use fvoci_model::{API_TOKEN_PREFIX, API_TOKEN_SHOWN_CHARS, ApiTokenScope};
use fvoci_store::LiveApiToken;

use super::secret;
use crate::error::{AppError, Reason};

/// How many requests one token sends in a minute at most.
pub const TOKEN_REQUESTS_PER_MINUTE: u32 = 120;

/// How many requests the tokens that reach one organization send together in a minute
/// at most: five tokens' worth, so that one person's agents do not crowd out the rest.
pub const ORGANIZATION_TOKEN_REQUESTS_PER_MINUTE: u32 = 5 * TOKEN_REQUESTS_PER_MINUTE;

/// The window the requests are counted in.
const WINDOW: Duration = Duration::from_secs(60);

/// How many windows are kept at most; past that, those that ended are forgotten, and
/// while none has, a new one is not kept (its requests go uncounted for that minute).
const MAX_WINDOWS: usize = 100_000;

/// How seldom a token's use is written down: its list tells when it was last used to
/// within this much.
pub const USE_RECORDING_INTERVAL_MILLIS: i64 = 5 * 60 * 1000;

/// A token as it is made: the token itself, which is told once, the characters it is
/// shown by, and the digest it is stored under.
pub struct MadeToken {
    /// `fvoci_pat_` and 43 characters of the URL-safe base64 alphabet: 256 random bits.
    pub token: String,
    /// `fvoci_pat_` and the first characters after it.
    pub prefix: String,
    /// The digest it is stored and found under.
    pub digest: String,
}

impl std::fmt::Debug for MadeToken {
    /// Leaves out the token.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MadeToken")
            .field("prefix", &self.prefix)
            .finish_non_exhaustive()
    }
}

/// Makes a new token.
///
/// # Panics
///
/// Panics if the operating system cannot supply random bytes ([`secret::new_secret`]).
#[must_use]
pub fn make_token() -> MadeToken {
    let token = format!("{API_TOKEN_PREFIX}{}", secret::new_secret());
    let prefix = token[..API_TOKEN_PREFIX.len() + API_TOKEN_SHOWN_CHARS].to_owned();
    let digest = secret::token_digest(&token);
    MadeToken {
        token,
        prefix,
        digest,
    }
}

/// Whether a credential is a personal API token, by its prefix alone.
#[must_use]
pub fn is_api_token(credential: &str) -> bool {
    credential.starts_with(API_TOKEN_PREFIX)
}

/// The refusal of a personal API token where a session is looked up: a route that is
/// not open to tokens. A 403, so that a route that serves whoever has no session does
/// not take the token for nobody's.
#[must_use]
pub fn refused_as_session() -> AppError {
    AppError::forbidden("a personal API token does not open this")
        .with_reason(Reason::TokenForbiddenEndpoint)
}

/// The refusal of a token that reads only, sent to a route that writes.
#[must_use]
pub fn reads_only() -> AppError {
    AppError::forbidden("the personal API token reads only")
        .with_reason(Reason::TokenScopeInsufficient)
}

/// The refusal of a token, or of its making, in a workspace of an organization that
/// takes only sessions made through its provider and gives its workspaces to no token.
#[must_use]
pub fn disallowed_by_organization() -> AppError {
    AppError::forbidden("the organization gives its workspaces to no personal API token")
        .with_reason(Reason::ApiTokensDisallowed)
}

/// What a token gives an agent: to do as its owner may, with the content of the
/// workspaces it names, reading or writing as its scope says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Delegation {
    /// The token.
    pub token_id: String,
    /// Its name, which what it writes is told and versioned under.
    pub name: String,
    /// What it may do.
    pub scope: ApiTokenScope,
    /// Whether it reaches its owner's own space (never its diaries).
    pub allow_personal_space: bool,
    /// Whether its owner is told of what it does.
    pub notify_self: bool,
    /// The provider of the session that made it, if it was made through one.
    pub origin_provider_id: Option<String>,
}

impl From<LiveApiToken> for Delegation {
    fn from(token: LiveApiToken) -> Self {
        Self {
            token_id: token.id,
            name: token.name,
            scope: token.scope,
            allow_personal_space: token.allow_personal_space,
            notify_self: token.notify_self,
            origin_provider_id: token.origin_provider_id,
        }
    }
}

/// What the requests of tokens are counted by.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Counted {
    Token(String),
    Organization(String),
}

/// The requests of one key in the current window.
#[derive(Debug, Clone, Copy)]
struct Window {
    started: Instant,
    requests: u32,
}

/// The counts of the requests of tokens, by token and by organization.
#[derive(Debug, Default)]
pub struct TokenLimits {
    windows: Mutex<HashMap<Counted, Window>>,
}

impl TokenLimits {
    /// Counts a request of the token at `now`.
    ///
    /// # Errors
    ///
    /// Returns 429 with the reason `token_rate_limited` and how long to wait, if the
    /// token sent as many as it may this minute.
    pub fn admit_token(&self, token_id: &str, now: Instant) -> Result<(), AppError> {
        self.admit(
            Counted::Token(token_id.to_owned()),
            TOKEN_REQUESTS_PER_MINUTE,
            now,
        )
    }

    /// Counts a request of a token to a workspace of the organization at `now`.
    ///
    /// # Errors
    ///
    /// Returns 429 as [`TokenLimits::admit_token`] does, if the organization's tokens
    /// sent as many as they may this minute.
    pub fn admit_organization(&self, org_id: &str, now: Instant) -> Result<(), AppError> {
        self.admit(
            Counted::Organization(org_id.to_owned()),
            ORGANIZATION_TOKEN_REQUESTS_PER_MINUTE,
            now,
        )
    }

    fn admit(&self, key: Counted, limit: u32, now: Instant) -> Result<(), AppError> {
        let mut windows = self
            .windows
            .lock()
            .expect("the windows are never held across a panic");
        if windows.len() >= MAX_WINDOWS && !windows.contains_key(&key) {
            windows.retain(|_, window| now.saturating_duration_since(window.started) < WINDOW);
            if windows.len() >= MAX_WINDOWS {
                return Ok(());
            }
        }
        let window = windows.entry(key).or_insert(Window {
            started: now,
            requests: 0,
        });
        if now.saturating_duration_since(window.started) >= WINDOW {
            *window = Window {
                started: now,
                requests: 0,
            };
        }
        if window.requests >= limit {
            let wait = WINDOW.saturating_sub(now.saturating_duration_since(window.started));
            return Err(AppError::quota_reached(
                "too many requests of personal API tokens; try again later",
                Reason::TokenRateLimited,
                wait,
            ));
        }
        window.requests += 1;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorKind;

    #[test]
    fn a_token_is_256_random_bits_behind_its_prefix_and_shown_by_six_more() {
        let made = make_token();
        assert!(is_api_token(&made.token));
        assert_eq!(
            made.token.len(),
            API_TOKEN_PREFIX.len() + secret::SECRET_LENGTH
        );
        assert_eq!(
            made.prefix.len(),
            API_TOKEN_PREFIX.len() + API_TOKEN_SHOWN_CHARS
        );
        assert!(made.token.starts_with(&made.prefix));
        assert_eq!(made.digest, secret::token_digest(&made.token));
        assert_ne!(make_token().token, made.token, "each is new");
        assert!(
            !format!("{made:?}").contains(&made.token),
            "nor is it printed"
        );
        assert!(!is_api_token("a-session-token"));
    }

    #[test]
    fn a_token_sends_its_requests_of_a_minute_and_waits_for_the_next() {
        let limits = TokenLimits::default();
        let start = Instant::now();
        for _ in 0..TOKEN_REQUESTS_PER_MINUTE {
            limits.admit_token("t", start).expect("admitted");
        }
        let refused = limits
            .admit_token("t", start + Duration::from_secs(20))
            .expect_err("one too many");
        assert_eq!(refused.kind(), ErrorKind::RateLimited);
        assert_eq!(refused.reason(), Some(Reason::TokenRateLimited));
        limits
            .admit_token("other", start)
            .expect("another token counts for itself");
        limits
            .admit_token("t", start + WINDOW)
            .expect("the next minute");
    }

    #[test]
    fn the_tokens_of_an_organization_send_five_tokens_worth_together() {
        let limits = TokenLimits::default();
        let now = Instant::now();
        for _ in 0..ORGANIZATION_TOKEN_REQUESTS_PER_MINUTE {
            limits.admit_organization("o", now).expect("admitted");
        }
        assert!(limits.admit_organization("o", now).is_err());
        assert!(limits.admit_organization("p", now).is_ok());
    }
}
