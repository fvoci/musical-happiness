//! Login, logout, registration, password change and signup-token regeneration.

use axum::Router;
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::routing::post;
use fvoci_model::{ChangePasswordRequest, LoginRequest, LoginResponse, RegisterRequest};

use super::{ApiResult, RequiredSession, decode_json_or_internal, empty_response, json_response};
use crate::app::App;
use crate::auth::tokens_match;
use crate::error::AppError;

pub(super) fn routes() -> Router<App> {
    Router::new()
        .route("/login", post(handle_login))
        .route("/logout", post(handle_logout))
        .route("/register", post(handle_register))
        .route(
            "/teams/{team_id}/regenerate_signup_token",
            post(handle_regenerate_signup_token),
        )
        .route(
            "/users/{user_id}/changepassword",
            post(handle_change_password),
        )
}

/// Accounts do not exist in single-user mode.
fn reject_in_single_user_mode(app: &App) -> Result<(), AppError> {
    if app.single_user_token().is_some() {
        return Err(AppError::unauthorized("not permitted in single-user mode"));
    }
    Ok(())
}

async fn handle_login(State(app): State<App>, body: Bytes) -> ApiResult {
    reject_in_single_user_mode(&app)?;
    let login: LoginRequest = decode_json_or_internal(&body)?;
    if login.login_type != "normal" {
        return Err(AppError::bad_request("invalid login type"));
    }
    let token = app
        .login(&login.username, &login.email, &login.password)
        .await
        .map_err(|_| AppError::unauthorized("incorrect login"))?;
    Ok(json_response(&LoginResponse { token }))
}

async fn handle_logout(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
) -> ApiResult {
    reject_in_single_user_mode(&app)?;
    app.logout(&session.id)
        .await
        .map_err(|_| AppError::unauthorized("incorrect logout"))?;
    Ok(empty_response())
}

async fn handle_register(State(app): State<App>, body: Bytes) -> ApiResult {
    reject_in_single_user_mode(&app)?;
    let mut register: RegisterRequest = decode_json_or_internal(&body)?;
    register.email = register.email.trim().to_owned();
    register.username = register.username.trim().to_owned();

    // The first user needs no signup token; everyone after needs the team's.
    if register.token.is_empty() {
        if app.registered_user_count().await? > 0 {
            return Err(AppError::unauthorized(
                "no sign-up token and user(s) already exist",
            ));
        }
    } else {
        let team = app.root_team().await?;
        if !tokens_match(&team.signup_token, &register.token) {
            return Err(AppError::unauthorized("invalid token"));
        }
    }
    register.validate()?;

    app.register_user(&register.username, &register.email, &register.password)
        .await
        .map_err(AppError::bad_request)?;
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
    let request: ChangePasswordRequest = decode_json_or_internal(&body)?;
    request.validate()?;

    // The original lets any signed-in user change any user's password, provided they
    // know that user's current password. A password change is for one's own account.
    if session.user_id != user_id {
        return Err(AppError::forbidden(
            "access denied to change another user's password",
        ));
    }

    app.change_password(&user_id, &request.old_password, &request.new_password)
        .await
        .map_err(AppError::bad_request)?;
    Ok(empty_response())
}
