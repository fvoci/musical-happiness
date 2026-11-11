//! Users: the signed-in user, other users' profiles, and preferences.

use std::collections::BTreeMap;

use axum::body::Bytes;
use axum::extract::{Path, State};
use fvoci_model::{
    BoardMember, User, UserPreferencesPatch, UserSettings, UserSettingsPatch, check_display_name,
};
use fvoci_store::{InstanceRole, Preference};
use serde::Deserialize;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::contract::bodies::{AccountChange, AdminChange, SettingsChange};
use super::contract::params::UserId;
use super::contract::tag;

use super::{
    ApiResult, RequiredSession, decode_json, decode_object, json_object, json_response,
    require_admin,
};
use crate::app::App;
use crate::error::AppError;

pub(super) fn routes() -> OpenApiRouter<App> {
    OpenApiRouter::new()
        .routes(routes!(handle_get_users_list))
        .routes(routes!(handle_get_me, handle_patch_me))
        .routes(routes!(handle_get_my_memberships))
        .routes(routes!(handle_get_client_state, handle_patch_client_state))
        .routes(routes!(
            handle_get_user_settings,
            handle_patch_user_settings
        ))
        .routes(routes!(handle_get_user))
        .routes(routes!(handle_set_user_admin))
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

/// Users by their identifiers
///
/// The users with the identifiers of the body, which must name at least one: an empty
/// list is 400. Every identifier must be a user's and none may be there twice;
/// otherwise the request is 404. Oldest account first, not in the order of the request.
#[utoipa::path(
    post,
    path = "/users",
    operation_id = "getUsers",
    tag = tag::USERS,
    request_body = Vec<String>,
    responses((status = 200, description = "The users.", body = Vec<User>)),
)]
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

/// The user of the session
#[utoipa::path(
    get,
    path = "/users/me",
    operation_id = "getMe",
    tag = tag::USERS,
    responses((status = 200, description = "The user.", body = User)),
)]
async fn handle_get_me(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
) -> ApiResult {
    let mut user = app.user(&session.user_id).await.map_err(user_not_found)?;

    crate::app::sanitize(&mut user);
    Ok(json_response(&user))
}

/// Change the user's account
///
/// Changes what the body names of the user of the session: `displayName`, the name to
/// be shown by (empty or `null` for none). A member that is no such thing, or a name
/// that is too long or holds a control character, is refused (400 naming it) and
/// changes nothing. Answers the user as `getMe` does.
#[utoipa::path(
    patch,
    path = "/users/me",
    operation_id = "patchMe",
    tag = tag::USERS,
    request_body = AccountChange,
    responses((status = 200, description = "The user as they are now.", body = User)),
)]
async fn handle_patch_me(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    body: Bytes,
) -> ApiResult {
    let members = json_object(decode_json(&body)?)?;
    let mut display_name = None;
    for (name, value) in &members {
        match (name.as_str(), value) {
            ("displayName", serde_json::Value::Null) => display_name = Some(String::new()),
            ("displayName", serde_json::Value::String(name)) => display_name = Some(name.clone()),
            ("displayName", _) => {
                return Err(
                    AppError::bad_request("displayName must be a string or null")
                        .with_field("displayName"),
                );
            }
            (other, _) => {
                return Err(
                    AppError::bad_request(format_args!("there is no member {other:?}"))
                        .with_field(other),
                );
            }
        }
    }
    let mut user = match display_name {
        Some(name) => {
            check_display_name(&name)?;
            app.set_display_name(&session.user_id, &name).await?
        }
        None => app.user(&session.user_id).await?,
    };
    crate::app::sanitize(&mut user);
    Ok(json_response(&user))
}

/// The boards the user is a member of
///
/// The user's memberships, with the roles of each. By the identifier of the board.
#[utoipa::path(
    get,
    path = "/users/me/memberships",
    operation_id = "getMyMemberships",
    tag = tag::USERS,
    responses((status = 200, description = "The memberships.", body = Vec<BoardMember>)),
)]
async fn handle_get_my_memberships(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
) -> ApiResult {
    let members = app.store().get_members_for_user(&session.user_id).await?;
    Ok(json_response(&members))
}

/// A user
#[utoipa::path(
    get,
    path = "/users/{userId}",
    operation_id = "getUser",
    tag = tag::USERS,
    params(UserId),
    responses((status = 200, description = "The user.", body = User)),
)]
async fn handle_get_user(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    Path(user_id): Path<String>,
) -> ApiResult {
    let mut user = app.user(&user_id).await.map_err(user_not_found)?;
    // A lookup that failed is no answer to who may be seen.
    if !app.can_see_user(&session.user_id).await? {
        return Err(AppError::not_found(USER));
    }
    if user_id == session.user_id {
        crate::app::sanitize(&mut user);
    } else {
        app.sanitize_profile(&mut user);
    }
    Ok(json_response(&user))
}

/// The body of `PUT /users/{userId}/admin`.
#[derive(Deserialize)]
struct AdminFlag {
    #[serde(rename = "isAdmin")]
    is_admin: Option<bool>,
}

/// Make a user an administrator, or end that
///
/// Makes a user an administrator of the instance, or ends that. For administrators;
/// anybody else is refused with 403 and the reason `admin_required`. The last
/// administrator cannot stop being one, which is 409 with the reason `last_admin`:
/// handing the role over is two requests, first making the other one an administrator.
/// A body without `isAdmin` is 400.
#[utoipa::path(
    put,
    path = "/users/{userId}/admin",
    operation_id = "setUserAdmin",
    tag = tag::USERS,
    params(UserId),
    request_body = AdminChange,
    responses((status = 200, description = "The user as they are now.", body = User)),
)]
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

/// The bag of client state as it goes out: the value of every entry by its name.
fn client_state(entries: Vec<Preference>) -> BTreeMap<String, String> {
    entries
        .into_iter()
        .map(|entry| (entry.name, entry.value))
        .collect()
}

/// Change the client's state
///
/// Sets and removes entries of the state the client keeps with the account. The
/// server does not know what the entries mean; it bounds their number, their names and
/// their size.
#[utoipa::path(
    patch,
    path = "/users/me/client-state",
    operation_id = "patchClientState",
    tag = tag::USERS,
    request_body = UserPreferencesPatch,
    responses((
        status = 200,
        description = "All of the state as it is now: the value of every entry by its name.",
        body = BTreeMap<String, String>,
    )),
)]
async fn handle_patch_client_state(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    body: Bytes,
) -> ApiResult {
    let patch: UserPreferencesPatch = decode_object(&body)?;
    patch
        .validate()
        .map_err(|error| AppError::bad_request(error).with_field("updatedFields"))?;
    let entries = app.update_user_config(&session.user_id, &patch).await?;
    Ok(json_response(&client_state(entries)))
}

/// The user's settings
///
/// What the user has set about how things are shown to them. Every setting is
/// answered; what was never set has its default.
#[utoipa::path(
    get,
    path = "/users/me/settings",
    operation_id = "getUserSettings",
    tag = tag::USERS,
    responses((status = 200, description = "The settings.", body = UserSettings)),
)]
async fn handle_get_user_settings(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
) -> ApiResult {
    Ok(json_response(&app.user_settings(&session.user_id).await?))
}

/// Change the user's settings
///
/// Changes the settings the body names. A member that is `null` puts its setting back
/// to the default. A member that is no setting, or a value its setting cannot have, is
/// refused and changes nothing.
#[utoipa::path(
    patch,
    path = "/users/me/settings",
    operation_id = "patchUserSettings",
    tag = tag::USERS,
    request_body = SettingsChange,
    responses((status = 200, description = "All settings as they are now.", body = UserSettings)),
)]
async fn handle_patch_user_settings(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    body: Bytes,
) -> ApiResult {
    let members = json_object(decode_json(&body)?)?;
    let patch = UserSettingsPatch::from_members(&members).map_err(|error| {
        let field = error.field().to_owned();
        AppError::bad_request(error).with_field(&field)
    })?;
    Ok(json_response(
        &app.patch_user_settings(&session.user_id, &patch).await?,
    ))
}

/// The client's state
///
/// The state the client keeps with the account.
#[utoipa::path(
    get,
    path = "/users/me/client-state",
    operation_id = "getClientState",
    tag = tag::USERS,
    responses((
        status = 200,
        description = "The value of every entry by its name.",
        body = BTreeMap<String, String>,
    )),
)]
async fn handle_get_client_state(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
) -> ApiResult {
    let entries = app.user_preferences(&session.user_id).await?;
    Ok(json_response(&client_state(entries)))
}
