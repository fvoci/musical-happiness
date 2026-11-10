//! Login, logout, registration, password change and signup-token regeneration.

use axum::Router;
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::http::header::USER_AGENT;
use axum::routing::post;
use fvoci_model::{ChangePasswordRequest, LoginRequest, LoginResponse, RegisterRequest};

use super::{
    ApiResult, RequiredSession, decode_object, empty_response, json_response, require_admin,
    require_team_access,
};
use crate::app::{Admission, App};
use crate::auth::tokens_match;
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
pub(super) fn routes() -> Router<App> {
    Router::new()
        .route("/logout", post(handle_logout))
        .route(
            "/teams/{team_id}/regenerate_signup_token",
            post(handle_regenerate_signup_token),
        )
        .route(
            "/users/{user_id}/changepassword",
            post(handle_change_password),
        )
}

/// The routes anyone can send a body to: they come before there is a session.
pub(super) fn unauthenticated_routes() -> Router<App> {
    Router::new()
        .route("/login", post(handle_login))
        .route("/register", post(handle_register))
}

async fn handle_login(State(app): State<App>, headers: HeaderMap, body: Bytes) -> ApiResult {
    let login: LoginRequest = decode_object(&body)?;
    if login.login_type != "normal" {
        return Err(AppError::bad_request("invalid login type").with_field("type"));
    }
    let token = app
        .login(
            &login.username,
            &login.email,
            &login.password,
            &user_agent(&headers),
        )
        .await?;
    Ok(json_response(&LoginResponse { token }))
}

async fn handle_logout(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
) -> ApiResult {
    app.logout(&session.id).await?;
    Ok(empty_response())
}

async fn handle_register(State(app): State<App>, body: Bytes) -> ApiResult {
    let mut register: RegisterRequest = decode_object(&body)?;
    register.email = register.email.trim().to_owned();
    register.username = register.username.trim().to_owned();

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

    app.register_user(
        &register.username,
        &register.email,
        &register.password,
        admission,
    )
    .await?;
    Ok(empty_response())
}

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

async fn handle_change_password(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    Path(user_id): Path<String>,
    body: Bytes,
) -> ApiResult {
    // The original lets any signed-in user change any user's password, provided they
    // know that user's current password. A password change is for one's own account.
    if session.user_id != user_id {
        return Err(AppError::forbidden(
            "access denied to change another user's password",
        ));
    }
    let request: ChangePasswordRequest = decode_object(&body)?;
    request.validate()?;

    app.change_password(&session, &request.old_password, &request.new_password)
        .await?;
    Ok(empty_response())
}
