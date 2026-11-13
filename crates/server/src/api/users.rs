//! Users: the signed-in user, other users' profiles, and preferences.

use std::collections::BTreeMap;

use axum::body::Bytes;
use axum::extract::{Extension, Path, State};
use fvoci_model::{
    BoardMember, LinkedIdentity, Problem, RefusalReason, Refusals, UnlinkRequest, User,
    UserPreferencesPatch, UserSettings, UserSettingsPatch, display_name, shown_name,
};
use fvoci_store::{InstanceRole, Preference};
use serde::Deserialize;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::contract::bodies::{AccountChange, AdminChange, SettingsChange};
use super::contract::params::UserId;
use super::contract::tag;

use super::auth::address_of;
use super::{
    ApiResult, RequiredSession, decode_json, decode_object, json_object, json_response, no_content,
    require_admin,
};
use crate::app::App;
use crate::client_address::ClientAddress;
use crate::error::AppError;

pub(super) fn routes() -> OpenApiRouter<App> {
    OpenApiRouter::new()
        .routes(routes!(handle_get_me, handle_patch_me))
        .routes(routes!(handle_get_my_memberships))
        .routes(routes!(handle_get_client_state, handle_patch_client_state))
        .routes(routes!(
            handle_get_user_settings,
            handle_patch_user_settings
        ))
        .routes(routes!(handle_get_my_identities, handle_unlink_identity))
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
/// be shown by. White space around it is dropped, and empty or `null` is none. At most
/// 64 UTF-16 code units, without control characters and without characters that show
/// nothing, break a line or turn the text around (U+00AD, U+200B–U+200F,
/// U+2028–U+202E, U+2060–U+206F, U+FEFF, U+FFF9–U+FFFB). Not unique. A member that is
/// no such thing, or a name that breaks these rules, is refused (400 naming it) and
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
    let mut requested = None;
    // Every member that is refused, not the first alone.
    let mut refused = Refusals::new();
    for (name, value) in &members {
        match (name.as_str(), value) {
            ("displayName", serde_json::Value::Null) => requested = Some(String::new()),
            ("displayName", serde_json::Value::String(name)) => match display_name(name) {
                Ok(name) => requested = Some(name),
                Err(error) => refused.push(error.into()),
            },
            ("displayName", _) => refused.refuse(
                "displayName",
                RefusalReason::Invalid,
                "displayName must be a string or null",
            ),
            (other, _) => refused.refuse(
                other,
                RefusalReason::Unknown,
                format!("there is no member {:?}", shown_name(other)),
            ),
        }
    }
    refused.into_result()?;
    let mut user = match requested {
        Some(name) => app.set_display_name(&session.user_id, &name).await?,
        None => app.user(&session.user_id).await?,
    };
    crate::app::sanitize(&mut user);
    Ok(json_response(&user))
}

/// The boards the user is a member of
///
/// The user's memberships, with the roles of each, of the boards of the workspaces the
/// user is a member of. By the identifier of the board.
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
    // The boards are of the organization the server stands in, and name no workspace.
    app.require_session_opens_here(&session).await?;
    let members = app.store().get_members_for_user(&session.user_id).await?;
    Ok(json_response(&members))
}

/// The identities linked to the account
///
/// Who the user of the session is at the providers they linked to the account, or
/// first signed in through: an organization's provider with that issuer signs in to
/// this account. Linked by `startOidcLogin` with `linkPassword`. Oldest first.
#[utoipa::path(
    get,
    path = "/users/me/identities",
    operation_id = "getMyIdentities",
    tag = tag::AUTH,
    responses((status = 200, description = "The identities.", body = Vec<LinkedIdentity>)),
)]
async fn handle_get_my_identities(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
) -> ApiResult {
    Ok(json_response(&app.identities(&session.user_id).await?))
}

/// Remove an identity from the account
///
/// Removes the identity of `issuer` and `subject` from the account of the session: the
/// providers of that issuer no longer sign in to it. Asks for the account's password,
/// as `changePassword` does: a wrong one is 400 with the reason `invalid_credentials`,
/// naming `/password`, and too many are 429. An identity that is not the account's is
/// 404. An account without a password keeps its last identity, which is all that lets
/// its owner in: 409 with the reason `last_sign_in`; and has no password to confirm
/// removing another with: 409 with the reason `password_not_set`. The sessions made
/// through a provider go on.
#[utoipa::path(
    delete,
    path = "/users/me/identities",
    operation_id = "unlinkIdentity",
    tag = tag::AUTH,
    request_body = UnlinkRequest,
    responses(
        (status = 204, description = "The identity is removed."),
        (
            status = 429,
            description = "The password was wrong too often, or the server has too much password work to do at the moment.",
            body = Problem,
            content_type = "application/problem+json",
            headers(("Retry-After" = u64, description = "How many seconds to wait before trying again.")),
        ),
    ),
)]
async fn handle_unlink_identity(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    client: Option<Extension<ClientAddress>>,
    body: Bytes,
) -> ApiResult {
    let request: UnlinkRequest = decode_object(&body)?;
    request.validate()?;
    app.unlink_identity(&session, &request, address_of(client))
        .await?;
    Ok(no_content())
}

/// A user
///
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
    // What is changed is the organization's the server stands in, which no path names.
    app.require_session_opens_here(&session).await?;
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

    let mut user = app.set_user_admin(&user_id, role, &session.user_id).await?;
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
    patch.validate()?;
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
    let patch = UserSettingsPatch::from_members(&members)?;
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
