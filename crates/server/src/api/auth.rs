//! Login, logout, registration, password change and signup-token regeneration.

use axum::body::Bytes;
use axum::extract::{Extension, Path, State};
use axum::http::HeaderMap;
use axum::http::header::{SET_COOKIE, USER_AGENT};
use axum::response::Response;
use fvoci_model::{
    ChangePasswordRequest, ErrorBody, LoginRequest, RegisterRequest, Team, User, display_name,
};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::contract::params::TeamId;
use super::contract::tag;

use super::{
    ApiResult, RequiredSession, created, decode_object, json_response, no_content, require_admin,
    require_team_access,
};
use crate::app::{Admission, App};
use crate::auth::tokens_match;
use crate::client_address::ClientAddress;
use crate::error::{AppError, Reason};

/// The longest `User-Agent` kept with a session, in bytes. Browsers send a hundred or
/// two; anything longer is cut, it is a label and nothing is decided by it.
const USER_AGENT_MAX_BYTES: usize = 512;

/// What the client says it is, cut to [`USER_AGENT_MAX_BYTES`] at a character boundary.
fn user_agent(headers: &HeaderMap) -> String {
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
        .routes(routes!(handle_regenerate_signup_token))
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
            description = "Too many attempts. `retry_after`, and the `Retry-After` header, say how long to wait; there is no `reason`.",
            body = ErrorBody,
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
fn with_session_cookie(app: &App, token: &str, mut response: Response) -> Response {
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
fn address_of(client: Option<Extension<ClientAddress>>) -> Option<std::net::IpAddr> {
    client.and_then(|Extension(ClientAddress(address))| address)
}

/// Register a user
///
/// Makes an account. The first user of a server needs no signup token and becomes its
/// administrator; everyone after needs the team's token, and is refused with 403 and
/// the reason `signup_token_required` without it. A token that is not the team's is 403
/// with the reason `signup_token_invalid`, for the first user too. A name or an email
/// address that is taken is 409 with the reason `username_taken` or `email_taken`.
/// Registering does not sign in.
///
/// The name and the email address must not be blank, the email address must look like
/// one, the display name (`displayName`, may be left out) is read as `patchMe` reads it,
/// and the password must have at least 8 characters and at most 1024 bytes; otherwise
/// the request is 400, and `field` names the member.
#[utoipa::path(
    post,
    path = "/register",
    operation_id = "register",
    tag = tag::AUTH,
    request_body = RegisterRequest,
    security(()),
    responses(
        (status = 201, description = "The user that was made.", body = User),
        (
            status = 429,
            description = "Too many registrations from where this one comes from, or the server has too much password work to do at the moment.",
            body = ErrorBody,
            headers(("Retry-After" = u64, description = "How many seconds to wait before trying again.")),
        ),
    ),
)]
async fn handle_register(
    State(app): State<App>,
    client: Option<Extension<ClientAddress>>,
    body: Bytes,
) -> ApiResult {
    let mut register: RegisterRequest = decode_object(&body)?;
    register.email = register.email.trim().to_owned();
    register.username = register.username.trim().to_owned();
    register.display_name = display_name(&register.display_name)?;

    // The first user needs no signup token; everyone after needs the team's. Asked
    // here so that a request without one costs no password hash; whether it really is
    // the first is decided where the user is stored.
    let admission = if register.token.is_empty() {
        if app.store().is_bootstrapped().await? {
            return Err(AppError::forbidden("registration needs the signup token")
                .with_reason(Reason::SignupTokenRequired)
                .with_field("token"));
        }
        Admission::FirstUserOnly
    } else {
        let team = app.root_team().await?;
        if !tokens_match(&team.signup_token, &register.token) {
            return Err(AppError::forbidden("the signup token is not valid")
                .with_reason(Reason::SignupTokenInvalid)
                .with_field("token"));
        }
        Admission::WithToken
    };
    register.validate()?;

    let user = app
        .register_user(&register, admission, address_of(client))
        .await?;
    Ok(created(&user))
}

/// Make a new signup token
///
/// Makes a new signup token for the team; the old one stops working. For
/// administrators.
#[utoipa::path(
    post,
    path = "/teams/{teamId}/signup-token",
    operation_id = "regenerateSignupToken",
    tag = tag::TEAMS,
    params(TeamId),
    responses((status = 200, description = "The team, with its new token.", body = Team)),
)]
async fn handle_regenerate_signup_token(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    Path(team_id): Path<String>,
) -> ApiResult {
    require_team_access(&app, &session.user_id, &team_id, "access denied to team").await?;
    require_admin(&app, &session.user_id).await?;
    let team = app
        .regenerate_signup_token(&team_id, &session.user_id)
        .await?;
    // The team with its new token: the one answer that carries it without being asked.
    Ok(json_response(&team))
}

/// Change the password
///
/// Changes the password of the user of the session, who proves to know the old one.
/// The user's other sessions end, and this one goes on under a new cookie.
///
/// A wrong old password is 400 (not 401: the session is good) with the reason
/// `invalid_credentials` and the field `oldPassword`. Wrong ones are counted with the
/// failed logins to the account by its name, and too many are 429, also for a login.
/// The new password must have at least 8 characters and at most 1024 bytes. A password
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
            body = ErrorBody,
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
