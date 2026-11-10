//! Login, logout, registration, password change and signup-token regeneration.

use axum::Router;
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::routing::post;
use fvoci_model::{ChangePasswordRequest, LoginRequest, LoginResponse, RegisterRequest};

use super::{ApiResult, RequiredSession, decode_object, empty_response, json_response};
use crate::app::App;
use crate::auth::tokens_match;
use crate::error::{AppError, Reason};

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

/// Accounts do not exist in single-user mode.
fn reject_in_single_user_mode(app: &App) -> Result<(), AppError> {
    if app.single_user_token().is_some() {
        return Err(AppError::forbidden("not permitted in single-user mode")
            .with_reason(Reason::SingleUserMode));
    }
    Ok(())
}

async fn handle_login(State(app): State<App>, body: Bytes) -> ApiResult {
    reject_in_single_user_mode(&app)?;
    let login: LoginRequest = decode_object(&body)?;
    if login.login_type != "normal" {
        return Err(AppError::bad_request("invalid login type").with_field("type"));
    }
    let token = app
        .login(&login.username, &login.email, &login.password)
        .await?;
    Ok(json_response(&LoginResponse { token }))
}

async fn handle_logout(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
) -> ApiResult {
    reject_in_single_user_mode(&app)?;
    app.logout(&session.id).await?;
    Ok(empty_response())
}

async fn handle_register(State(app): State<App>, body: Bytes) -> ApiResult {
    reject_in_single_user_mode(&app)?;
    let mut register: RegisterRequest = decode_object(&body)?;
    register.email = register.email.trim().to_owned();
    register.username = register.username.trim().to_owned();

    // The first user needs no signup token; everyone after needs the team's.
    if register.token.is_empty() {
        if app.registered_user_count().await? > 0 {
            return Err(AppError::forbidden("registration needs the signup token")
                .with_reason(Reason::SignupTokenRequired)
                .with_field("token"));
        }
    } else {
        let team = app.root_team().await?;
        if !tokens_match(&team.signup_token, &register.token) {
            return Err(AppError::forbidden("the signup token is not valid")
                .with_reason(Reason::SignupTokenInvalid)
                .with_field("token"));
        }
    }
    register.validate()?;

    app.register_user(&register.username, &register.email, &register.password)
        .await?;
    Ok(empty_response())
}

async fn handle_regenerate_signup_token(
    State(app): State<App>,
    RequiredSession(_session): RequiredSession,
) -> ApiResult {
    app.regenerate_signup_token().await?;
    Ok(empty_response())
}

async fn handle_change_password(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    Path(user_id): Path<String>,
    body: Bytes,
) -> ApiResult {
    reject_in_single_user_mode(&app)?;
    // The original lets any signed-in user change any user's password, provided they
    // know that user's current password. A password change is for one's own account.
    if session.user_id != user_id {
        return Err(AppError::forbidden(
            "access denied to change another user's password",
        ));
    }
    let request: ChangePasswordRequest = decode_object(&body)?;
    request.validate()?;

    app.change_password(&user_id, &request.old_password, &request.new_password)
        .await?;
    Ok(empty_response())
}
