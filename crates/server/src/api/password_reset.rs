//! Password recovery without a session; reset tokens travel only in request bodies.

use axum::body::Bytes;
use axum::extract::{Extension, State};
use serde::Deserialize;
use utoipa_axum::{router::OpenApiRouter, routes};

use super::auth::address_of;
use super::contract::tag;
use super::{ApiResult, decode_object, no_content};
use crate::app::App;
use crate::client_address::ClientAddress;

#[derive(Deserialize, utoipa::ToSchema)]
struct FinishPasswordReset {
    /// The secret from the reset link's fragment.
    #[serde(default)]
    token: String,
    /// The new password: at least eight characters, at most 1024 bytes.
    password: String,
}

pub(super) fn routes() -> OpenApiRouter<App> {
    OpenApiRouter::new()
        .routes(routes!(complete))
        .routes(routes!(request))
}

/// Finish resetting a password
///
/// Consumes a reset token before its one-hour expiry and sets the new password.
/// All of the account's sessions and WebSocket connections end. A missing, expired
/// or already used token answers the same 400, naming `token` in `errors`.
#[utoipa::path(
    post, path = "/password-reset/complete", operation_id = "completePasswordReset",
    tag = tag::AUTH, security(()), request_body = FinishPasswordReset,
    responses((status = 204, description = "The password was reset; sign in again.")),
)]
async fn complete(
    State(app): State<App>,
    client: Option<Extension<ClientAddress>>,
    body: Bytes,
) -> ApiResult {
    let request: FinishPasswordReset = decode_object(&body)?;
    app.finish_password_reset(&request.token, &request.password, address_of(client))
        .await?;
    Ok(no_content())
}

#[derive(Deserialize, utoipa::ToSchema)]
struct RequestPasswordReset {
    /// The account's email address.
    email: String,
    /// Preferred language, normally en or ko; English if absent or unsupported.
    #[serde(default)]
    language: String,
}

/// Request a password reset
///
/// Answers 204 with an empty body for an existing account, an absent account and
/// an account without a password. Lookup and SMTP delivery run in the background;
/// the request never waits for either. Accounts without passwords receive no mail.
/// Requests are limited by address and normalized email using the login limit settings,
/// with separate counters. Without SMTP this route answers 404.
#[utoipa::path(
    post, path = "/password-reset/request", operation_id = "requestPasswordReset",
    tag = tag::AUTH, security(()), request_body = RequestPasswordReset,
    responses((status = 204, description = "If eligible, the account will receive a reset link.")),
)]
async fn request(
    State(app): State<App>,
    client: Option<Extension<ClientAddress>>,
    body: Bytes,
) -> ApiResult {
    let request: RequestPasswordReset = decode_object(&body)?;
    app.request_password_reset(&request.email, &request.language, address_of(client))?;
    Ok(no_content())
}
