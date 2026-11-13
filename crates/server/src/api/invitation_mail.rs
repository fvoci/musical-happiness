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
/// before each delivery. A member link is sent as it is, its expiry and uses unchanged.
/// A guest link lets one person in once, so it is not sent: each recipient gets a
/// one-use guest link of their own, to the same board in the same role and ending
/// with it, made by the sender as their message leaves; the named link is left as it
/// is. Invalid recipients reject the whole request before sending.
/// Each message is in the recipient's language when the mail is written in it, else the
/// sender's setting, else `language`, else English. No SMTP means 404.
/// A request names at most `invitation_mail_recipients` addresses, repeats counted
/// (400, `too_many_recipients`). Its messages are queued all or none: 429
/// (`invitation_mail_limit`, with `Retry-After`) when the workspace would pass
/// `invitation_mail_per_hour` messages within an hour or its organization its own
/// hourly limit (`invitation_mail_per_org_hour`), and 503 (`mail_queue_full`) when the
/// organization's half of the server's invitation queue, or the queue, cannot hold them
/// all. A refused request sends nothing.
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
