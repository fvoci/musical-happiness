//! Authentication: session tokens, passwords, and read tokens of shared boards.

pub mod password;
pub mod token;

use fvoci_model::Session;
use fvoci_store::Store;
use subtle::ConstantTimeEq as _;

use crate::config::Configuration;
use crate::error::AppError;

/// Looks up sessions and checks read tokens.
#[derive(Debug, Clone)]
pub struct Authenticator {
    store: Store,
    session_expire_seconds: i64,
    session_refresh_seconds: i64,
    public_shared_boards: bool,
}

impl Authenticator {
    /// Creates an authenticator with the session lifetimes and the sharing switch of the
    /// configuration.
    pub fn new(store: Store, config: &Configuration) -> Self {
        Self {
            store,
            session_expire_seconds: config.session_expire_time,
            session_refresh_seconds: config.session_refresh_time,
            public_shared_boards: config.enable_public_shared_boards,
        }
    }

    /// Returns the session a token belongs to, refreshing it if it is due.
    ///
    /// # Errors
    ///
    /// Returns an unauthorized error if the token is empty, unknown or expired, and a
    /// server error if the session cannot be looked up.
    pub async fn get_session(&self, token: &str) -> Result<Session, AppError> {
        if token.is_empty() {
            return Err(AppError::unauthorized("no session token"));
        }
        let session = match self
            .store
            .get_session(token, self.session_expire_seconds)
            .await
        {
            Ok(session) => session,
            // An unknown or expired token is an everyday refusal; a store that fails is
            // the server's fault and is reported as such.
            Err(error) if error.is_not_found() => {
                return Err(AppError::unauthorized("invalid session token"));
            }
            Err(error) => {
                tracing::error!(%error, "the session lookup failed");
                return Err(AppError::from(error).context("looking up the session"));
            }
        };

        // Preserved quirk: the store does not read a session's `update_at`, so it is zero
        // here and the session is refreshed on every request.
        let refresh_before = now_millis() - self.session_refresh_seconds.saturating_mul(1000);
        if session.update_at < refresh_before
            && let Err(error) = self.store.refresh_session(&session).await
        {
            tracing::warn!(%error, "unable to refresh the session");
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
        Ok(sharing.id == board_id && sharing.enabled && tokens_match(&sharing.token, read_token))
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
