//! Authentication: session tokens, passwords, and read tokens of shared boards.

pub mod password;
pub mod secret;
pub mod token;

use fvoci_model::Session;
use fvoci_store::Store;
use subtle::ConstantTimeEq as _;

use crate::config::Configuration;
use crate::error::AppError;

/// How long after the last recorded use of a session its next use is recorded: ten
/// minutes. Requests in between do not write to the database, and the idle limit is
/// that much less exact.
const SESSION_USE_RECORDING_INTERVAL_MILLIS: i64 = 10 * 60 * 1000;

/// Looks up sessions and checks read tokens.
#[derive(Debug, Clone)]
pub struct Authenticator {
    store: Store,
    session_idle_millis: i64,
    public_shared_boards: bool,
}

impl Authenticator {
    /// Creates an authenticator with the session lifetime and the sharing switch of the
    /// configuration.
    pub fn new(store: Store, config: &Configuration) -> Self {
        Self {
            store,
            session_idle_millis: config.session_idle_millis(),
            public_shared_boards: config.enable_public_shared_boards,
        }
    }

    /// Returns the session a token belongs to, and records that it was used unless that
    /// was recorded in the last ten minutes.
    ///
    /// The session is looked up by the digest of the token; the token itself is stored
    /// nowhere. A session has ended, and is not found, when it has not been used for the
    /// idle limit, when its absolute limit has passed, and when its user is deactivated.
    ///
    /// # Errors
    ///
    /// Returns an unauthorized error if the token is empty, unknown or of a session that
    /// has ended, and a server error if the session cannot be looked up.
    pub async fn get_session(&self, token: &str) -> Result<Session, AppError> {
        if token.is_empty() {
            return Err(AppError::unauthorized("no session token"));
        }
        let now = now_millis();
        let mut session = match self
            .store
            .get_session(
                &secret::token_digest(token),
                now,
                now.saturating_sub(self.session_idle_millis),
            )
            .await
        {
            Ok(session) => session,
            // An unknown or expired token is an everyday refusal; a store that fails is
            // the server's fault and is reported as such.
            Err(error) if error.is_not_found() => {
                return Err(AppError::unauthorized("invalid session token"));
            }
            Err(error) => {
                return Err(AppError::from(error).context("looking up the session"));
            }
        };

        if now.saturating_sub(session.last_used_at) >= SESSION_USE_RECORDING_INTERVAL_MILLIS {
            // A session that cannot be marked as used is still the caller's session.
            match self.store.touch_session(&session.id, now).await {
                Ok(()) => session.last_used_at = now,
                Err(error) => tracing::warn!(%error, "unable to record the use of the session"),
            }
        }
        Ok(session)
    }

    /// Returns whether `read_token` grants read access to a shared board.
    ///
    /// # Errors
    ///
    /// Returns an error if the board has a sharing record but public sharing is turned
    /// off in the configuration, or if the lookup fails.
    pub async fn is_valid_read_token(
        &self,
        board_id: &str,
        read_token: &str,
    ) -> Result<bool, AppError> {
        let sharing = match self.store.get_sharing(board_id).await {
            Ok(sharing) => sharing,
            Err(error) if error.is_not_found() => return Ok(false),
            Err(error) => return Err(error.into()),
        };
        if !self.public_shared_boards {
            return Err(AppError::internal("public shared boards disabled"));
        }
        // A board that is not shared has no token; nothing matches that, not even
        // nothing.
        Ok(
            sharing.enabled
                && !sharing.token.is_empty()
                && tokens_match(&sharing.token, read_token),
        )
    }
}

/// Compares two secrets without revealing through timing where they differ.
pub fn tokens_match(expected: &str, given: &str) -> bool {
    expected.as_bytes().ct_eq(given.as_bytes()).into()
}

/// Returns the current time in milliseconds since the Unix epoch.
pub fn now_millis() -> i64 {
    let since_epoch = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    i64::try_from(since_epoch.as_millis()).unwrap_or(i64::MAX)
}
