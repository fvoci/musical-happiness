//! Extractors that resolve the session of a request.

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use fvoci_model::{JsonMap, SINGLE_USER_ID, Session};

use super::request_token;
use crate::app::App;
use crate::auth::{now_millis, tokens_match};
use crate::error::AppError;

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

/// The session every request has in single-user mode.
fn single_user_session(token: String, auth_service: &str) -> Session {
    let now = now_millis();
    Session {
        id: SINGLE_USER_ID.to_owned(),
        token,
        user_id: SINGLE_USER_ID.to_owned(),
        auth_service: auth_service.to_owned(),
        props: JsonMap::new(),
        create_at: now,
        update_at: now,
    }
}

impl FromRequestParts<App> for RequiredSession {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, app: &App) -> Result<Self, Self::Rejection> {
        let token = request_token(&parts.headers, parts.uri.query());
        if let Some(single_user_token) = app.single_user_token() {
            if !tokens_match(single_user_token, &token) {
                return Err(AppError::unauthorized("invalid single user token"));
            }
            return Ok(Self(single_user_session(token, &app.config().auth_mode)));
        }

        let session = app.authenticator().get_session(&token).await?;
        check_auth_service(&session, app)?;
        Ok(Self(session))
    }
}

impl FromRequestParts<App> for OptionalSession {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, app: &App) -> Result<Self, Self::Rejection> {
        let token = request_token(&parts.headers, parts.uri.query());
        if let Some(single_user_token) = app.single_user_token() {
            // The original hands out the single-user session here whatever the token is,
            // which lets anyone who can reach the port read boards and files. Only the
            // right token gets the session.
            let session = tokens_match(single_user_token, &token)
                .then(|| single_user_session(token, &app.config().auth_mode));
            return Ok(Self(session));
        }

        match app.authenticator().get_session(&token).await {
            Ok(session) => {
                check_auth_service(&session, app)?;
                Ok(Self(Some(session)))
            }
            Err(_) => Ok(Self(None)),
        }
    }
}

/// A session belongs to the authentication mode that created it.
fn check_auth_service(session: &Session, app: &App) -> Result<(), AppError> {
    if session.auth_service == app.config().auth_mode {
        return Ok(());
    }
    tracing::error!(
        session_id = %session.id,
        want = %app.config().auth_mode,
        got = %session.auth_service,
        "Session authService mismatch"
    );
    // The log says why; the client learns only that the session does not count.
    Err(AppError::unauthorized("invalid session token"))
}
