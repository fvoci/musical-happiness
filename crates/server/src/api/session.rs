//! Extractors that resolve the session of a request.

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use fvoci_model::Session;

use super::request_token;
use crate::app::App;
use crate::error::{AppError, ErrorKind};

/// The session of a request to a route that needs one. Extraction fails with 401 if the
/// request carries no valid session token.
#[derive(Debug)]
pub(crate) struct RequiredSession(pub(crate) Session);

/// The session of a request to a route that also serves anonymous readers of shared
/// boards. `None` if the request carries no valid session token; the handler then checks
/// the read token.
#[derive(Debug)]
pub(crate) struct OptionalSession(pub(crate) Option<Session>);

impl OptionalSession {
    /// The identifier of the session's user, or the empty string without a session.
    pub(crate) fn user_id(&self) -> &str {
        self.0
            .as_ref()
            .map_or("", |session| session.user_id.as_str())
    }
}

impl FromRequestParts<App> for RequiredSession {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, app: &App) -> Result<Self, Self::Rejection> {
        let token = request_token(&parts.headers, parts.uri.query());
        session_of(app, &token).await.map(Self)
    }
}

/// The session a token stands for.
///
/// # Errors
///
/// Returns a 401 if the token is no session's, and the store's error if the session
/// cannot be looked up.
pub(crate) async fn session_of(app: &App, token: &str) -> Result<Session, AppError> {
    app.authenticator().get_session(token).await
}

impl FromRequestParts<App> for OptionalSession {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, app: &App) -> Result<Self, Self::Rejection> {
        let token = request_token(&parts.headers, parts.uri.query());
        match app.authenticator().get_session(&token).await {
            Ok(session) => Ok(Self(Some(session))),
            // No session is an answer these routes can work with (a read token may do).
            // A store that failed is not "no session": answered as one, it would make
            // clients throw a valid session away.
            Err(error) if error.kind() == ErrorKind::Unauthorized => Ok(Self(None)),
            Err(error) => Err(error),
        }
    }
}
