//! Boards: listing, creating, reading, patching, deleting, duplicating and undeleting.

use axum::body::Bytes;
use axum::extract::{Path, RawQuery};
use fvoci_model::{Board, BoardPatch, BoardType, BoardsAndBlocks, JsonMap, MinimumRole};
use serde::de::DeserializeOwned;
use serde_json::Value;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::contract::bodies::NewBoard;
use super::contract::params::MemberWorkspaceId;
use super::contract::params::{AsTemplate, BoardId, ToWorkspace};
use super::contract::tag;
use super::workspace::Ws;

use super::{ApiResult, created, decode_json, json_response, no_content, query_param};
use super::{
    duplicate_as, json_object, require_board_permission, require_granted, require_workspace_access,
    user_is_guest,
};
use crate::app::{App, as_managed};
use crate::error::AppError;
use crate::permissions::{BoardPermission, Grant};

pub(super) fn routes() -> OpenApiRouter<App> {
    OpenApiRouter::new()
        .routes(routes!(handle_create_board))
        .routes(routes!(
            handle_get_board,
            handle_patch_board,
            handle_delete_board
        ))
        .routes(routes!(handle_duplicate_board))
        .routes(routes!(handle_undelete_board))
        .routes(routes!(handle_reorder_card_properties))
}

// -- what the handlers of this area share ----------------------------------------------

/// A member of a board or board patch whose value is outside its closed set.
///
/// The model decodes the board type and the minimum role into enums, so a value outside
/// them fails the decoding. The original decodes any string and rejects it later, after
/// other checks and with messages of its own. To answer as the original does, the
/// handlers take such members out of the JSON before decoding it and report them where
/// the original validates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum InvalidBoardMember {
    /// `type` is neither `O` nor `P`.
    Type,
    /// `minimumRole` is not a board role.
    MinimumRole,
}

impl InvalidBoardMember {
    /// The bad request the original answers with.
    pub(super) fn into_error(self) -> AppError {
        AppError::bad_request(match self {
            Self::Type => "invalid-board-type",
            Self::MinimumRole => "invalid-board-minimum-role",
        })
    }
}

/// Removes the `type` and `minimumRole` members of a JSON board patch that are strings
/// outside their closed sets. Returns the first one in the original's order of checking:
/// the type, then the minimum role.
pub(super) fn take_invalid_patch_members(patch: &mut JsonMap) -> Option<InvalidBoardMember> {
    fn is_string_outside<T: DeserializeOwned>(value: Option<&Value>) -> bool {
        matches!(value, Some(Value::String(_)))
            && value.is_some_and(|value| T::deserialize(value).is_err())
    }

    let mut invalid = None;
    // In reverse order, so that the first in the original's order wins.
    if is_string_outside::<MinimumRole>(patch.get("minimumRole")) {
        patch.remove("minimumRole");
        invalid = Some(InvalidBoardMember::MinimumRole);
    }
    if is_string_outside::<BoardType>(patch.get("type")) {
        patch.remove("type");
        invalid = Some(InvalidBoardMember::Type);
    }
    invalid
}

/// Does for a JSON board what [`take_invalid_patch_members`] does for a patch. A board
/// must have a type, so a board without one is reported too, and gets the private type
/// to stand in: the original treats every board that is not open as private until it
/// validates the type.
pub(super) fn take_invalid_board_members(board: &mut JsonMap) -> Option<InvalidBoardMember> {
    let mut invalid = take_invalid_patch_members(board);
    if matches!(board.get("type"), None | Some(Value::Null)) {
        board.insert("type".to_owned(), Value::from(BoardType::Private.as_str()));
        invalid = Some(InvalidBoardMember::Type);
    }
    invalid
}

/// Decodes the body of a board patch. A type or minimum role outside its closed set is a
/// bad request with the original's message.
fn decode_board_patch(body: &[u8]) -> Result<BoardPatch, AppError> {
    let mut patch = json_object(decode_json(body)?)?;
    if let Some(invalid) = take_invalid_patch_members(&mut patch) {
        return Err(invalid.into_error());
    }
    serde_json::from_value(Value::Object(patch)).map_err(AppError::bad_request)
}

// -- handlers -------------------------------------------------------------------------

/// Make a board
///
/// The user becomes the board's admin, and the board is filed under the user's default
/// category, unless it is a template.
#[utoipa::path(
    post,
    path = "/w/{workspaceId}/boards",
    params(MemberWorkspaceId),
    operation_id = "createBoard",
    tag = tag::BOARDS,
    request_body = NewBoard,
    responses((status = 201, description = "The board that was made.", body = Board)),
)]
async fn handle_create_board(ws: Ws, body: Bytes) -> ApiResult {
    let user_id = &ws.session.user_id;
    let mut requested = json_object(decode_json(&body)?)?;
    let invalid = take_invalid_board_members(&mut requested);
    let mut board: Board =
        serde_json::from_value(Value::Object(requested)).map_err(AppError::bad_request)?;

    // Whoever works in the workspace makes boards in it, open ones and private ones.
    ws.require_board_maker("access denied to create board")
        .await?;
    ws.place_board(&mut board, "workspaceId")?;

    board.validate().map_err(AppError::bad_request)?;
    if let Some(invalid) = invalid {
        return Err(invalid.into_error());
    }

    let board = ws.app.create_board_with_admin(board, user_id).await?;
    Ok(created(&board))
}

/// A board
///
/// For a user who may see the board: a member of it, or for an open board anyone in
/// its workspace.
#[utoipa::path(
    get,
    path = "/w/{workspaceId}/boards/{boardId}",
    operation_id = "getBoard",
    tag = tag::BOARDS,
    params(MemberWorkspaceId, BoardId),
    responses((status = 200, description = "The board.", body = Board)),
)]
async fn handle_get_board(ws: Ws, Path((_, board_id)): Path<(String, String)>) -> ApiResult {
    // An open board is for whoever works in its workspace, a private one for its
    // members, and its name for the workspace's administrators.
    let access = ws
        .app
        .board_access(&ws.session.user_id, &board_id)
        .await?
        .filter(|access| access.grant.sees())
        .ok_or_else(|| AppError::not_found("board"))?;
    let board = if access.grant.manages_only() {
        as_managed(access.board)
    } else {
        access.board
    };
    Ok(json_response(&board))
}

/// Change a board
///
/// For who may manage the board's properties. Changing `type` or `minimumRole` is for
/// the board's admins.
#[utoipa::path(
    patch,
    path = "/w/{workspaceId}/boards/{boardId}",
    operation_id = "patchBoard",
    tag = tag::BOARDS,
    params(MemberWorkspaceId, BoardId),
    request_body = BoardPatch,
    responses((status = 200, description = "The board as it is now.", body = Board)),
)]
async fn handle_patch_board(
    ws: Ws,
    Path((_, board_id)): Path<(String, String)>,
    body: Bytes,
) -> ApiResult {
    let user_id = &ws.session.user_id;
    require_board_permission(
        &ws.app,
        user_id,
        &board_id,
        BoardPermission::ManageBoardProperties,
        "access denied to modifying board properties",
    )
    .await?;
    let patch = decode_board_patch(&body)?;
    if patch.board_type.is_some() || patch.minimum_role.is_some() {
        require_board_permission(
            &ws.app,
            user_id,
            &board_id,
            BoardPermission::ManageBoardType,
            "access denied to modifying board type",
        )
        .await?;
    }

    // The store names what it refuses as a board has it; the patch has two of those
    // members under other names.
    let board = ws
        .app
        .patch_board(&patch, &board_id, user_id)
        .await
        .map_err(|error| {
            error
                .moved("/properties", "/updatedProperties")
                .moved("/cardProperties", "/updatedCardProperties")
        })?;
    Ok(json_response(&board))
}

/// Put the card properties of a board in an order
///
/// The body names every card property of the board (`cardProperties`, by `id`) once,
/// in the new order, as `reorderCategories` names categories; one left out, one named
/// twice or one the board does not have is 400, and nothing changes. For who may manage
/// the board's properties, as `patchBoard`. The board's listeners get `UPDATE_BOARD`, as
/// for any change of the board.
#[utoipa::path(
    put,
    path = "/w/{workspaceId}/boards/{boardId}/property-order",
    operation_id = "reorderCardProperties",
    tag = tag::BOARDS,
    params(MemberWorkspaceId, BoardId),
    request_body = Vec<String>,
    responses((
        status = 200,
        description = "The identifiers of the card properties in their new order.",
        body = Vec<String>,
    )),
)]
async fn handle_reorder_card_properties(
    ws: Ws,
    Path((_, board_id)): Path<(String, String)>,
    body: Bytes,
) -> ApiResult {
    let user_id = &ws.session.user_id;
    require_board_permission(
        &ws.app,
        user_id,
        &board_id,
        BoardPermission::ManageBoardProperties,
        "access denied to modifying board properties",
    )
    .await?;
    let order: Vec<String> = decode_json(&body)?;
    let order = ws
        .app
        .reorder_card_properties(&board_id, &order, user_id)
        .await?;
    Ok(json_response(&order))
}

/// Delete a board
///
/// The board goes to the trash with its blocks and its members, and `undeleteBoard`
/// brings it back until the trash is emptied of it. Its sharing ends, with its read
/// token, and so do its places in the users' sidebars and the subscriptions to its
/// blocks; those do not come back with the board. For the board's admins.
#[utoipa::path(
    delete,
    path = "/w/{workspaceId}/boards/{boardId}",
    operation_id = "deleteBoard",
    tag = tag::BOARDS,
    params(MemberWorkspaceId, BoardId),
    responses((status = 204, description = "The board is deleted.")),
)]
async fn handle_delete_board(ws: Ws, Path((_, board_id)): Path<(String, String)>) -> ApiResult {
    require_board_permission(
        &ws.app,
        &ws.session.user_id,
        &board_id,
        BoardPermission::DeleteBoard,
        "access denied to delete board",
    )
    .await?;
    // The board may be deleted between the two lookups; then it is gone for everybody.
    ws.app.board(&board_id).await?;

    ws.app.delete_board(&board_id, &ws.session.user_id).await?;
    Ok(no_content())
}

/// Copy a board
///
/// Copies a board with its blocks, except comments, as a board or as a template. The
/// user becomes the admin of the copy, which is always private. A copy of the same kind
/// has the title of the original with ` copy` after it; a template made from a board is
/// titled `New board template`, and a board made from a template keeps the title. The
/// board's members may copy it; an open template is for everyone in its workspace, and a
/// built-in template for everyone.
#[utoipa::path(
    post,
    path = "/w/{workspaceId}/boards/{boardId}/duplicate",
    operation_id = "duplicateBoard",
    tag = tag::BOARDS,
    params(MemberWorkspaceId, BoardId, AsTemplate, ToWorkspace),
    responses((
        status = 201,
        description = "The copy: one board and its blocks. The blocks are in the order of the original's, oldest first.",
        body = BoardsAndBlocks,
    )),
)]
async fn handle_duplicate_board(
    ws: Ws,
    Path((_, board_id)): Path<(String, String)>,
    RawQuery(query): RawQuery,
) -> ApiResult {
    let user_id = &ws.session.user_id;
    let duplicate_as = duplicate_as(query.as_deref());
    let mut to_workspace = query_param(query.as_deref(), "toWorkspace");

    // Whether the board is there for the user comes first: a board that cannot be seen
    // answers as one that does not exist, whatever else the request says.
    // Copying is reading: an open template is for everybody who works in its workspace
    // and for its members, a built-in one for everybody, any other board for its members.
    let access = ws
        .app
        .board_access(user_id, &board_id)
        .await?
        .ok_or_else(|| AppError::not_found("board"))?;
    require_granted(
        access.grant,
        BoardPermission::ViewBoard,
        "access denied to board",
    )?;
    if to_workspace.is_empty() {
        // A copy stays in the workspace of the path: the board's, or for a built-in
        // template, which is every workspace's, the one it is copied from.
        to_workspace.clone_from(&ws.workspace_id);
    }
    require_workspace_access(&ws.app, user_id, &to_workspace).await?;
    if user_is_guest(&ws.app, user_id).await? {
        return Err(AppError::forbidden("access denied to create board"));
    }

    let (boards_and_blocks, _members) = ws
        .app
        .duplicate_board(&board_id, user_id, &to_workspace, duplicate_as)
        .await?;
    Ok(created(&boards_and_blocks))
}

/// Bring a deleted board back
///
/// Restores a board from the trash with its blocks and its members. Each user who had
/// filed it, and can see it again, finds it where it was in the sidebar (the same
/// category and place, hidden or not) or in the default category if that one was
/// deleted since, and is told with `UPDATE_BOARD_CATEGORY` and the category's
/// `REORDER_CATEGORY_BOARDS`. Its sharing and the subscriptions to its blocks ended with
/// the deletion and stay ended. For who was an admin of the board when it was deleted,
/// and for the workspace's administrators (who, if they were no member, get it without
/// its description and properties); to anybody else there is no such board.
#[utoipa::path(
    post,
    path = "/w/{workspaceId}/boards/{boardId}/undelete",
    operation_id = "undeleteBoard",
    tag = tag::BOARDS,
    params(MemberWorkspaceId, BoardId),
    responses((status = 200, description = "The board as it is again.", body = Board)),
)]
async fn handle_undelete_board(ws: Ws, Path((_, board_id)): Path<(String, String)>) -> ApiResult {
    // The trash keeps the memberships a board had, which is how it is known who may
    // bring it back, as what they are in its workspace now. To anybody else, and for a
    // board that is not in the trash, there is no such board.
    let grant = ws
        .app
        .deleted_board_access(&ws.session.user_id, &board_id)
        .await?
        .map_or(Grant::NONE, |access| access.grant);
    require_granted(
        grant,
        BoardPermission::DeleteBoard,
        "access denied to undelete board",
    )?;

    let board = ws
        .app
        .undelete_board(&board_id, &ws.session.user_id)
        .await?;
    if grant.manages_only() {
        return Ok(json_response(&as_managed(board)));
    }
    Ok(json_response(&board))
}
