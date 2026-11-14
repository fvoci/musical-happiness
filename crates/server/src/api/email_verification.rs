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
    OpenApiRouter::new().routes(routes!(complete))
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
