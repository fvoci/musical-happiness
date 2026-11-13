//! Password recovery without a session; reset tokens travel only in request bodies.

use std::convert::Infallible;
use std::pin::Pin;
use std::task::{Context, Poll};

use axum::body::{Body, Bytes};
use axum::extract::{Extension, State};
use futures_util::{Stream, future::BoxFuture};
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
/// All of the account's sessions and WebSocket connections end, and so do its other
/// reset links, as they do when the password is changed or set by an operator. The sign-in
/// lock of the account, at its user name and its email, ends, and so does the wait of the
/// address that finished the reset. A missing, expired or already used token answers the
/// same 400, naming `token` in `errors`.
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
    /// The requester's language tag (`ko`, `en-GB`), used when the account's own setting
    /// names no language the mail is written in (English, Korean); English if neither does.
    #[serde(default)]
    language: String,
}

/// Request a password reset
///
/// Answers 204 with an empty body for an existing account, an absent account and
/// an account without a password. Lookup and delivery run in the background;
/// the request never waits for either. Accounts without passwords receive no mail.
/// Requests are limited by address only, with the login setting of attempts per minute
/// and a separate counter (429); never by email. An account is sent at most
/// `mail_reset_per_account_hour` messages an hour: more requests are answered the same
/// and send nothing. A server that sends no mail answers 404.
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
    let work = app.prepare_password_reset(&request.email, &request.language, address_of(client))?;
    let mut response = no_content();
    *response.body_mut() = Body::from_stream(RecoveryResponse {
        app,
        work: Some(Box::pin(work)),
    });
    Ok(response)
}

/// The empty acknowledgement owns unstarted work until HTTP releases the response.
/// A 204 body is normally dropped without polling; a disconnected client also
/// releases it. Neither body polling nor dropping waits for the background job.
struct RecoveryResponse {
    app: App,
    work: Option<BoxFuture<'static, ()>>,
}

impl Stream for RecoveryResponse {
    type Item = Result<Bytes, Infallible>;

    fn poll_next(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Poll::Ready(None)
    }
}

impl Drop for RecoveryResponse {
    fn drop(&mut self) {
        if let Some(work) = self.work.take() {
            self.app.spawn_background(work);
        }
    }
}
