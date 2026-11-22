//! Changing the email address of the account of a session, and confirming the new
//! address without one; the link's token travels only in a request body.

use axum::body::Bytes;
use axum::extract::{Extension, State};
use fvoci_model::Problem;
use serde::Deserialize;
use utoipa_axum::{router::OpenApiRouter, routes};

use super::auth::address_of;
use super::contract::tag;
use super::recovery::RecoveryResponse;
use super::session::RequiredSession;
use super::{ApiResult, decode_object, no_content};
use crate::app::App;
use crate::client_address::ClientAddress;

pub(super) fn routes() -> OpenApiRouter<App> {
    OpenApiRouter::new()
        .routes(routes!(request, cancel))
        .routes(routes!(resend))
        .routes(routes!(confirm))
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
struct RequestEmailChange {
    /// The address the account is to have. White space around it is dropped.
    #[serde(default)]
    email: String,
    /// The account's password.
    #[serde(default)]
    password: String,
    /// The requester's language tag (`ko`, `en-GB`), used when the account's own
    /// settings name no language the mail is written in (English, Korean); English if
    /// neither does.
    #[serde(default)]
    language: String,
}

impl std::fmt::Debug for RequestEmailChange {
    /// Leaves out the password.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RequestEmailChange")
            .field("email", &self.email)
            .finish_non_exhaustive()
    }
}

/// Change the email address of the account
///
/// Asks for the address of the account of the session to become `email`. The
/// account's `password` is asked for in this request, whatever the session is and
/// whatever it has shown before: a session alone does not move the address that
/// password resets are mailed to. An account that has no password (`hasPassword` of
/// `getMe`) is 409 with the reason `password_not_set`, and sets one first
/// (`setPassword`). The account of a session that waits to be deleted changes nothing
/// (401, `account_deletion_pending`).
///
/// **On a server that sends mail** (`mailEnabled` of `getClientConfig`) the address
/// does not change yet. The account waits for the new one to be confirmed: `getMe`
/// answers it as `pendingEmail`, and a link is mailed to it
/// (`<server_url>/confirm-email#<token>`, for 24 hours, used once) that
/// `confirmEmailChange` takes. Until then the account's address is the one it had:
/// reset links go there, and `login` finds the account by it. An account waits for
/// one address: a new request replaces the one before, whose link confirms nothing
/// any more, and so does a change or a reset of the password. What is not confirmed
/// ends after seven days.
///
/// The answer is 204 whether or not another account or a registration has the
/// address: lookup and mail run once the answer is handed off, nothing is mailed for
/// an address somebody has, and nothing holds the address until it is confirmed. An
/// account, and an address, are mailed at most `mail_reset_per_account_hour` such
/// links an hour; more requests are answered the same and mail nothing.
///
/// **On a server that sends no mail** there is nothing to confirm an address with, as
/// at registration: the address changes with this request, and the answer is 409 with
/// the reason `email_taken` if another account or a registration has it, in any
/// letter case. The account's reset links end.
///
/// An address is told apart from another as at registration: by its letters in any
/// case. What is no address, or is the account's already, is 400 naming `email`. A
/// wrong password is 400 (not 401: the session is good) with the reason
/// `invalid_credentials`, naming `password`, counted with the failed logins to the
/// account by its name; too many are 429, also for a login. Requests are also limited
/// by where they come from, with the requests for a reset link (429). A password that
/// was changed while this request was under way is 409.
#[utoipa::path(
    post,
    path = "/users/me/email-change",
    operation_id = "requestEmailChange",
    tag = tag::USERS,
    request_body = RequestEmailChange,
    responses(
        (status = 204, description = "The account waits for the address to be confirmed, or, on a server that sends no mail, has it."),
        (
            status = 429,
            description = "The password was wrong too often, or too many requests came from where this one comes from.",
            body = Problem,
            content_type = "application/problem+json",
            headers(("Retry-After" = u64, description = "How many seconds to wait before trying again.")),
        ),
    ),
)]
async fn request(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    client: Option<Extension<ClientAddress>>,
    body: Bytes,
) -> ApiResult {
    let request: RequestEmailChange = decode_object(&body)?;
    let work = app
        .ask_email_change(
            &session,
            &request.email,
            &request.password,
            &request.language,
            address_of(client),
        )
        .await?;
    Ok(RecoveryResponse::releasing(app, work))
}

/// Take back a change of the email address
///
/// Ends what the account of the session waits for (`pendingEmail` of `getMe`): the
/// link that was mailed confirms nothing any more, and the account keeps the address
/// it has. No password is asked for: nothing changes of the account. 204 also when it
/// waited for nothing.
#[utoipa::path(
    delete,
    path = "/users/me/email-change",
    operation_id = "cancelEmailChange",
    tag = tag::USERS,
    responses((status = 204, description = "The account waits for no address.")),
)]
async fn cancel(State(app): State<App>, RequiredSession(session): RequiredSession) -> ApiResult {
    app.cancel_email_change(&session).await?;
    Ok(no_content())
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
struct ResendEmailChange {
    /// The requester's language tag, as in `requestEmailChange`.
    #[serde(default)]
    language: String,
}

/// Mail the link of a change of the email address again
///
/// Mails a new link to the address the account of the session waits for
/// (`pendingEmail` of `getMe`); the link of the mail before confirms nothing once
/// the new one is written. No password is asked for: the address is the one the
/// password was given for. Answers 204 whatever became of the message, as
/// `requestEmailChange` does, within the same hourly limits. 404 for an account that
/// waits for no address, and on a server that sends no mail. Requests are limited by
/// where they come from, with the requests for a reset link (429).
#[utoipa::path(
    post,
    path = "/users/me/email-change/resend",
    operation_id = "resendEmailChange",
    tag = tag::USERS,
    request_body = ResendEmailChange,
    responses((status = 204, description = "If nobody has the address, it will receive a new link.")),
)]
async fn resend(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    client: Option<Extension<ClientAddress>>,
    body: Bytes,
) -> ApiResult {
    let request: ResendEmailChange = decode_object(&body)?;
    let work = app
        .prepare_email_change_mail(&session, &request.language, address_of(client))
        .await?;
    Ok(RecoveryResponse::releasing(app, work))
}

#[derive(Deserialize, utoipa::ToSchema)]
struct ConfirmEmailChange {
    /// The secret from the confirmation link's fragment.
    #[serde(default)]
    token: String,
}

impl std::fmt::Debug for ConfirmEmailChange {
    /// Leaves out the token.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConfirmEmailChange").finish_non_exhaustive()
    }
}

/// Confirm the new email address of an account
///
/// Consumes the token of the link mailed by `requestEmailChange`, within a day of
/// the mail and seven days of the request: the address the link was mailed to becomes
/// the account's. No session is needed and none is made: the link shows that its
/// holder reads the new address, and the password was given when the change was
/// asked for. From then on reset links go to the new address and `login` finds the
/// account by it; the reset links mailed to the address before end, and that address
/// is mailed a notice that it is the account's no more, which names the account and
/// nothing of the new address. The account's sessions go on.
///
/// A link is used once. A token that is missing, used, ended, replaced by a newer
/// mail or request, or taken back (`cancelEmailChange`), one of an account that is no
/// longer in use or whose password was set anew since, and one whose address somebody
/// else took meanwhile are all answered 400 naming `token` in `errors`, and change
/// nothing: which of them it was is not said. Requests are counted by where they come
/// from, as registrations are, and too many are 429.
#[utoipa::path(
    post,
    path = "/email-change/confirm",
    operation_id = "confirmEmailChange",
    tag = tag::AUTH,
    security(()),
    request_body = ConfirmEmailChange,
    responses(
        (status = 204, description = "The address is the account's."),
        (
            status = 429,
            description = "Too many requests from where this one comes from.",
            body = Problem,
            content_type = "application/problem+json",
            headers(("Retry-After" = u64, description = "How many seconds to wait before trying again.")),
        ),
    ),
)]
async fn confirm(
    State(app): State<App>,
    client: Option<Extension<ClientAddress>>,
    body: Bytes,
) -> ApiResult {
    let request: ConfirmEmailChange = decode_object(&body)?;
    app.confirm_email_change(&request.token, address_of(client))
        .await?;
    Ok(no_content())
}
