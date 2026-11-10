//! Boards and blocks created, patched and deleted together in one request.

use std::collections::HashSet;

use axum::Router;
use axum::body::Bytes;
use axum::extract::State;
use axum::routing::post;
use fvoci_model::{
    Block, BoardsAndBlocks, DeleteBoardsAndBlocks, JsonMap, PatchBoardsAndBlocks,
    generate_boards_and_blocks_ids,
};
use serde_json::Value;

use super::boards::{InvalidBoardMember, take_invalid_board_members, take_invalid_patch_members};
use super::decode_object_with;
use super::{ApiResult, RequiredSession, empty_response, json_response};
use super::{decode_object, require_board_permission, require_team_in_body, user_is_guest};
use crate::app::App;
use crate::error::AppError;
use crate::permissions::{BoardPermission, TeamPermission};

pub(super) fn routes() -> Router<App> {
    Router::new().route(
        "/boards-and-blocks",
        post(handle_create_boards_and_blocks)
            .patch(handle_patch_boards_and_blocks)
            .delete(handle_delete_boards_and_blocks),
    )
}

/// The JSON objects in the list that is the member `name` of `object`. An element of the
/// list that is not an object is `None`.
fn objects_mut<'v>(
    object: &'v mut JsonMap,
    name: &str,
) -> impl Iterator<Item = Option<&'v mut JsonMap>> {
    object
        .get_mut(name)
        .and_then(Value::as_array_mut)
        .into_iter()
        .flatten()
        .map(Value::as_object_mut)
}

/// `POST /boards-and-blocks`: creates boards with their blocks, under new identifiers,
/// with the user as admin of each board.
async fn handle_create_boards_and_blocks(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    body: Bytes,
) -> ApiResult {
    let user_id = &session.user_id;
    let (request, invalid): (BoardsAndBlocks, Option<InvalidBoardMember>) =
        decode_object_with(&body, |body| {
            objects_mut(body, "boards")
                .flatten()
                .find_map(take_invalid_board_members)
        })?;
    // Not as in the original, which validates neither the type nor the minimum role of
    // these boards and stores whatever it is given. The model has no way to hold such a
    // board, so it is refused as `POST /boards` refuses it.
    if let Some(invalid) = invalid {
        return Err(invalid.into_error());
    }

    if request.boards.is_empty() {
        return Err(AppError::bad_request("at least one board is required"));
    }

    let team_id = request.boards[0].team_id.as_str();
    let mut board_ids = HashSet::new();
    for board in &request.boards {
        board_ids.insert(board.id.as_str());
        if team_id != board.team_id {
            return Err(AppError::bad_request(
                "cannot create boards for multiple teams",
            ));
        }
        if board.id.is_empty() {
            return Err(AppError::bad_request(
                "boards need an ID to be referenced from the blocks",
            ));
        }
    }

    require_team_in_body(&app, team_id, "boards").await?;
    if !app.has_permission_to_team(user_id, team_id, TeamPermission::ViewTeam) {
        return Err(AppError::forbidden("access denied to board template"));
    }
    if user_is_guest(&app, user_id).await? {
        return Err(AppError::forbidden("access denied to create board"));
    }

    let problem = request
        .blocks
        .iter()
        .find_map(|block| new_block_problem(block, &board_ids));
    if let Some(problem) = problem {
        return Err(AppError::bad_request(problem));
    }

    let request = generate_boards_and_blocks_ids(request).map_err(AppError::bad_request)?;
    let created = app
        .create_boards_and_blocks_with_admin(request, user_id)
        .await?;
    Ok(json_response(&created))
}

/// Returns what is wrong with a block that is to be created with the boards that have
/// the given identifiers, if anything, in the original's order of checking.
fn new_block_problem(block: &Block, board_ids: &HashSet<&str>) -> Option<String> {
    if block.block_type.is_empty() {
        Some(format!("missing type for block id {}", block.id))
    } else if block.create_at < 1 {
        Some(format!("invalid createAt for block id {}", block.id))
    } else if block.update_at < 1 {
        Some(format!("invalid UpdateAt for block id {}", block.id))
    } else if !board_ids.contains(block.board_id.as_str()) {
        Some(format!(
            "invalid BoardID {} (not exists in the created boards)",
            block.board_id
        ))
    } else {
        None
    }
}

/// `PATCH /boards-and-blocks`: patches boards and blocks, all or none.
async fn handle_patch_boards_and_blocks(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    body: Bytes,
) -> ApiResult {
    let user_id = &session.user_id;
    // The patches' types and minimum roles are validated board by board below, as in the
    // original, not while decoding.
    let (patches, invalid_members): (PatchBoardsAndBlocks, Vec<Option<InvalidBoardMember>>) =
        decode_object_with(&body, |body| {
            objects_mut(body, "boardPatches")
                .map(|patch| patch.and_then(take_invalid_patch_members))
                .collect()
        })?;
    patches.validate().map_err(AppError::bad_request)?;

    let mut team_id = String::new();
    let mut board_ids = HashSet::new();
    for (index, (board_id, patch)) in patches
        .board_ids
        .iter()
        .zip(&patches.board_patches)
        .enumerate()
    {
        board_ids.insert(board_id.as_str());

        if let Some(invalid) = invalid_members.get(index).copied().flatten() {
            return Err(invalid.into_error());
        }
        require_board_permission(
            &app,
            user_id,
            board_id,
            BoardPermission::ManageBoardProperties,
            "access denied to modifying board properties",
        )
        .await?;
        if patch.board_type.is_some() || patch.minimum_role.is_some() {
            require_board_permission(
                &app,
                user_id,
                board_id,
                BoardPermission::ManageBoardType,
                "access denied to modifying board type",
            )
            .await?;
        }

        let board = app.board(board_id).await?;
        if team_id.is_empty() {
            team_id.clone_from(&board.team_id);
        }
        if team_id != board.team_id {
            return Err(AppError::bad_request("mismatched team ID"));
        }
    }

    require_blocks_of_boards(&app, user_id, &patches.block_ids, &board_ids).await?;

    let result = app.patch_boards_and_blocks(&patches, user_id).await?;
    Ok(json_response(&result))
}

/// The one answer for a block that does not exist and for one of a board the request
/// does not list.
fn not_a_block_of_the_boards() -> AppError {
    AppError::bad_request("the request names a block that is not on its boards")
}

async fn require_blocks_of_boards(
    app: &App,
    user_id: &str,
    block_ids: &[String],
    board_ids: &HashSet<&str>,
) -> Result<(), AppError> {
    for block_id in block_ids {
        // One answer for a block that does not exist and for one on a board the request
        // does not list, so that nothing is said about blocks of other boards.
        let block = match app.block(block_id).await {
            Ok(block) if board_ids.contains(block.board_id.as_str()) => block,
            Ok(_) => return Err(not_a_block_of_the_boards()),
            Err(error) if error.is_not_found() => {
                return Err(not_a_block_of_the_boards());
            }
            Err(error) => return Err(error),
        };
        require_board_permission(
            app,
            user_id,
            &block.board_id,
            BoardPermission::ManageBoardCards,
            "access denied to modifying cards",
        )
        .await?;
    }
    Ok(())
}

/// `DELETE /boards-and-blocks`: deletes boards and blocks, all or none.
async fn handle_delete_boards_and_blocks(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    body: Bytes,
) -> ApiResult {
    let user_id = &session.user_id;
    let request: DeleteBoardsAndBlocks = decode_object(&body)?;
    request.validate().map_err(AppError::bad_request)?;

    let mut team_id = String::new();
    let mut board_ids = HashSet::new();
    for board_id in &request.boards {
        board_ids.insert(board_id.as_str());
        require_board_permission(
            &app,
            user_id,
            board_id,
            BoardPermission::DeleteBoard,
            "access denied to delete board",
        )
        .await?;
        let board = app.board(board_id).await?;
        if team_id.is_empty() {
            team_id.clone_from(&board.team_id);
        }
        if team_id != board.team_id {
            return Err(AppError::bad_request(
                "all boards should belong to the same team",
            ));
        }
    }

    require_blocks_of_boards(&app, user_id, &request.blocks, &board_ids).await?;

    app.delete_boards_and_blocks(&request, user_id).await?;
    Ok(empty_response())
}
