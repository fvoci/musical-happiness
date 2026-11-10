//! Users: the signed-in user, other users' profiles, and preferences.

use axum::Router;
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::routing::{get, post, put};
use fvoci_model::{User, UserPreferencesPatch};
use fvoci_store::InstanceRole;
use serde::Deserialize;

use super::{ApiResult, RequiredSession, decode_json, decode_object, json_response, require_admin};
use crate::app::App;
use crate::error::AppError;

pub(super) fn routes() -> Router<App> {
    Router::new()
        .route("/users", post(handle_get_users_list))
        .route("/users/me", get(handle_get_me))
        .route("/users/me/memberships", get(handle_get_my_memberships))
        .route("/users/me/config", get(handle_get_user_preferences))
        .route("/users/{user_id}", get(handle_get_user))
        .route("/users/{user_id}/admin", put(handle_set_user_admin))
        .route("/users/{user_id}/config", put(handle_update_user_config))
}

/// What a request misses that names a user who is not there.
const USER: &str = "user";

/// Says which kind of thing was not found, for a lookup of a user.
fn user_not_found(error: AppError) -> AppError {
    if error.is_not_found() {
        AppError::not_found(USER)
    } else {
        error
    }
}

async fn handle_get_users_list(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    body: Bytes,
) -> ApiResult {
    let user_ids: Vec<String> = decode_json(&body)?;
    if user_ids.is_empty() {
        return Err(AppError::bad_request("the list of user IDs is empty"));
    }
    let users = app.users_list(&user_ids).await?;

    // Whether the asking user sees others does not depend on whom they ask about.
    if !app.can_see_user(&session.user_id).await? {
        return Ok(json_response(&Vec::<User>::new()));
    }
    let mut sanitized = Vec::with_capacity(users.len());
    for mut user in users {
        if user.id == session.user_id {
            crate::app::sanitize(&mut user);
        } else {
            app.sanitize_profile(&mut user);
        }
        sanitized.push(user);
    }
    Ok(json_response(&sanitized))
}

async fn handle_get_me(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
) -> ApiResult {
    let mut user = app.user(&session.user_id).await.map_err(user_not_found)?;

    crate::app::sanitize(&mut user);
    Ok(json_response(&user))
}

async fn handle_get_my_memberships(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
) -> ApiResult {
    let members = app.store().get_members_for_user(&session.user_id).await?;
    Ok(json_response(&members))
}

async fn handle_get_user(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    Path(user_id): Path<String>,
) -> ApiResult {
    let mut user = app.user(&user_id).await.map_err(user_not_found)?;
    if !app
        .can_see_user(&session.user_id)
        .await
        .is_ok_and(|sees| sees)
    {
        return Err(AppError::not_found(USER));
    }
    if user_id == session.user_id {
        crate::app::sanitize(&mut user);
    } else {
        app.sanitize_profile(&mut user);
    }
    Ok(json_response(&user))
}

/// The body of `PUT /users/{user_id}/admin`.
#[derive(Deserialize)]
struct AdminFlag {
    #[serde(rename = "isAdmin")]
    is_admin: Option<bool>,
}

/// `PUT /users/{user_id}/admin`: makes a user an administrator of the instance, or ends
/// that. For administrators. Handing the role over is two requests: make the other one
/// an administrator, then stop being one.
async fn handle_set_user_admin(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    Path(user_id): Path<String>,
    body: Bytes,
) -> ApiResult {
    require_admin(&app, &session.user_id).await?;
    let flag: AdminFlag = decode_object(&body)?;
    let role = match flag.is_admin {
        Some(true) => InstanceRole::Administrator,
        Some(false) => InstanceRole::User,
        None => {
            return Err(
                AppError::bad_request("isAdmin must be true or false").with_field("isAdmin")
            );
        }
    };

    let mut user = app.set_user_admin(&user_id, role).await?;
    if user.id == session.user_id {
        crate::app::sanitize(&mut user);
    } else {
        app.sanitize_profile(&mut user);
    }
    Ok(json_response(&user))
}

async fn handle_update_user_config(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    Path(user_id): Path<String>,
    body: Bytes,
) -> ApiResult {
    // The settings of another user are nobody's business; for this one they do not
    // exist.
    if user_id != session.user_id {
        return Err(AppError::not_found(USER));
    }
    let patch: UserPreferencesPatch = decode_object(&body)?;
    Ok(json_response(
        &app.update_user_config(&user_id, &patch).await?,
    ))
}

async fn handle_get_user_preferences(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
) -> ApiResult {
    Ok(json_response(
        &app.user_preferences(&session.user_id).await?,
    ))
}
