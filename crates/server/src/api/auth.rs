//! Login, logout, registration and password change.

use axum::body::Bytes;
use axum::extract::{Extension, State};
use axum::http::header::{SET_COOKIE, USER_AGENT};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse as _, Response};
use fvoci_model::{ChangePasswordRequest, LoginRequest, Problem, RegisterRequest, User};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::contract::tag;

use super::{ApiResult, RequiredSession, created, decode_object, no_content};
use crate::app::App;
use crate::client_address::ClientAddress;
use crate::error::AppError;

/// The longest `User-Agent` kept with a session, in bytes. Browsers send a hundred or
/// two; anything longer is cut, it is a label and nothing is decided by it.
const USER_AGENT_MAX_BYTES: usize = 512;

/// What the client says it is, cut to [`USER_AGENT_MAX_BYTES`] at a character boundary.
pub(super) fn user_agent(headers: &HeaderMap) -> String {
    let Some(value) = headers
        .get(USER_AGENT)
        .and_then(|value| value.to_str().ok())
    else {
        return String::new();
    };
    let mut end = value.len().min(USER_AGENT_MAX_BYTES);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_owned()
}

/// The routes that need a session, or none of their own doing.
pub(super) fn routes() -> OpenApiRouter<App> {
    OpenApiRouter::new()
        .routes(routes!(handle_logout))
        .routes(routes!(handle_change_password))
}

/// The routes anyone can send a body to: they come before there is a session.
pub(super) fn unauthenticated_routes() -> OpenApiRouter<App> {
    OpenApiRouter::new()
        .routes(routes!(handle_login))
        .routes(routes!(handle_register))
}

/// Sign in
///
/// Starts a session for the user with the given name, or, if no user has that name,
/// with the given email address. The session comes in the `Set-Cookie` header and
/// nowhere else.
///
/// Wrong credentials are 401 with the reason `invalid_credentials`, whatever was wrong
/// about them, a missing password too. Attempts are counted at the name and at the
/// email address the request gives, each for itself, and by where they come from; too
/// many are 429. What is counted does not depend on whether there is such an account.
#[utoipa::path(
    post,
    path = "/login",
    operation_id = "login",
    tag = tag::AUTH,
    request_body = LoginRequest,
    security(()),
    responses(
        (
            status = 204,
            description = "The user is signed in.",
            headers(("Set-Cookie" = String, description = "The session cookie.")),
        ),
        (
            status = 429,
            description = "Too many attempts. `retryAfter`, and the `Retry-After` header, say how long to wait; there is no `reason`.",
            body = Problem,
            content_type = "application/problem+json",
            headers(("Retry-After" = u64, description = "How many seconds to wait before trying again.")),
        ),
    ),
)]
async fn handle_login(
    State(app): State<App>,
    client: Option<Extension<ClientAddress>>,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult {
    let login: LoginRequest = decode_object(&body)?;
    let token = app
        .login(
            &login.username,
            &login.email,
            &login.password,
            &user_agent(&headers),
            address_of(client),
        )
        .await?;
    // The token goes out in the cookie only: a script of the page never sees it. A
    // client that is no browser reads it from the `Set-Cookie` header.
    Ok(with_session_cookie(&app, &token, no_content()))
}

/// Adds the session cookie with the token to a response.
pub(super) fn with_session_cookie(app: &App, token: &str, mut response: Response) -> Response {
    let cookie = app.authenticator().session_cookie().set(token);
    response.headers_mut().insert(SET_COOKIE, cookie);
    response
}

/// Sign out
///
/// Ends the session of the request and clears its cookie.
#[utoipa::path(
    post,
    path = "/logout",
    operation_id = "logout",
    tag = tag::AUTH,
    responses((
        status = 204,
        description = "The session is over.",
        headers(("Set-Cookie" = String, description = "Clears the session cookie.")),
    )),
)]
async fn handle_logout(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
) -> ApiResult {
    app.logout(&session.id).await?;
    let mut response = no_content();
    response
        .headers_mut()
        .insert(SET_COOKIE, app.authenticator().session_cookie().clear());
    Ok(response)
}

/// The address of the client, as the request log worked it out.
pub(super) fn address_of(client: Option<Extension<ClientAddress>>) -> Option<std::net::IpAddr> {
    client.and_then(|Extension(ClientAddress(address))| address)
}

/// Register a user
///
/// Makes an account and the organization it starts: `organizationName` names the
/// organization and its first workspace, and the account administers both. On a server that takes one organization (`org_creation` is `first_user_only`)
/// only the first user registers, and administers the server too; everybody after is
/// refused with 403 and the reason `invitation_required`, whatever else the request
/// says, and gets in by the link of an invitation (`acceptInvitation`). A name or an
/// email address that is taken is 409 with the reason `username_taken` or
/// `email_taken`. Registering does not sign in.
///
/// On a server that asks for the email address to be confirmed (`emailVerification` of
/// `getLoginOptions`), registering makes no account: it answers 202 with no body, and
/// mails a link to the address, in the language of `language` (English or Korean), that
/// makes the account and the organization and signs in (`completeEmailVerification`).
/// Until then there is nothing of it but its name and address, which are taken as an
/// account's are, for seven days; a registration that is not confirmed by then is
/// removed, and its name and address are free again.
///
/// The name and the email address must not be blank, the email address must look like
/// one, the display name (`displayName`, may be left out) is read as `patchMe` reads it,
/// the password must have at least 8 characters and at most 1024 bytes, and the organization
/// name must not be blank, and is otherwise read as a display name is; otherwise the
/// request is 400, and `errors` names every member that is refused, each with
/// its `reason`: `required`, `invalid_format`, `too_short`, `too_long` or
/// `invalid_characters`.
#[utoipa::path(
    post,
    path = "/register",
    operation_id = "register",
    tag = tag::AUTH,
    request_body = RegisterRequest,
    security(()),
    responses(
        (status = 201, description = "The user that was made.", body = User),
        (status = 202, description = "The registration waits for its email address to be confirmed by the link mailed to it."),
        (
            status = 429,
            description = "Too many registrations from where this one comes from, or the server has too much password work to do at the moment.",
            body = Problem,
            content_type = "application/problem+json",
            headers(("Retry-After" = u64, description = "How many seconds to wait before trying again.")),
        ),
    ),
)]
async fn handle_register(
    State(app): State<App>,
    client: Option<Extension<ClientAddress>>,
    body: Bytes,
) -> ApiResult {
    let register: RegisterRequest = decode_object(&body)?;
    // Asked first, so that a request that is refused whatever it says costs no
    // password hash; whether a registration really is the first is decided where the
    // user is stored.
    if !app.takes_registrations().await? {
        return Err(AppError::invitation_required());
    }
    let register = register.tidied();
    register.validate()?;
    if app.verifies_email() {
        app.sign_up_to_confirm(&register, &register.language, address_of(client))
            .await?;
        return Ok(StatusCode::ACCEPTED.into_response());
    }
    let user = app.sign_up(&register, address_of(client)).await?;
    Ok(created(&user))
}

/// Change the password
///
/// Changes the password of the user of the session, who proves to know the old one.
/// The user's other sessions end, and this one goes on under a new cookie.
///
/// A wrong old password is 400 (not 401: the session is good) with the reason
/// `invalid_credentials`, and `errors` names `oldPassword`. Wrong ones are counted with the
/// failed logins to the account by its name, and too many are 429, also for a login.
/// The new password must have at least 8 characters and at most 1024 bytes; `errors`
/// names every member that is refused. A password
/// that was changed by somebody else while this request was under way is 409.
#[utoipa::path(
    post,
    path = "/users/me/password",
    operation_id = "changePassword",
    tag = tag::AUTH,
    request_body = ChangePasswordRequest,
    responses(
        (
            status = 204,
            description = "The password is changed.",
            headers(("Set-Cookie" = String, description = "The session cookie, with its new value.")),
        ),
        (
            status = 429,
            description = "The old password was wrong too often, or the server has too much password work to do at the moment.",
            body = Problem,
            content_type = "application/problem+json",
            headers(("Retry-After" = u64, description = "How many seconds to wait before trying again.")),
        ),
    ),
)]
async fn handle_change_password(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    client: Option<Extension<ClientAddress>>,
    body: Bytes,
) -> ApiResult {
    let request: ChangePasswordRequest = decode_object(&body)?;
    request.validate()?;
    let address = address_of(client);

    let token = app
        .change_password(
            &session,
            &request.old_password,
            &request.new_password,
            address,
        )
        .await?;
    // The session goes on under a new token, which replaces the old one in the cookie.
    Ok(with_session_cookie(&app, &token, no_content()))
}
