//! Boards: listing, creating, reading, patching, deleting, duplicating and undeleting.

use axum::body::Bytes;
use axum::extract::{Path, RawQuery, State};
use fvoci_model::{Board, BoardPatch, BoardType, BoardsAndBlocks, JsonMap, MinimumRole};
use serde::de::DeserializeOwned;
use serde_json::Value;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::contract::bodies::NewBoard;
use super::contract::params::{AsTemplate, BoardId, ReadToken, ToWorkspace};
use super::contract::tag;

use super::{
    ApiResult, OptionalSession, RequiredSession, by_read_token, created, decode_json,
    json_response, no_content, query_param,
};
use super::{
    duplicate_as, json_object, require_board_permission, require_workspace_access,
    require_workspace_in_body, user_is_guest,
};
use crate::app::App;
use crate::error::AppError;
use crate::permissions::{BoardPermission, WorkspacePermission, member_has_permission};

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
    path = "/boards",
    operation_id = "createBoard",
    tag = tag::BOARDS,
    request_body = NewBoard,
    responses((status = 201, description = "The board that was made.", body = Board)),
)]
async fn handle_create_board(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    body: Bytes,
) -> ApiResult {
    let user_id = &session.user_id;
    let mut requested = json_object(decode_json(&body)?)?;
    let invalid = take_invalid_board_members(&mut requested);
    let board: Board =
        serde_json::from_value(Value::Object(requested)).map_err(AppError::bad_request)?;

    let (permission, denial) = match board.board_type {
        BoardType::Open => (
            WorkspacePermission::CreatePublicChannel,
            "access denied to create public boards",
        ),
        BoardType::Private => (
            WorkspacePermission::CreatePrivateChannel,
            "access denied to create private boards",
        ),
    };
    require_workspace_in_body(&app, user_id, &board.workspace_id, "workspaceId").await?;
    if !app
        .has_permission_to_workspace(user_id, &board.workspace_id, permission)
        .await?
    {
        return Err(AppError::forbidden(denial));
    }
    if user_is_guest(&app, user_id).await? {
        return Err(AppError::forbidden("access denied to create board"));
    }

    board.validate().map_err(AppError::bad_request)?;
    if let Some(invalid) = invalid {
        return Err(invalid.into_error());
    }

    let board = app.create_board_with_admin(board, user_id).await?;
    Ok(created(&board))
}

/// A board
///
/// For a user who may see the board: a member of it, or for an open board anyone in
/// its workspace. With the board's read token no session is needed. Who reads with the token
/// and is no member of the board, without a session or signed in, is not told who made
/// the board or changed it last (`createdBy` and `modifiedBy` are empty).
#[utoipa::path(
    get,
    path = "/boards/{boardId}",
    operation_id = "getBoard",
    tag = tag::BOARDS,
    params(BoardId, ReadToken),
    security((), ("sessionCookie" = []), ("bearerToken" = [])),
    responses((status = 200, description = "The board.", body = Board)),
)]
async fn handle_get_board(
    State(app): State<App>,
    session: OptionalSession,
    Path(board_id): Path<String>,
    RawQuery(query): RawQuery,
) -> ApiResult {
    let user_id = session.user_id();
    let shared = by_read_token(&app, query.as_deref(), &board_id).await?;
    let has_valid_read_token = shared.is_some();
    if user_id.is_empty() && !has_valid_read_token {
        return Err(AppError::unauthorized("access denied to board"));
    }
    let app = shared.unwrap_or(app);

    let mut board = app.board(&board_id).await?;
    if has_valid_read_token {
        if !app.is_board_reader(user_id, &board_id).await? {
            board.hide_people();
        }
    } else if !sees_board_without_token(&app, user_id, &board).await? {
        return Err(AppError::not_found("board"));
    }
    Ok(json_response(&board))
}

/// Whether a signed-in user sees the board without its read token: an open board is
/// for every user of its workspace, a private one for its members.
async fn sees_board_without_token(
    app: &App,
    user_id: &str,
    board: &Board,
) -> Result<bool, AppError> {
    let open_to_the_workspace = board.board_type == BoardType::Open
        && !user_is_guest(app, user_id).await?
        && app
            .has_permission_to_workspace(
                user_id,
                &board.workspace_id,
                WorkspacePermission::ViewWorkspace,
            )
            .await?;
    Ok(open_to_the_workspace
        || app
            .has_permission_to_board(user_id, &board.id, BoardPermission::ViewBoard)
            .await?)
}

/// Change a board
///
/// For who may manage the board's properties. Changing `type` or `minimumRole` is for
/// the board's admins.
#[utoipa::path(
    patch,
    path = "/boards/{boardId}",
    operation_id = "patchBoard",
    tag = tag::BOARDS,
    params(BoardId),
    request_body = BoardPatch,
    responses((status = 200, description = "The board as it is now.", body = Board)),
)]
async fn handle_patch_board(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    Path(board_id): Path<String>,
    body: Bytes,
) -> ApiResult {
    let user_id = &session.user_id;
    require_board_permission(
        &app,
        user_id,
        &board_id,
        BoardPermission::ManageBoardProperties,
        "access denied to modifying board properties",
    )
    .await?;
    let patch = decode_board_patch(&body)?;
    if patch.board_type.is_some() || patch.minimum_role.is_some() {
        require_board_permission(
            &app,
            user_id,
            &board_id,
            BoardPermission::ManageBoardType,
            "access denied to modifying board type",
        )
        .await?;
    }

    // The store names what it refuses as a board has it; the patch has two of those
    // members under other names.
    let board = app
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
    path = "/boards/{boardId}/property-order",
    operation_id = "reorderCardProperties",
    tag = tag::BOARDS,
    params(BoardId),
    request_body = Vec<String>,
    responses((
        status = 200,
        description = "The identifiers of the card properties in their new order.",
        body = Vec<String>,
    )),
)]
async fn handle_reorder_card_properties(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    Path(board_id): Path<String>,
    body: Bytes,
) -> ApiResult {
    let user_id = &session.user_id;
    require_board_permission(
        &app,
        user_id,
        &board_id,
        BoardPermission::ManageBoardProperties,
        "access denied to modifying board properties",
    )
    .await?;
    let order: Vec<String> = decode_json(&body)?;
    let order = app
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
    path = "/boards/{boardId}",
    operation_id = "deleteBoard",
    tag = tag::BOARDS,
    params(BoardId),
    responses((status = 204, description = "The board is deleted.")),
)]
async fn handle_delete_board(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    Path(board_id): Path<String>,
) -> ApiResult {
    require_board_permission(
        &app,
        &session.user_id,
        &board_id,
        BoardPermission::DeleteBoard,
        "access denied to delete board",
    )
    .await?;
    // The board may be deleted between the two lookups; then it is gone for everybody.
    app.board(&board_id).await?;

    app.delete_board(&board_id, &session.user_id).await?;
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
    path = "/boards/{boardId}/duplicate",
    operation_id = "duplicateBoard",
    tag = tag::BOARDS,
    params(BoardId, AsTemplate, ToWorkspace),
    responses((
        status = 201,
        description = "The copy: one board and its blocks. The blocks are in the order of the original's, oldest first.",
        body = BoardsAndBlocks,
    )),
)]
async fn handle_duplicate_board(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    Path(board_id): Path<String>,
    RawQuery(query): RawQuery,
) -> ApiResult {
    let user_id = &session.user_id;
    let duplicate_as = duplicate_as(query.as_deref());
    let mut to_workspace = query_param(query.as_deref(), "toWorkspace");

    // Whether the board is there for the user comes first: a board that cannot be seen
    // answers as one that does not exist, whatever else the request says.
    let denial = "access denied to board";
    let board = app.board(&board_id).await?;
    if board.is_template && board.board_type == BoardType::Open {
        // An open template is for everybody who works in its workspace and for its
        // members, and a built-in one for everybody; for anybody else it is not there.
        if !board.is_builtin
            && !app.works_in_workspace(user_id, &board.workspace_id).await?
            && !app.is_board_reader(user_id, &board_id).await?
        {
            return Err(AppError::not_found("board"));
        }
    } else {
        require_board_permission(&app, user_id, &board_id, BoardPermission::ViewBoard, denial)
            .await?;
    }
    if to_workspace.is_empty() {
        // A copy stays in the board's workspace. A built-in template is in no workspace of the
        // user's organization: its copy goes to the root workspace (TEMPORARY, with
        // `only_organization_scope`: to the workspace the route will name).
        to_workspace = if board.is_builtin {
            app.root_workspace().await?.id
        } else {
            board.workspace_id.clone()
        };
    }
    // A board without a workspace, copied to no workspace, fails here too.
    require_workspace_access(&app, user_id, &to_workspace).await?;
    if user_is_guest(&app, user_id).await? {
        return Err(AppError::forbidden("access denied to create board"));
    }

    let (boards_and_blocks, _members) = app
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
/// the deletion and stay ended. For who was an admin of the board when it was deleted;
/// to anybody who was no member there is no such board.
#[utoipa::path(
    post,
    path = "/boards/{boardId}/undelete",
    operation_id = "undeleteBoard",
    tag = tag::BOARDS,
    params(BoardId),
    responses((status = 200, description = "The board as it is again.", body = Board)),
)]
async fn handle_undelete_board(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    Path(board_id): Path<String>,
) -> ApiResult {
    // The trash keeps the memberships a board had, which is how it is known who may
    // bring it back. To anybody who was no member, and for a board that is not in the
    // trash, there is no such board.
    let deleted = app
        .deleted_board(&board_id)
        .await?
        .ok_or_else(|| AppError::not_found("board"))?;
    let member = deleted
        .members
        .iter()
        .find(|member| member.user_id == session.user_id)
        .ok_or_else(|| AppError::not_found("board"))?;
    if !member_has_permission(member, BoardPermission::DeleteBoard) {
        return Err(AppError::forbidden("access denied to undelete board"));
    }

    let board = app.undelete_board(&board_id, &session.user_id).await?;
    Ok(json_response(&board))
}
