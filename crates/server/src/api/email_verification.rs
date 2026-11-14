//! Confirming the email address of a registration without a session; the links'
//! tokens travel only in request bodies.

use axum::body::Bytes;
use axum::extract::{Extension, State};
use axum::http::HeaderMap;
use fvoci_model::AcceptedInvitation;
use serde::Deserialize;
use utoipa_axum::{router::OpenApiRouter, routes};

use super::auth::{address_of, user_agent, with_session_cookie};
use super::contract::tag;
use super::password_reset::RecoveryResponse;
use super::{ApiResult, decode_object, json_response};
use crate::app::App;
use crate::client_address::ClientAddress;

#[derive(Deserialize, utoipa::ToSchema)]
struct CompleteEmailVerification {
    /// The secret from the confirmation link's fragment.
    #[serde(default)]
    token: String,
}

pub(super) fn routes() -> OpenApiRouter<App> {
    OpenApiRouter::new()
        .routes(routes!(complete))
        .routes(routes!(request))
}

#[derive(Deserialize, utoipa::ToSchema)]
struct RequestEmailVerification {
    /// The user name the registration was made with; or leave it empty and give the
    /// address.
    #[serde(default)]
    username: String,
    /// The address the registration was made with, used when no registration has the
    /// given name.
    #[serde(default)]
    email: String,
    /// The requester's language tag (`ko`, `en-GB`), used when the registration's own
    /// screen named no language the mail is written in (English, Korean); English if
    /// neither does.
    #[serde(default)]
    language: String,
}

/// Mail a new confirmation link
///
/// Answers 204 with an empty body whether or not a registration that is not confirmed
/// yet has the user name (or else the address), and whatever became of the message;
/// lookup and delivery run in the background. A new link ends the one before it. A
/// registration's address is sent at most `mail_reset_per_account_hour` messages an
/// hour, its first link counted: more requests are answered the same, send nothing
/// and lock nothing. Requests are limited by where they come from, with the requests
/// for a reset link (429). A request that names neither is 400. A server that does not
/// ask for addresses to be confirmed (`emailVerification` of `getLoginOptions`)
/// answers 404.
#[utoipa::path(
    post, path = "/email-verification/request", operation_id = "requestEmailVerification",
    tag = tag::AUTH, security(()), request_body = RequestEmailVerification,
    responses((status = 204, description = "If a registration waits, its address will receive a new link.")),
)]
async fn request(
    State(app): State<App>,
    client: Option<Extension<ClientAddress>>,
    body: Bytes,
) -> ApiResult {
    let request: RequestEmailVerification = decode_object(&body)?;
    let work = app.prepare_signup_mail(
        &request.username,
        &request.email,
        &request.language,
        address_of(client),
    )?;
    Ok(RecoveryResponse::releasing(app, work))
}

/// Confirm the email address of a registration
///
/// Consumes the token of the link mailed to a registration's address, within a day of
/// the mail and seven days of the registration: makes the account, and the
/// organization and first workspace it registered with, and signs it in, in whatever
/// browser the link was opened (the mailbox is the proof). The session comes in the
/// `Set-Cookie` header, as from `login`, and the answer says where the account was let
/// in. A link is used once, and a newer mail of the same registration ends the links
/// before it. A missing, ended, used or replaced token answers 400, naming `token` in
/// `errors`. On a server that takes one organization and has its first user by now,
/// the registration is removed and the answer is 403 with the reason
/// `invitation_required`. Requests are counted by where they come from, as
/// registrations are, and too many are 429.
#[utoipa::path(
    post, path = "/email-verification/complete", operation_id = "completeEmailVerification",
    tag = tag::AUTH, security(()), request_body = CompleteEmailVerification,
    responses(
        (
            status = 200,
            description = "The account was made and signed in, and was let in here.",
            body = AcceptedInvitation,
            headers(("Set-Cookie" = String, description = "The session cookie.")),
        ),
        (
            status = 429,
            description = "Too many requests from where this one comes from.",
            body = fvoci_model::Problem,
            content_type = "application/problem+json",
            headers(("Retry-After" = u64, description = "How many seconds to wait before trying again.")),
        ),
    ),
)]
async fn complete(
    State(app): State<App>,
    client: Option<Extension<ClientAddress>>,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult {
    let request: CompleteEmailVerification = decode_object(&body)?;
    let (token, confirmed) = app
        .confirm_signup(&request.token, &user_agent(&headers), address_of(client))
        .await?;
    let answer = AcceptedInvitation {
        workspace_id: confirmed.workspace_id,
        board_id: None,
    };
    Ok(with_session_cookie(&app, &token, json_response(&answer)))
}
