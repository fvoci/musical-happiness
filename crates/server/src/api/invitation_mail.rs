//! Emailing an existing member or guest invitation without changing the link.

use axum::body::Bytes;
use fvoci_model::InvitationMail;
use utoipa_axum::{router::OpenApiRouter, routes};

use super::contract::{params::WorkerWorkspaceId, tag};
use super::workspace::Ws;
use super::{ApiResult, decode_object, no_content};
use crate::app::App;

pub(super) fn routes() -> OpenApiRouter<App> {
    OpenApiRouter::new().routes(routes!(send))
}

/// Email an invitation
///
/// Queues a separate message for each unique email address with an existing link.
/// The token must name a live invitation of this workspace, and the sender must
/// still be allowed to make that kind of invitation. The same checks are repeated
/// before each delivery. Member and one-use guest links retain their original
/// expiry and use limits. Invalid recipients reject the whole request before sending.
/// The recipient's language wins when known, otherwise the sender's language is used;
/// `language` is the fallback for a sender whose setting is auto. No SMTP means 404.
/// A request names at most `invitation_mail_recipients` addresses, repeats counted
/// (400, `too_many_recipients`). Its messages are queued all or none: 429
/// (`invitation_mail_limit`, with `Retry-After`) when the workspace would pass
/// `invitation_mail_per_hour` messages within an hour, and 503 (`mail_queue_full`) when
/// the server's invitation queue cannot hold them all. A refused request sends nothing.
#[utoipa::path(
    post, path = "/w/{workspaceId}/invitations/mail", operation_id = "emailInvitation",
    tag = tag::WORKSPACES, params(WorkerWorkspaceId), request_body = InvitationMail,
    responses((status = 204, description = "Delivery queued; the existing invitation is unchanged.")),
)]
async fn send(ws: Ws, body: Bytes) -> ApiResult {
    let ws = ws.require_no_guest()?;
    let request = decode_object(&body)?;
    ws.app
        .email_invitation(&ws.workspace_id, &ws.session.user_id, request)
        .await?;
    Ok(no_content())
}
