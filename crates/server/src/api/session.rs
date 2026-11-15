//! Extractors that resolve the session of a request, and the renewal of the session
//! cookie.

use std::sync::{Arc, Mutex};

use axum::extract::{FromRequestParts, Request, State};
use axum::http::header::SET_COOKIE;
use axum::http::request::Parts;
use axum::http::{Extensions, HeaderMap};
use axum::middleware::Next;
use axum::response::Response;
use fvoci_model::Session;

use crate::app::App;
use crate::auth::token::Credential;
use crate::error::{AppError, ErrorKind};

/// The session of a request to a route that needs one. Extraction fails with 401 if the
/// request carries no valid session token.
#[derive(Debug)]
pub(crate) struct RequiredSession(pub(crate) Session);

/// The session of a request to a route that also serves whoever has none: the
/// acceptance of an invitation, which makes an account for them. `None` if the request
/// carries no valid session token.
#[derive(Debug)]
pub(crate) struct OptionalSession(pub(crate) Option<Session>);

impl FromRequestParts<App> for OptionalSession {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, app: &App) -> Result<Self, Self::Rejection> {
        optional_session_of(app, &parts.headers, &parts.extensions)
            .await
            .map(Self)
    }
}

/// The session of a request, as [`session_of`] finds it, or `None` if the request
/// carries no valid session token.
///
/// # Errors
///
/// Returns the store's error if the session cannot be looked up.
pub(crate) async fn optional_session_of(
    app: &App,
    headers: &HeaderMap,
    extensions: &Extensions,
) -> Result<Option<Session>, AppError> {
    match session_of(app, headers, extensions).await {
        Ok(session) => Ok(Some(session)),
        // No session is an answer the route can work with. A store that failed is not
        // "no session": answered as one, it would make clients throw a valid session
        // away.
        Err(error) if error.kind() == ErrorKind::Unauthorized => Ok(None),
        Err(error) => Err(error),
    }
}

impl FromRequestParts<App> for RequiredSession {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, app: &App) -> Result<Self, Self::Rejection> {
        session_of(app, &parts.headers, &parts.extensions)
            .await
            .map(Self)
    }
}

/// Whether a request says it comes with a session, valid or not.
pub(crate) fn claims_session(app: &App, headers: &HeaderMap) -> bool {
    app.authenticator()
        .session_cookie()
        .credential(headers)
        .is_some()
}

/// The session of a request, from the token in its `Authorization` header or in its
/// session cookie.
///
/// If this lookup is the one that writes down the use of a session that came in a
/// cookie, the cookie is due for renewal: that is noted in the request's
/// [`CookieRenewal`], where [`renew_session_cookie`] finds it.
///
/// # Errors
///
/// Returns a 401 if the request carries no token or one that is no session's, and the
/// store's error if the session cannot be looked up.
pub(crate) async fn session_of(
    app: &App,
    headers: &HeaderMap,
    extensions: &Extensions,
) -> Result<Session, AppError> {
    let credential = app.authenticator().session_cookie().credential(headers);
    let Some(credential) = credential else {
        return Err(AppError::unauthorized("no session token"));
    };
    let authenticated = app.authenticator().get_session(credential.token()).await?;
    if authenticated.use_recorded
        && let Credential::Cookie(token) = credential
        && let Some(renewal) = extensions.get::<CookieRenewal>()
    {
        renewal.note(token);
    }
    Ok(authenticated.session)
}

/// Where a request's handlers note that the session cookie is due for renewal: the
/// token to send back, with a fresh lifetime.
#[derive(Debug, Clone, Default)]
pub(crate) struct CookieRenewal(Arc<Mutex<Option<String>>>);

impl CookieRenewal {
    fn note(&self, token: String) {
        *self
            .0
            .lock()
            .expect("the renewal note is never held across a panic") = Some(token);
    }

    fn take(&self) -> Option<String> {
        self.0
            .lock()
            .expect("the renewal note is never held across a panic")
            .take()
    }
}

/// Sends the session cookie again, with its full lifetime, on the response to the
/// request that recorded the session's use: a browser forgets the cookie when the
/// session has been idle for as long as the server would let it, and not before.
///
/// A response that sets the cookie itself (login, logout, a changed password) is left
/// as it is.
pub(crate) async fn renew_session_cookie(
    State(app): State<App>,
    mut request: Request,
    next: Next,
) -> Response {
    let renewal = CookieRenewal::default();
    request.extensions_mut().insert(renewal.clone());
    let mut response = next.run(request).await;
    if let Some(token) = renewal.take()
        && !response.headers().contains_key(SET_COOKIE)
    {
        let cookie = app.authenticator().session_cookie().set(&token);
        response.headers_mut().insert(SET_COOKIE, cookie);
    }
    response
}
