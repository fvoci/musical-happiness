//! Users: the signed-in user, other users' profiles, and preferences.

use axum::Router;
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::routing::{get, post, put};
use fvoci_model::{SINGLE_USER_ID, User, UserPreferencesPatch};

use super::{ApiResult, RequiredSession, decode_json, json_response};
use crate::app::App;
use crate::auth::now_millis;
use crate::error::AppError;

pub(super) fn routes() -> Router<App> {
    Router::new()
        .route("/users", post(handle_get_users_list))
        .route("/users/me", get(handle_get_me))
        .route("/users/me/memberships", get(handle_get_my_memberships))
        .route("/users/me/config", get(handle_get_user_preferences))
        .route("/users/{user_id}", get(handle_get_user))
        .route("/users/{user_id}/config", put(handle_update_user_config))
}

/// The user record that stands for the one user of single-user mode.
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

pub(super) async fn single_user(app: &App) -> User {
    // The root team's last change stands in for the user's creation time.
    let create_at = app
        .root_team()
        .await
        .map(|team| team.update_at)
        .unwrap_or_default();
    User {
        id: SINGLE_USER_ID.to_owned(),
        username: SINGLE_USER_ID.to_owned(),
        email: SINGLE_USER_ID.to_owned(),
        create_at,
        update_at: now_millis(),
        ..User::default()
    }
}

async fn handle_get_users_list(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    body: Bytes,
) -> ApiResult {
    let user_ids: Vec<String> = decode_json(&body)?;
    let Some(first) = user_ids.first() else {
        return Err(AppError::bad_request("the list of user IDs is empty"));
    };
    let users = if first == SINGLE_USER_ID {
        vec![single_user(&app).await]
    } else {
        app.users_list(&user_ids).await?
    };

    // Whether the asking user sees others does not depend on whom they ask about; there
    // is always at least one user here, so the lookup happens as in the original.
    // The one user of single-user mode has no record to look up.
    if session.user_id != SINGLE_USER_ID && !app.can_see_user(&session.user_id).await? {
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
    let mut user = if session.user_id == SINGLE_USER_ID {
        single_user(&app).await
    } else {
        app.user(&session.user_id).await.map_err(user_not_found)?
    };

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
    let mut user = if user_id == SINGLE_USER_ID && session.user_id == SINGLE_USER_ID {
        single_user(&app).await
    } else {
        let user = app.user(&user_id).await.map_err(user_not_found)?;
        if !app
            .can_see_user(&session.user_id)
            .await
            .is_ok_and(|sees| sees)
        {
            return Err(AppError::not_found(USER));
        }
        user
    };
    if user_id == session.user_id {
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
    let patch: UserPreferencesPatch = decode_json(&body)?;
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
