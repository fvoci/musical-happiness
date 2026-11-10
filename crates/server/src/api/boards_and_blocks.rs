//! Boards and blocks created, patched and deleted together in one request.

use std::collections::HashSet;

use axum::body::Bytes;
use axum::extract::State;
use fvoci_model::{
    Block, BoardsAndBlocks, DeleteBoardsAndBlocks, PatchBoardsAndBlocks,
    generate_boards_and_blocks_ids,
};

use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::contract::bodies::NewBoardsAndBlocks;
use super::contract::tag;

use super::boards::{InvalidBoardMember, take_invalid_board_members, take_invalid_patch_members};
use super::decode_object_with;
use super::{ApiResult, RequiredSession, json_response, no_content};
use super::{decode_object, require_board_permission, require_team_in_body, user_is_guest};
use super::{objects_mut, put_patch_bodies, take_block_bodies, take_patch_bodies};
use crate::app::App;
use crate::error::AppError;
use crate::permissions::{BoardPermission, TeamPermission};

pub(super) fn routes() -> OpenApiRouter<App> {
    OpenApiRouter::new().routes(routes!(
        handle_create_boards_and_blocks,
        handle_patch_boards_and_blocks,
        handle_delete_boards_and_blocks
    ))
}

/// Make boards with their blocks
///
/// Makes boards and their blocks together, all or none, under identifiers the server
/// makes; the identifiers of the request only say which block belongs to which board
/// and to which other block. The request needs at least one board and one block; every
/// board needs an `id`, and all boards are of one team. The user becomes the admin of
/// each board, and each board that is no template is filed under the user's default
/// category.
#[utoipa::path(
    post,
    path = "/boards-and-blocks",
    operation_id = "createBoardsAndBlocks",
    tag = tag::BOARDS,
    request_body = NewBoardsAndBlocks,
    responses((
        status = 201,
        description = "The boards in the order of the request, and the blocks board by board in that order, each board's in the order of the request.",
        body = BoardsAndBlocks,
    )),
)]
async fn handle_create_boards_and_blocks(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    body: Bytes,
) -> ApiResult {
    let user_id = &session.user_id;
    let (mut request, (invalid, bodies)): (BoardsAndBlocks, _) =
        decode_object_with(&body, |body| {
            let invalid: Option<InvalidBoardMember> = objects_mut(body, "boards")
                .flatten()
                .find_map(take_invalid_board_members);
            (invalid, take_block_bodies(objects_mut(body, "blocks")))
        })?;
    // Not as in the original, which validates neither the type nor the minimum role of
    // these boards and stores whatever it is given. The model has no way to hold such a
    // board, so it is refused as `POST /boards` refuses it.
    if let Some(invalid) = invalid {
        return Err(invalid.into_error());
    }
    // The boards are new and have no files, so a body with an image is refused, without
    // looking up anything the request names.
    for (block, body) in request.blocks.iter_mut().zip(bodies?) {
        if body
            .as_ref()
            .is_some_and(|body| !body.file_ids().is_empty())
        {
            return Err(AppError::bad_request(
                "an image of the body shows no file of the board: a new board has none",
            )
            .with_field("body"));
        }
        block.body = body;
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
    if let Some((field, problem)) = problem {
        return Err(AppError::bad_request(problem).with_field(field));
    }

    let request = generate_boards_and_blocks_ids(request).map_err(AppError::bad_request)?;
    let created = app
        .create_boards_and_blocks_with_admin(request, user_id)
        .await?;
    Ok(super::created(&created))
}

/// Returns what is wrong with a block that is to be created with the boards that have
/// the given identifiers, if anything, in the original's order of checking: the member
/// of the block that is wrong, and the message.
fn new_block_problem(block: &Block, board_ids: &HashSet<&str>) -> Option<(&'static str, String)> {
    if block.block_type.is_empty() {
        Some(("type", format!("missing type for block id {}", block.id)))
    } else if !block.block_type.is_current() {
        Some((
            "type",
            fvoci_model::BlockError::type_not_current(&block.block_type).to_string(),
        ))
    } else if block.create_at < 1 {
        Some((
            "createAt",
            format!("invalid createAt for block id {}", block.id),
        ))
    } else if block.update_at < 1 {
        Some((
            "updateAt",
            format!("invalid updateAt for block id {}", block.id),
        ))
    } else if !board_ids.contains(block.board_id.as_str()) {
        Some((
            "boardId",
            format!(
                "invalid boardId {} (no board of the request has that id)",
                block.board_id
            ),
        ))
    } else {
        None
    }
}

/// Change boards and blocks together
///
/// Applies a patch to each of several boards of one team and to blocks of those
/// boards, all or none. The request names at least one board, and every block it names
/// is on one of its boards: blocks alone are not changed this way. Each board needs
/// what `patchBoard` needs, each block what `patchBlock` needs.
#[utoipa::path(
    patch,
    path = "/boards-and-blocks",
    operation_id = "patchBoardsAndBlocks",
    tag = tag::BOARDS,
    request_body = PatchBoardsAndBlocks,
    responses((
        status = 200,
        description = "The boards and the blocks as they are now, each in the order of the request.",
        body = BoardsAndBlocks,
    )),
)]
async fn handle_patch_boards_and_blocks(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    body: Bytes,
) -> ApiResult {
    let user_id = &session.user_id;
    // The patches' types and minimum roles are validated board by board below, as in the
    // original, not while decoding.
    let (mut patches, (invalid_members, changes)): (PatchBoardsAndBlocks, _) =
        decode_object_with(&body, |body| {
            let invalid: Vec<Option<InvalidBoardMember>> = objects_mut(body, "boardPatches")
                .map(|patch| patch.and_then(take_invalid_patch_members))
                .collect();
            (
                invalid,
                take_patch_bodies(objects_mut(body, "blockPatches")),
            )
        })?;
    patches.validate().map_err(AppError::bad_request)?;
    put_patch_bodies(&mut patches.block_patches, changes?)?;

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
    for (block_id, patch) in patches.block_ids.iter().zip(&patches.block_patches) {
        if let Some(body) = patch.body.new_body() {
            let block = app.block(block_id).await?;
            app.check_body_files(&block.board_id, Some(body)).await?;
        }
    }

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

/// Delete boards and blocks together
///
/// Deletes several boards of one team, all or none. The request names at least one
/// board, and every block it names must be on one of its boards; the blocks are checked
/// and nothing more, since a board takes all of its blocks along. The boards go to the
/// trash as `deleteBoard` puts them there, and `undeleteBoard` brings each back with
/// its blocks. For the admins of every board named.
#[utoipa::path(
    delete,
    path = "/boards-and-blocks",
    operation_id = "deleteBoardsAndBlocks",
    tag = tag::BOARDS,
    request_body = DeleteBoardsAndBlocks,
    responses((status = 204, description = "The boards and blocks are deleted.")),
)]
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
    Ok(no_content())
}
