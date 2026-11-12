//! Board members: listing, adding, changing, removing, joining and leaving.

use axum::body::Bytes;
use axum::extract::{Path, State};
use fvoci_model::{BoardMember, BoardRole, BoardType, WorkspaceRole};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::contract::bodies::{MemberRoles, NewMember};
use super::contract::params::{BoardId, UserId};
use super::contract::tag;

use super::{ApiResult, RequiredSession, json_response, no_content};
use super::{decode_object, refusal, require_board_permission, require_granted, user_is_guest};
use crate::app::App;
use crate::error::{AppError, Reason};
use crate::permissions::{self, BoardPermission};

/// The refusal to add, change or remove a member.
const MODIFY_MEMBERS_DENIED: &str = "access denied to modify board members";

pub(super) fn routes() -> OpenApiRouter<App> {
    OpenApiRouter::new()
        .routes(routes!(handle_get_members_for_board, handle_add_member))
        .routes(routes!(handle_update_member, handle_delete_member))
        .routes(routes!(handle_join_board))
        .routes(routes!(handle_leave_board))
}

/// The membership a request asks for: the user and board the handler decided on, and
/// the role flags of the body. Whatever else the body says is ignored.
fn requested_member(board_id: String, user_id: String, body: &BoardMember) -> BoardMember {
    BoardMember {
        board_id,
        user_id,
        scheme_admin: body.scheme_admin,
        scheme_editor: body.scheme_editor,
        scheme_commenter: body.scheme_commenter,
        scheme_viewer: body.scheme_viewer,
        ..BoardMember::default()
    }
}

/// A membership with exactly the one role.
fn member_with_role(board_id: String, user_id: String, role: BoardRole) -> BoardMember {
    let member = BoardMember {
        board_id,
        user_id,
        ..BoardMember::default()
    };
    permissions::with_role(member, Some(role))
}

/// Refuses a membership whose role the user's place in the workspace does not let
/// them hold: a guest is no more than an editor (403, reason `guest_role_limit`).
fn require_holdable(workspace_role: WorkspaceRole, member: &BoardMember) -> Result<(), AppError> {
    match permissions::role_of(member) {
        Some(role) if !permissions::may_hold(workspace_role, role) => Err(AppError::forbidden(
            "a guest of the workspace is given no role above editor",
        )
        .with_reason(Reason::GuestRoleLimit)),
        _ => Ok(()),
    }
}

/// The members of a board
///
/// For the board's members and the workspace's administrators. By the identifier of
/// the user: a membership has no time of its own, so this is not the order in which the
/// users came to the board.
#[utoipa::path(
    get,
    path = "/boards/{boardId}/members",
    operation_id = "getMembers",
    tag = tag::MEMBERS,
    params(BoardId),
    responses((status = 200, description = "The members.", body = Vec<BoardMember>)),
)]
async fn handle_get_members_for_board(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    Path(board_id): Path<String>,
) -> ApiResult {
    require_board_permission(
        &app,
        &session.user_id,
        &board_id,
        BoardPermission::ListMembers,
        "access denied to board members",
    )
    .await?;

    Ok(json_response(&app.members_for_board(&board_id).await?))
}

/// Make a user a member of a board
///
/// The board's admins add members with the roles the request names; with no role at
/// all the member may do nothing on the board, not even view it. On an open board
/// editors who work in the workspace add members too, with the board's minimum role,
/// whatever else the request names; a request of theirs that asks for `schemeAdmin` is
/// 403. A guest of the workspace is given no role above editor: asking for
/// `schemeAdmin` for one is 403 with the reason `guest_role_limit`. A user who is a
/// member already stays as they are. The workspace's administrators manage the members
/// of every board of it; one who makes themselves a member of a board whose content
/// they could not read is recorded in the organization's audit log.
#[utoipa::path(
    post,
    path = "/boards/{boardId}/members",
    operation_id = "addMember",
    tag = tag::MEMBERS,
    params(BoardId),
    request_body = NewMember,
    responses((status = 200, description = "The membership as it is now.", body = BoardMember)),
)]
async fn handle_add_member(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    Path(board_id): Path<String>,
    body: Bytes,
) -> ApiResult {
    let user_id = &session.user_id;
    let access = app
        .board_access(user_id, &board_id)
        .await?
        .ok_or_else(|| AppError::not_found("board"))?;
    let (board, grant) = (access.board, access.grant);
    let manages_roles = grant.allows(BoardPermission::ManageBoardRoles);
    let edits_board = grant.allows(BoardPermission::ManageBoardProperties);
    // A guest's editing adds nobody: who is in the workspace is not a guest's to learn.
    let adds_to_open_board =
        edits_board && board.board_type == BoardType::Open && access.actor.works_there();
    if !(manages_roles || adds_to_open_board) {
        return Err(refusal(grant, MODIFY_MEMBERS_DENIED));
    }

    let request: BoardMember = decode_object(&body)?;
    if request.user_id.is_empty() {
        return Err(AppError::bad_request("empty userId").with_field("userId"));
    }
    // The original stores a membership of nobody; the user must exist.
    match user_is_guest(&app, &request.user_id).await {
        Ok(_) => {}
        Err(error) if error.is_not_found() => {
            return Err(AppError::bad_request("no such user").with_field("userId"));
        }
        // A lookup that failed is no answer to whether the user exists.
        Err(error) => return Err(error),
    }
    // A board takes the people of its workspace, its guests too. Anybody else is, to who
    // adds members, a user who does not exist: the answer is the same, so that it
    // tells nothing of who has an account elsewhere on the server.
    let Some(target_role) = app
        .actor(&request.user_id, &board.workspace_id)
        .await?
        .workspace
    else {
        return Err(AppError::bad_request("no such user").with_field("userId"));
    };

    // Not in the original, which lets an editor of an open board add members with any
    // role (`server/api/members.go:164-168`). Anyone can join an open board as an editor
    // and could then make a second account its admin, and with that take the board over.
    // The web app offers a user who does not manage roles the board's minimum role only.
    if request.scheme_admin && !manages_roles {
        return Err(AppError::forbidden(MODIFY_MEMBERS_DENIED));
    }

    // Not in the original either, which stores the roles the request names whoever
    // sends it: an editor of an open board could hand out any role below admin, or none.
    // Choosing roles belongs to those who manage them.
    let member = if manages_roles {
        requested_member(board_id, request.user_id.clone(), &request)
    } else {
        // The board's minimum role: what joining gives, and so what somebody gives who
        // has no say about roles.
        member_with_role(
            board_id,
            request.user_id.clone(),
            BoardRole::from(board.minimum_role),
        )
    };
    require_holdable(target_role, &member)?;
    // A workspace's administrator who makes themselves a member of a board they could
    // not read, with a role, and so comes to read it, is recorded doing so. The board was there a
    // moment ago; if it is not any more, there is nothing to be a member of.
    let added = if request.user_id == *user_id
        && grant.manages_only()
        && permissions::role_of(&member).is_some()
    {
        app.add_self_to_board_recorded(&member).await?
    } else {
        app.add_member_to_board(&member).await?
    };
    let added = added.ok_or_else(|| AppError::not_found("board"))?;
    Ok(json_response(&added))
}

/// Join an open board
///
/// Makes the user a member of an open board, with the board's minimum role. A member
/// below that role is raised to it; a member with that role or a higher one stays as
/// they are. A template cannot be joined, and a private board is not there for who is
/// no member of it.
#[utoipa::path(
    post,
    path = "/boards/{boardId}/join",
    operation_id = "joinBoard",
    tag = tag::MEMBERS,
    params(BoardId),
    responses((status = 200, description = "The membership as it is now.", body = BoardMember)),
)]
async fn handle_join_board(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    Path(board_id): Path<String>,
) -> ApiResult {
    let user_id = &session.user_id;
    let access = app
        .board_access(user_id, &board_id)
        .await?
        .ok_or_else(|| AppError::not_found("board"))?;
    let (board, grant) = (access.board, access.grant);

    // The original lets a workspace administrator join any board as its admin with the
    // `allow_admin` parameter; here a workspace's administrator joins as anybody does.
    if board.board_type != BoardType::Open {
        // A private board exists only for its members, who have nothing to join.
        return Err(refusal(grant, "cannot join a non Open board"));
    }
    // An open board is there for who works in its workspace and its members, and a
    // built-in template for everybody. To anybody else it answers as a board that does
    // not exist: joining is no way into a workspace.
    if !grant.sees() {
        return Err(AppError::not_found("board"));
    }
    // An open template is for everybody to copy, and for its members to change. Joining
    // made anyone an editor of it, of the built-in templates too: of the board every new
    // user's first board is copied from.
    if board.is_template {
        return Err(AppError::forbidden("a template cannot be joined"));
    }
    // A guest of the workspace was invited to the open board and has no more to join:
    // to them, as to anybody who does not work in the workspace, there is nothing to
    // join, which joining would raise to the board's minimum role.
    if !grant.allows(BoardPermission::JoinBoard) {
        return Err(AppError::not_found("board"));
    }
    if user_is_guest(&app, user_id).await? {
        return Err(AppError::forbidden("guests not allowed to join boards"));
    }

    Ok(json_response(&app.join_board(&board_id, user_id).await?))
}

/// Leave a board
///
/// Ends the user's own membership of a board. The last admin of a board cannot leave
/// it: that is 409 with the reason `last_admin`.
#[utoipa::path(
    post,
    path = "/boards/{boardId}/leave",
    operation_id = "leaveBoard",
    tag = tag::MEMBERS,
    params(BoardId),
    responses((status = 204, description = "The user is no member any more.")),
)]
async fn handle_leave_board(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    Path(board_id): Path<String>,
) -> ApiResult {
    let user_id = &session.user_id;
    require_board_permission(
        &app,
        user_id,
        &board_id,
        BoardPermission::ViewBoard,
        "access denied to board",
    )
    .await?;
    // The board may be deleted between the two lookups; then there is nothing to leave.
    app.board(&board_id).await?;

    app.delete_board_member(&board_id, user_id).await?;
    Ok(no_content())
}

/// Change the roles of a member
///
/// Replaces the roles of a member of the board; with no role at all the member may do
/// nothing on the board, not even view it. For the board's admins. The last admin of a
/// board cannot be made less: that is 409 with the reason `last_admin`. A guest of the
/// workspace is made no more than an editor: `schemeAdmin` for one is 403 with the
/// reason `guest_role_limit`. A user who is no member of the board, or of its workspace any
/// more, is 404.
#[utoipa::path(
    put,
    path = "/boards/{boardId}/members/{userId}",
    operation_id = "updateMember",
    tag = tag::MEMBERS,
    params(BoardId, UserId),
    request_body = MemberRoles,
    responses((status = 200, description = "The membership as it is now.", body = BoardMember)),
)]
async fn handle_update_member(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    Path((board_id, member_user_id)): Path<(String, String)>,
    body: Bytes,
) -> ApiResult {
    let grant = app.board_grant(&session.user_id, &board_id).await?;
    require_granted(
        grant,
        BoardPermission::ManageBoardRoles,
        MODIFY_MEMBERS_DENIED,
    )?;
    // The board may be deleted between the two lookups; then it is gone for everybody.
    let target = app
        .board_access(&member_user_id, &board_id)
        .await?
        .ok_or_else(|| AppError::not_found("board"))?;
    let request: BoardMember = decode_object(&body)?;
    let member = requested_member(board_id, member_user_id, &request);
    // Who is no member of the board is not found, and so is who has left its
    // workspace: the role left there opens nothing, and is not to be changed.
    let (Some(_), Some(workspace_role)) = (target.member, target.actor.workspace) else {
        return Err(AppError::not_found("member"));
    };
    require_holdable(workspace_role, &member)?;

    // A workspace's administrator who gives themselves a role on a board they could not
    // read is recorded doing so, as one who adds themselves.
    let updated = if member.user_id == session.user_id
        && grant.manages_only()
        && permissions::role_of(&member).is_some()
    {
        app.update_self_on_board_recorded(&member).await?
    } else {
        app.update_board_member(&member).await?
    };
    match updated {
        Some(updated) => Ok(json_response(&updated)),
        None => Err(AppError::not_found("member")),
    }
}

/// Remove a member from a board
///
/// For the board's admins. The last admin of a board cannot be removed: that is 409
/// with the reason `last_admin`. Removing a user who is no member is no error, and is
/// answered 204 like a removal.
#[utoipa::path(
    delete,
    path = "/boards/{boardId}/members/{userId}",
    operation_id = "removeMember",
    tag = tag::MEMBERS,
    params(BoardId, UserId),
    responses((status = 204, description = "The user is no member any more.")),
)]
async fn handle_delete_member(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    Path((board_id, member_user_id)): Path<(String, String)>,
) -> ApiResult {
    require_board_permission(
        &app,
        &session.user_id,
        &board_id,
        BoardPermission::ManageBoardRoles,
        MODIFY_MEMBERS_DENIED,
    )
    .await?;

    app.board(&board_id).await?;
    app.delete_board_member(&board_id, &member_user_id).await?;
    Ok(no_content())
}
