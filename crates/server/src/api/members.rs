//! Board members: listing, adding, changing, removing, joining and leaving.

use axum::Router;
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::routing::{get, post, put};
use fvoci_model::{BoardMember, BoardRole, BoardType};

use super::{ApiResult, RequiredSession, json_response, no_content};
use super::{
    board_refusal, decode_object, require_board_permission, require_team_access, user_is_guest,
};
use crate::app::App;
use crate::error::AppError;
use crate::permissions::BoardPermission;

/// The refusal to add, change or remove a member.
const MODIFY_MEMBERS_DENIED: &str = "access denied to modify board members";

pub(super) fn routes() -> Router<App> {
    Router::new()
        .route(
            "/boards/{board_id}/members",
            get(handle_get_members_for_board).post(handle_add_member),
        )
        .route(
            "/boards/{board_id}/members/{user_id}",
            put(handle_update_member).delete(handle_delete_member),
        )
        .route("/boards/{board_id}/join", post(handle_join_board))
        .route("/boards/{board_id}/leave", post(handle_leave_board))
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
    BoardMember {
        board_id,
        user_id,
        scheme_admin: role == BoardRole::Admin,
        scheme_editor: role == BoardRole::Editor,
        scheme_commenter: role == BoardRole::Commenter,
        scheme_viewer: role == BoardRole::Viewer,
        ..BoardMember::default()
    }
}

/// `GET /boards/{board_id}/members`: the members of a board, for its members.
async fn handle_get_members_for_board(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    Path(board_id): Path<String>,
) -> ApiResult {
    require_board_permission(
        &app,
        &session.user_id,
        &board_id,
        BoardPermission::ViewBoard,
        "access denied to board members",
    )
    .await?;

    Ok(json_response(&app.members_for_board(&board_id).await?))
}

/// `POST /boards/{board_id}/members`: makes a user a member of a board. Admins add
/// members to any board, with the roles the request names. On an open board editors add
/// members too, with the board's minimum role, whatever the request names.
async fn handle_add_member(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    Path(board_id): Path<String>,
    body: Bytes,
) -> ApiResult {
    let user_id = &session.user_id;
    let manages_roles = app
        .has_permission_to_board(user_id, &board_id, BoardPermission::ManageBoardRoles)
        .await?;
    let edits_board = app
        .has_permission_to_board(user_id, &board_id, BoardPermission::ManageBoardProperties)
        .await?;
    // The board may be deleted between the two lookups; then it is gone for everybody.
    let board = match app.board(&board_id).await {
        Ok(board) => board,
        Err(error) if error.is_not_found() && !manages_roles && !edits_board => {
            return Err(board_refusal(&app, user_id, &board_id, MODIFY_MEMBERS_DENIED).await);
        }
        Err(error) => return Err(error),
    };
    let adds_to_open_board = edits_board && board.board_type == BoardType::Open;
    if !(manages_roles || adds_to_open_board) {
        return Err(board_refusal(&app, user_id, &board_id, MODIFY_MEMBERS_DENIED).await);
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
    require_team_access(
        &app,
        &request.user_id,
        &board.team_id,
        "access denied to team",
    )
    .await?;

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
    // The board was there a moment ago; if it is not any more, there is nothing to be a
    // member of.
    let added = app
        .add_member_to_board(&member)
        .await?
        .ok_or_else(|| AppError::not_found("board"))?;
    Ok(json_response(&added))
}

/// `POST /boards/{board_id}/join`: makes the user a member of an open board, with the
/// board's minimum role. A member below that role is raised to it; a member with that
/// role or a higher one stays as they are.
async fn handle_join_board(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    Path(board_id): Path<String>,
) -> ApiResult {
    let user_id = &session.user_id;
    let board = app.board(&board_id).await?;

    // The original lets a team administrator join any board as its admin with the
    // `allow_admin` parameter. Nobody administers the team of a standalone server.
    if board.board_type != BoardType::Open {
        // A private board exists only for its members, who have nothing to join.
        return Err(board_refusal(&app, user_id, &board_id, "cannot join a non Open board").await);
    }
    // An open template is for everybody to copy, and for its members to change. Joining
    // made anyone an editor of it, of the built-in templates too: of the board every new
    // user's first board is copied from.
    if board.is_template {
        return Err(AppError::forbidden("a template cannot be joined"));
    }
    require_team_access(&app, user_id, &board.team_id, "access denied to team").await?;
    if user_is_guest(&app, user_id).await? {
        return Err(AppError::forbidden("guests not allowed to join boards"));
    }

    Ok(json_response(&app.join_board(&board_id, user_id).await?))
}

/// `POST /boards/{board_id}/leave`: ends the user's own membership of a board.
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

/// `PUT /boards/{board_id}/members/{user_id}`: replaces the roles of a member.
async fn handle_update_member(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    Path((board_id, member_user_id)): Path<(String, String)>,
    body: Bytes,
) -> ApiResult {
    require_board_permission(
        &app,
        &session.user_id,
        &board_id,
        BoardPermission::ManageBoardRoles,
        MODIFY_MEMBERS_DENIED,
    )
    .await?;
    // The board may be deleted between the two lookups; then it is gone for everybody.
    app.board(&board_id).await?;
    let request: BoardMember = decode_object(&body)?;
    let member = requested_member(board_id, member_user_id, &request);

    match app.update_board_member(&member).await? {
        Some(updated) => Ok(json_response(&updated)),
        None => Err(AppError::not_found("member")),
    }
}

/// `DELETE /boards/{board_id}/members/{user_id}`: removes a member from a board.
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
