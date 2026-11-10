//! Boards: listing, creating, reading, patching, deleting, duplicating and undeleting.

use axum::Router;
use axum::body::Bytes;
use axum::extract::{Path, RawQuery, State};
use axum::routing::{get, post};
use fvoci_model::{Board, BoardPatch, BoardType, JsonMap, MinimumRole};
use fvoci_store::PublicBoards;
use serde::de::DeserializeOwned;
use serde_json::Value;

use super::{
    ApiResult, OptionalSession, RequiredSession, created, decode_json, has_valid_read_token,
    json_response, no_content, query_param,
};
use super::{
    duplicate_as, json_object, require_board_permission, require_team_access, require_team_in_body,
    user_is_guest,
};
use crate::app::App;
use crate::error::AppError;
use crate::permissions::{BoardPermission, TeamPermission, member_has_permission};

pub(super) fn routes() -> Router<App> {
    Router::new()
        .route("/teams/{team_id}/boards", get(handle_get_boards))
        .route("/boards", post(handle_create_board))
        .route(
            "/boards/{board_id}",
            get(handle_get_board)
                .patch(handle_patch_board)
                .delete(handle_delete_board),
        )
        .route("/boards/{board_id}/duplicate", post(handle_duplicate_board))
        .route("/boards/{board_id}/undelete", post(handle_undelete_board))
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

/// `GET /teams/{team_id}/boards`: the boards of a team that the user is a member of,
/// and its open boards.
async fn handle_get_boards(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    Path(team_id): Path<String>,
) -> ApiResult {
    let user_id = &session.user_id;
    require_team_access(&app, user_id, &team_id, "access denied to team").await?;

    // A guest would see only the boards they are a member of.
    let public_boards = if user_is_guest(&app, user_id).await? {
        PublicBoards::Exclude
    } else {
        PublicBoards::Include
    };
    let boards = app
        .boards_for_user_and_team(user_id, &team_id, public_boards)
        .await?;
    Ok(json_response(&boards))
}

/// `POST /boards`: creates a board with the user as its admin.
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
            TeamPermission::CreatePublicChannel,
            "access denied to create public boards",
        ),
        BoardType::Private => (
            TeamPermission::CreatePrivateChannel,
            "access denied to create private boards",
        ),
    };
    require_team_in_body(&app, &board.team_id, "teamId").await?;
    if !app.has_permission_to_team(user_id, &board.team_id, permission) {
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

/// `GET /boards/{board_id}`: a board, for a user who may see it or for anybody with the
/// board's read token.
async fn handle_get_board(
    State(app): State<App>,
    session: OptionalSession,
    Path(board_id): Path<String>,
    RawQuery(query): RawQuery,
) -> ApiResult {
    let user_id = session.user_id();
    let has_valid_read_token = has_valid_read_token(&app, query.as_deref(), &board_id).await?;
    if user_id.is_empty() && !has_valid_read_token {
        return Err(AppError::unauthorized("access denied to board"));
    }

    let board = app.board(&board_id).await?;

    if !has_valid_read_token {
        // An open board is for every user of its team; a private one for its members,
        // and for nobody else it exists.
        let sees = match board.board_type {
            BoardType::Private => false,
            BoardType::Open => {
                !user_is_guest(&app, user_id).await?
                    && app.has_permission_to_team(user_id, &board.team_id, TeamPermission::ViewTeam)
            }
        };
        if !sees
            && !app
                .has_permission_to_board(user_id, &board_id, BoardPermission::ViewBoard)
                .await?
        {
            return Err(AppError::not_found("board"));
        }
    }
    let mut board = board;
    if user_id.is_empty() {
        // Whoever reads through a read token alone is nobody the server knows. An
        // identifier tells when it was made: a user's would tell when they signed up.
        board.created_by.clear();
        board.modified_by.clear();
    }
    Ok(json_response(&board))
}

/// `PATCH /boards/{board_id}`: changes a board. What the patch touches decides which
/// permission it needs.
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

    let board = app.patch_board(&patch, &board_id, user_id).await?;
    Ok(json_response(&board))
}

/// `DELETE /boards/{board_id}`: deletes a board with its blocks.
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

/// `POST /boards/{board_id}/duplicate`: copies a board, as a board or a template
/// (`asTemplate=true`), into the board's team or another (`toTeam`).
async fn handle_duplicate_board(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    Path(board_id): Path<String>,
    RawQuery(query): RawQuery,
) -> ApiResult {
    let user_id = &session.user_id;
    let duplicate_as = duplicate_as(query.as_deref());
    let mut to_team = query_param(query.as_deref(), "toTeam");

    // Whether the board is there for the user comes first: a board that cannot be seen
    // answers as one that does not exist, whatever else the request says.
    let denial = "access denied to board";
    let board = app.board(&board_id).await?;
    if board.is_template && board.board_type == BoardType::Open {
        // An open template is for everybody in its team, and a built-in one for
        // everybody.
        if !board.is_builtin {
            require_team_access(&app, user_id, &board.team_id, denial).await?;
        }
    } else {
        require_board_permission(&app, user_id, &board_id, BoardPermission::ViewBoard, denial)
            .await?;
    }
    if to_team.is_empty() {
        to_team.clone_from(&board.team_id);
    }
    // A board without a team, copied to no team, fails here too.
    require_team_access(&app, user_id, &to_team, "access denied to team").await?;
    if user_is_guest(&app, user_id).await? {
        return Err(AppError::forbidden("access denied to create board"));
    }

    let (boards_and_blocks, _members) = app
        .duplicate_board(&board_id, user_id, &to_team, duplicate_as)
        .await?;
    Ok(created(&boards_and_blocks))
}

/// `POST /boards/{board_id}/undelete`: restores a deleted board with its blocks.
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
