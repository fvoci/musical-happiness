//! Extractors that resolve the session of a request, and the renewal of the session
//! cookie.

use std::sync::{Arc, Mutex};

use axum::extract::{FromRequestParts, RawPathParams, Request, State};
use axum::http::header::SET_COOKIE;
use axum::http::request::Parts;
use axum::http::{Extensions, HeaderMap};
use axum::middleware::Next;
use axum::response::Response;
use fvoci_model::Session;

use crate::app::App;
use crate::auth::token::Credential;
use crate::error::{AppError, ErrorKind, Reason};

/// The session of a request to a route that names no workspace and works in the
/// organization the server stands in: a board, a card or another thing by its
/// identifier. Extraction fails with 401 if the request carries no valid session
/// token, and with 401 and the reason `sso_required` if the session does not open that
/// organization (`App::require_session_opens_here`).
///
/// TEMPORARY, with `only_organization_scope`: a route under `/w/{workspaceId}` is
/// asked where its workspace is entered (`Ws`), and a route that is the account's own
/// takes [`AccountSession`].
#[derive(Debug)]
pub(crate) struct RequiredSession(pub(crate) Session);

/// The session of a request to a route that is the account's own, of no organization:
/// who the user is, their settings and password, their workspaces, signing out. Also
/// for a handler that enters a workspace itself, which asks there. Extraction fails
/// with 401 if the request carries no valid session token; whether the session opens
/// an organization is not asked.
#[derive(Debug)]
pub(crate) struct AccountSession(pub(crate) Session);

/// The session of a request to a route that also serves anonymous readers of shared
/// boards. `None` if the request carries no valid session token; the handler then checks
/// the read token. A session that does not open the server's organization
/// ([`RequiredSession`]) is refused as there, but where the request carries the read
/// token that opens the board of its path: that request is read as the token's alone,
/// and who is signed in reads a public link as anybody does.
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
        let session = session_of(app, &parts.headers, &parts.extensions).await?;
        app.require_session_opens_here(&session).await?;
        Ok(Self(session))
    }
}

impl FromRequestParts<App> for AccountSession {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, app: &App) -> Result<Self, Self::Rejection> {
        session_of(app, &parts.headers, &parts.extensions)
            .await
            .map(Self)
    }
}

impl FromRequestParts<App> for OptionalSession {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, app: &App) -> Result<Self, Self::Rejection> {
        let session = match session_of(app, &parts.headers, &parts.extensions).await {
            Ok(session) => session,
            // No session is an answer these routes can work with (a read token may do).
            // A store that failed is not "no session": answered as one, it would make
            // clients throw a valid session away.
            Err(error) if error.kind() == ErrorKind::Unauthorized => return Ok(Self(None)),
            Err(error) => return Err(error),
        };
        match app.require_session_opens_here(&session).await {
            Ok(()) => Ok(Self(Some(session))),
            Err(error) if error.reason() == Some(Reason::SsoRequired) => {
                if opens_by_read_token(parts, app).await? {
                    Ok(Self(None))
                } else {
                    Err(error)
                }
            }
            Err(error) => Err(error),
        }
    }
}

/// Whether the request names a board in its path and, in its query, the read token
/// that opens it: the request of a reader of the board's public link.
async fn opens_by_read_token(parts: &mut Parts, app: &App) -> Result<bool, AppError> {
    let token = parts
        .uri
        .query()
        .and_then(|query| crate::auth::token::query_value(query, "readToken"))
        .unwrap_or_default();
    if token.is_empty() {
        return Ok(false);
    }
    let Ok(params) = RawPathParams::from_request_parts(parts, app).await else {
        return Ok(false);
    };
    let Some(board_id) = params
        .iter()
        .find(|(name, _)| *name == "boardId")
        .map(|(_, value)| value.to_owned())
    else {
        return Ok(false);
    };
    Ok(app.enter_shared_board(&board_id, &token).await?.is_some())
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
