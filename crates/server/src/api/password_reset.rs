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
    OpenApiRouter::new().routes(routes!(complete))
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
