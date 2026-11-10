//! Users: the signed-in user, other users' profiles, and preferences.

use axum::Router;
use axum::body::Bytes;
use axum::extract::{Path, RawQuery, State};
use axum::routing::{get, post, put};
use fvoci_model::{SINGLE_USER_ID, User, UserPreferencesPatch};

use super::{ApiResult, RequiredSession, decode_json_or_internal, json_response, query_param};
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
    let user_ids: Vec<String> = decode_json_or_internal(&body)?;
    let Some(first) = user_ids.first() else {
        return Err(AppError::bad_request("User IDs are empty"));
    };
    let users = if first == SINGLE_USER_ID {
        vec![single_user(&app).await]
    } else {
        app.users_list(&user_ids).await?
    };

    // Whether the asking user sees others does not depend on whom they ask about; there
    // is always at least one user here, so the lookup happens as in the original.
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
    RawQuery(query): RawQuery,
) -> ApiResult {
    let mut user = if session.user_id == SINGLE_USER_ID {
        single_user(&app).await
    } else {
        app.user(&session.user_id).await?
    };

    // Of the permissions the original reports here, a standalone server grants only the
    // one to post in a channel, to anyone who names a channel.
    let channel_id = query_param(query.as_deref(), "channelID");
    if !channel_id.is_empty() && !session.user_id.is_empty() {
        user.permissions.push("create_post".to_owned());
    }
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
    let mut user = app.user(&user_id).await?;
    if !app.can_see_user(&session.user_id).await? {
        return Err(AppError::not_found(format_args!("user ID={user_id}")));
    }
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
    let patch: UserPreferencesPatch = decode_json_or_internal(&body)?;
    if user_id != session.user_id {
        return Err(AppError::forbidden(""));
    }
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
