//! Invitations: the links that let somebody into a workspace.
//!
//! The routes under a workspace make its links. The route that accepts one names no
//! workspace: whoever comes with a link knows its token and nothing else, and where
//! the token leads is the server's to find out. A token is sent in a body, never in
//! an address, and is answered once, by the request that makes its link.

use axum::body::Bytes;
use axum::extract::{Extension, State};
use fvoci_model::{
    AcceptInvitation, AcceptedInvitation, Invitation, InvitationPreview, InvitationToken,
    NewInvitation, WorkspaceRole,
};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::auth::address_of;
use super::contract::params::MemberWorkspaceId;
use super::contract::tag;
use super::workspace::Ws;
use super::{ApiResult, OptionalSession, created, decode_object, json_response};
use crate::app::{Accepting, App};
use crate::client_address::ClientAddress;
use crate::error::AppError;
use crate::permissions::{self, WorkspacePermission};

/// Where the account to make is in the request that accepts a link: a JSON Pointer.
const ACCOUNT: &str = "/account";

/// The routes of a workspace's links, for its members.
pub(super) fn routes() -> OpenApiRouter<App> {
    OpenApiRouter::new().routes(routes!(handle_create_invitation))
}

/// The routes of whoever comes with a link: they may have no session yet.
pub(super) fn unauthenticated_routes() -> OpenApiRouter<App> {
    OpenApiRouter::new()
        .routes(routes!(handle_preview_invitation))
        .routes(routes!(handle_accept_invitation))
}

/// Make an invitation
///
/// Makes a link that lets whoever accepts it into the workspace, and answers it with
/// its `token`: the one time the token is told, as the server keeps its digest only. A
/// link ends seven days after it was made, or when it is revoked.
///
/// With the `role` `member` the link makes members of the workspace, as often as it is
/// used. Only the workspace's administrators make one: anybody else is refused (403).
/// Whoever accepts it becomes a member of the workspace's organization too.
#[utoipa::path(
    post,
    path = "/w/{workspaceId}/invitations",
    operation_id = "createInvitation",
    tag = tag::WORKSPACES,
    params(MemberWorkspaceId),
    request_body = NewInvitation,
    responses((status = 201, description = "The invitation, with its token.", body = Invitation)),
)]
async fn handle_create_invitation(ws: Ws, body: Bytes) -> ApiResult {
    let request: NewInvitation = decode_object(&body)?;
    match request.role {
        WorkspaceRole::Member => {
            for (given, field) in [
                (request.board_id.is_some(), "boardId"),
                (request.board_role.is_some(), "boardRole"),
            ] {
                if given {
                    return Err(
                        AppError::bad_request("an invitation for members names no board")
                            .with_field(field),
                    );
                }
            }
            if !permissions::allows_in_workspace(ws.actor(), WorkspacePermission::InviteMembers) {
                return Err(AppError::forbidden(
                    "only the administrators of the workspace invite its members",
                ));
            }
            let invitation = ws
                .app
                .invite_members(&ws.workspace_id, &ws.session.user_id)
                .await?;
            Ok(created(&invitation))
        }
        WorkspaceRole::Admin | WorkspaceRole::Guest => Err(AppError::bad_request(
            "an invitation makes a member of the workspace",
        )
        .with_field("role")),
    }
}

/// Look at an invitation
///
/// What the link of `token` shows to whoever holds it, before they accept it and
/// whether they are signed in or not: the name of the workspace it leads into and the
/// name of who made it (the name they are shown by, or their login name).
///
/// A token that lets nobody in is 404 with the reason `invitation_unavailable`, as
/// `acceptInvitation` answers it and whatever is wrong with it. Requests are counted
/// by where they come from, and too many are 429.
#[utoipa::path(
    post,
    path = "/invitations/preview",
    operation_id = "previewInvitation",
    tag = tag::WORKSPACES,
    request_body = InvitationToken,
    security(()),
    responses(
        (status = 200, description = "What the link shows.", body = InvitationPreview),
        (
            status = 429,
            description = "Too many requests from where this one comes from.",
            body = fvoci_model::Problem,
            content_type = "application/problem+json",
            headers(("Retry-After" = u64, description = "How many seconds to wait before trying again.")),
        ),
    ),
)]
async fn handle_preview_invitation(
    State(app): State<App>,
    client: Option<Extension<ClientAddress>>,
    body: Bytes,
) -> ApiResult {
    let request: InvitationToken = decode_object(&body)?;
    let preview = app
        .preview_invitation(&request.token, address_of(client))
        .await?;
    Ok(json_response(&preview))
}

/// Accept an invitation
///
/// Lets somebody into the workspace the link of `token` leads into, and answers where
/// that is. A request with a session lets in the user of the session, and names no
/// `account` (400 if it does). A request without one makes the `account` it names and
/// lets that in: the account's members are checked as `register` checks them (400,
/// with `errors`, which name them under `/account`), and a name or an email address
/// that is taken is 409 with the reason `username_taken` or `email_taken`. Accepting
/// does not sign in.
///
/// A member's link makes the account a member of the workspace and of its
/// organization; who is a guest of the workspace becomes a member, and who works there
/// already stays what they are. The account is made, the membership stored and the use
/// of the link counted together or not at all.
///
/// A token that lets nobody in is 404 with the reason `invitation_unavailable`,
/// whatever is wrong with it: there is no such link, it ended, it was used as often as
/// it may be, it was revoked, or who made it may no longer invite. Requests are counted
/// by where they come from, and too many are 429.
#[utoipa::path(
    post,
    path = "/invitations/accept",
    operation_id = "acceptInvitation",
    tag = tag::WORKSPACES,
    request_body = AcceptInvitation,
    security((), ("sessionCookie" = []), ("bearerToken" = [])),
    responses(
        (status = 200, description = "Where the account was let in.", body = AcceptedInvitation),
        (
            status = 429,
            description = "Too many requests from where this one comes from, or the server has too much password work to do at the moment.",
            body = fvoci_model::Problem,
            content_type = "application/problem+json",
            headers(("Retry-After" = u64, description = "How many seconds to wait before trying again.")),
        ),
    ),
)]
async fn handle_accept_invitation(
    State(app): State<App>,
    OptionalSession(session): OptionalSession,
    client: Option<Extension<ClientAddress>>,
    body: Bytes,
) -> ApiResult {
    let request: AcceptInvitation = decode_object(&body)?;
    let account = request.account.map(fvoci_model::NewAccount::tidied);
    let accepting = match (&session, &account) {
        (Some(_), Some(_)) => {
            return Err(AppError::bad_request(
                "a request with a session lets in its user, and names no account",
            )
            .with_field("account"));
        }
        (Some(session), None) => Accepting::SignedIn {
            user_id: &session.user_id,
        },
        (None, Some(account)) => {
            account
                .validate()
                .map_err(|refused| AppError::from(refused).under(ACCOUNT))?;
            Accepting::NewAccount(account)
        }
        (None, None) => {
            return Err(AppError::unauthorized(
                "accepting an invitation takes a session or an account to make",
            ));
        }
    };
    // What is refused of the body is of the account: a name or an address that is taken.
    let accepted = app
        .accept_invitation(&request.token, accepting, address_of(client))
        .await
        .map_err(|error| error.under(ACCOUNT))?;
    Ok(json_response(&accepted))
}
