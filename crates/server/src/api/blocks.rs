//! Blocks: reading, inserting, patching, deleting, undeleting and duplicating.

use axum::body::Bytes;
use axum::extract::{Path, RawQuery, State};
use fvoci_model::{
    Block, BlockPatch, BlockPatchBatch, BlockType, Board, BoardType, generate_block_ids,
    stamp_modification_metadata,
};

use serde_json::Value;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::contract::bodies::NewBlock;
use super::contract::params::{
    AllBlocks, AsTemplate, BlockId, BlockTypeFilter, BoardId, DisableNotify, OneBlockId, ParentId,
    ReadToken, SourceBoardId,
};
use super::contract::tag;

use super::{
    ApiResult, OptionalSession, RequiredSession, created, decode_json, has_valid_read_token,
    json_response, no_content, query_param,
};
use super::{change_notification, duplicate_as, require_board_permission, user_is_guest};
use super::{
    decode_object_with, objects_mut, put_patch_bodies, take_block_bodies, take_patch_bodies,
};
use crate::app::App;
use crate::auth::now_millis;
use crate::error::{AppError, ErrorKind};
use crate::permissions::{BoardPermission, TeamPermission};

pub(super) fn routes() -> OpenApiRouter<App> {
    OpenApiRouter::new()
        .routes(routes!(
            handle_get_blocks,
            handle_post_blocks,
            handle_patch_blocks
        ))
        .routes(routes!(handle_delete_block, handle_patch_block))
        .routes(routes!(handle_undelete_block))
        .routes(routes!(handle_duplicate_block))
}

/// Returns a query parameter, or `None` if it is absent or empty.
fn optional_query_param(query: Option<&str>, name: &str) -> Option<String> {
    Some(query_param(query, name)).filter(|value| !value.is_empty())
}

/// The refusal to change what is on a board.
const CHANGES_DENIED: &str = "access denied to make board changes";

/// Returns the block if it belongs to the board. A block of another board is as missing
/// as one that does not exist: the path names the board the request is about.
async fn block_on_board(app: &App, block_id: &str, board_id: &str) -> Result<Block, AppError> {
    let block = app.block(block_id).await?;
    if block.board_id == board_id {
        Ok(block)
    } else {
        Err(AppError::not_found("block"))
    }
}

/// Checks that a signed-in user may read the blocks of a board: the members of the
/// board, and for an open template everyone in its team.
async fn require_read_access(app: &App, user_id: &str, board: &Board) -> Result<(), AppError> {
    if board.is_template && board.board_type == BoardType::Open {
        if !board.is_builtin
            && !app.has_permission_to_team(user_id, &board.team_id, TeamPermission::ViewTeam)
        {
            return Err(AppError::forbidden("access denied to board template"));
        }
    } else {
        require_board_permission(
            app,
            user_id,
            &board.id,
            BoardPermission::ViewBoard,
            "access denied to board",
        )
        .await?;
    }
    if board.is_template && user_is_guest(app, user_id).await? {
        return Err(AppError::forbidden(
            "guest are not allowed to get board templates",
        ));
    }
    Ok(())
}

/// Checks that the user may read the board that inserted blocks copy their files from.
///
/// Security fix: the original copies the files without looking at the source board
/// (`server/api/blocks.go:297-303`), so a user could make the server copy files out of a
/// board they cannot see. Whoever may read the board's blocks may copy from it.
async fn require_source_board_access(
    app: &App,
    user_id: &str,
    source_board_id: &str,
) -> Result<(), AppError> {
    // The same answer for a board that is not there and one the user may not read. A
    // store that failed is neither: it is answered as what it is.
    let refused = |error: AppError| match error.kind() {
        ErrorKind::NotFound | ErrorKind::Forbidden | ErrorKind::Unauthorized => {
            AppError::not_found("source board")
        }
        _ => error,
    };
    let board = app.board(source_board_id).await.map_err(refused)?;
    require_read_access(app, user_id, &board)
        .await
        .map_err(refused)
}

/// The blocks of a board
///
/// For the board's members, and for an open template everyone in its team. With the
/// board's read token no session is needed; who reads that way is not told who made a
/// block or changed it last (`createdBy` and `modifiedBy` are empty).
///
/// `all` asks for every block and `blockId` for one, which is 404 if the board has no
/// such block. Otherwise `parentId` and `type` narrow the list, and without either the
/// list is every block of the board. A parameter with an empty value counts as left
/// out. Oldest first; the blocks that one request made come in the order that request
/// had them in.
#[utoipa::path(
    get,
    path = "/boards/{boardId}/blocks",
    operation_id = "getBlocks",
    tag = tag::BLOCKS,
    params(BoardId, AllBlocks, OneBlockId, ParentId, BlockTypeFilter, ReadToken),
    security((), ("sessionCookie" = []), ("bearerToken" = [])),
    responses((status = 200, description = "The blocks.", body = Vec<Block>)),
)]
async fn handle_get_blocks(
    State(app): State<App>,
    session: OptionalSession,
    Path(board_id): Path<String>,
    RawQuery(query): RawQuery,
) -> ApiResult {
    let query = query.as_deref();
    let user_id = session.user_id();

    let has_read_token = has_valid_read_token(&app, query, &board_id).await?;
    if user_id.is_empty() && !has_read_token {
        return Err(AppError::unauthorized("access denied to board"));
    }
    let board = app.board(&board_id).await?;
    if !has_read_token {
        require_read_access(&app, user_id, &board).await?;
    }

    let blocks = if !query_param(query, "all").is_empty() {
        app.blocks_for_board(&board_id).await?
    } else if let Some(block_id) = optional_query_param(query, "blockId") {
        vec![block_on_board(&app, &block_id, &board_id).await?]
    } else {
        let parent_id = optional_query_param(query, "parentId");
        // The type is matched as it is written: one that no block has finds nothing. The
        // word `unknown` is the exception, and narrows nothing.
        let block_type = optional_query_param(query, "type").map(BlockType::from);
        app.blocks(&board_id, parent_id.as_deref(), block_type.as_ref())
            .await?
    };
    tracing::debug!(board_id, block_count = blocks.len(), "GetBlocks");
    let mut blocks = blocks;
    if user_id.is_empty() {
        // Whoever reads through a read token alone is nobody the server knows. An
        // identifier tells when it was made: a user's would tell when they signed up.
        for block in &mut blocks {
            block.created_by.clear();
            block.modified_by.clear();
        }
    }
    Ok(json_response(&blocks))
}

/// Insert blocks
///
/// Inserts blocks under identifiers the server makes; the identifiers of the request
/// only link its blocks to each other. All blocks are inserted or none, and they are
/// listed afterwards in the order the request had them in. Comments need the role of a
/// commenter, everything else that of an editor.
#[utoipa::path(
    post,
    path = "/boards/{boardId}/blocks",
    operation_id = "createBlocks",
    tag = tag::BLOCKS,
    params(BoardId, SourceBoardId, DisableNotify),
    request_body = Vec<NewBlock>,
    responses((
        status = 201,
        description = "The blocks as they were stored, in the order of the request.",
        body = Vec<Block>,
    )),
)]
async fn handle_post_blocks(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    Path(board_id): Path<String>,
    RawQuery(query): RawQuery,
    body: Bytes,
) -> ApiResult {
    let query = query.as_deref();
    let user_id = session.user_id.as_str();
    let notification = change_notification(query);
    // Which permission is needed depends on the blocks, so the body is read first; but
    // whoever may not even see the board learns nothing from what the body is like.
    require_board_permission(
        &app,
        user_id,
        &board_id,
        BoardPermission::ViewBoard,
        CHANGES_DENIED,
    )
    .await?;
    // A list first, so that a request that is none is refused as before, and the bodies
    // apart, so that one that may not be stored is refused as a body.
    let mut blocks: Vec<Value> = decode_json(&body)?;
    let bodies = take_block_bodies(blocks.iter_mut().map(Value::as_object_mut))?;
    let mut blocks: Vec<Block> =
        serde_json::from_value(Value::Array(blocks)).map_err(AppError::bad_request)?;
    for (block, body) in blocks.iter_mut().zip(bodies) {
        block.body = body;
    }

    let mut has_comments = false;
    let mut has_contents = false;
    for block in &blocks {
        let id = &block.id;
        if block.block_type.is_empty() {
            return Err(
                AppError::bad_request(format_args!("missing type for block id {id}"))
                    .with_field("type"),
            );
        }
        if block.block_type == BlockType::Comment {
            has_comments = true;
        } else {
            has_contents = true;
        }
        if block.create_at < 1 {
            return Err(
                AppError::bad_request(format_args!("invalid createAt for block id {id}"))
                    .with_field("createAt"),
            );
        }
        if block.update_at < 1 {
            return Err(
                AppError::bad_request(format_args!("invalid updateAt for block id {id}"))
                    .with_field("updateAt"),
            );
        }
        if block.board_id != board_id {
            return Err(
                AppError::bad_request(format_args!("invalid boardId for block id {id}"))
                    .with_field("boardId"),
            );
        }
    }
    if has_contents {
        require_board_permission(
            &app,
            user_id,
            &board_id,
            BoardPermission::ManageBoardCards,
            CHANGES_DENIED,
        )
        .await?;
    }
    if has_comments {
        require_board_permission(
            &app,
            user_id,
            &board_id,
            BoardPermission::CommentBoardCards,
            "access denied to post card comments",
        )
        .await?;
    }

    generate_block_ids(&mut blocks);
    stamp_modification_metadata(user_id, &mut blocks, now_millis());

    // Set when a template is made from a board or a board from a template: the files
    // the blocks refer to are then copied over from that board.
    let source_board_id = query_param(query, "sourceBoardId");
    // The images of a body show files of the board, or of the board they are copied
    // from.
    let files_board_id = if source_board_id.is_empty() {
        &board_id
    } else {
        require_source_board_access(&app, user_id, &source_board_id).await?;
        &source_board_id
    };
    for block in &blocks {
        app.check_body_files(files_board_id, block.body.as_ref())
            .await?;
    }
    if !source_board_id.is_empty() {
        // The blocks are not stored yet: each is pointed at its copy here, and stored
        // with that below.
        app.copy_card_files_into(&source_board_id, &mut blocks)
            .await;
    }

    app.insert_blocks(&mut blocks, user_id, notification)
        .await?;
    tracing::debug!(block_count = blocks.len(), "POST Blocks");
    Ok(created(&blocks))
}

/// Delete a block
///
/// The block goes to the trash with the blocks below it, and `undeleteBlock` brings it
/// back until the trash is emptied of it. For editors, whatever the block is: a
/// commenter cannot delete their own comment this way. A comment of somebody else is
/// deleted by the board's admins only.
#[utoipa::path(
    delete,
    path = "/boards/{boardId}/blocks/{blockId}",
    operation_id = "deleteBlock",
    tag = tag::BLOCKS,
    params(BoardId, BlockId, DisableNotify),
    responses((status = 204, description = "The block is deleted.")),
)]
async fn handle_delete_block(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    Path((board_id, block_id)): Path<(String, String)>,
    RawQuery(query): RawQuery,
) -> ApiResult {
    let user_id = session.user_id.as_str();
    let notification = change_notification(query.as_deref());
    require_board_permission(
        &app,
        user_id,
        &board_id,
        BoardPermission::ManageBoardCards,
        CHANGES_DENIED,
    )
    .await?;
    let block = block_on_board(&app, &block_id, &board_id).await?;
    if block.block_type == BlockType::Comment && block.created_by != user_id {
        require_board_permission(
            &app,
            user_id,
            &board_id,
            BoardPermission::DeleteOthersComments,
            "access denied to delete other users' comments",
        )
        .await?;
    }
    app.delete_block(&block_id, user_id, notification).await?;
    tracing::debug!(board_id, block_id, "DELETE Block");
    Ok(no_content())
}

/// Bring a deleted block back
///
/// Restores a block of the board from the trash, with the blocks that went there with
/// it. A block whose parent is gone has nowhere to come back to: 404. For editors.
#[utoipa::path(
    post,
    path = "/boards/{boardId}/blocks/{blockId}/undelete",
    operation_id = "undeleteBlock",
    tag = tag::BLOCKS,
    params(BoardId, BlockId),
    responses((status = 200, description = "The block as it is again.", body = Block)),
)]
async fn handle_undelete_block(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    Path((board_id, block_id)): Path<(String, String)>,
) -> ApiResult {
    let user_id = session.user_id.as_str();
    require_board_permission(
        &app,
        user_id,
        &board_id,
        BoardPermission::ManageBoardCards,
        CHANGES_DENIED,
    )
    .await?;
    let board = app.board(&board_id).await?;
    // What is restored is what the trash has of this board. A block that exists, one
    // that never did and one of another board are all not there to restore.
    app.deleted_block(&block_id)
        .await?
        .filter(|deleted| deleted.block.board_id == board.id)
        .ok_or_else(|| AppError::not_found("block"))?;

    let undeleted = app.undelete_block(&block_id, user_id).await?;
    tracing::debug!(block_id, "UNDELETE Block");
    Ok(json_response(&undeleted))
}

/// Change a block
///
/// For editors, also for a comment: a commenter cannot change their own comment this
/// way.
#[utoipa::path(
    patch,
    path = "/boards/{boardId}/blocks/{blockId}",
    operation_id = "patchBlock",
    tag = tag::BLOCKS,
    params(BoardId, BlockId, DisableNotify),
    request_body = BlockPatch,
    responses((status = 204, description = "The block is changed.")),
)]
async fn handle_patch_block(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    Path((board_id, block_id)): Path<(String, String)>,
    RawQuery(query): RawQuery,
    body: Bytes,
) -> ApiResult {
    let user_id = session.user_id.as_str();
    let notification = change_notification(query.as_deref());
    require_board_permission(
        &app,
        user_id,
        &board_id,
        BoardPermission::ManageBoardCards,
        CHANGES_DENIED,
    )
    .await?;
    block_on_board(&app, &block_id, &board_id).await?;
    let (mut patch, changes): (BlockPatch, _) =
        decode_object_with(&body, |patch| take_patch_bodies([Some(patch)]))?;
    put_patch_bodies([&mut patch], changes?)?;
    app.check_body_files(&board_id, patch.body.new_body())
        .await?;

    app.patch_block(&block_id, &patch, user_id, notification)
        .await?;
    tracing::debug!(board_id, block_id, "PATCH Block");
    Ok(no_content())
}

/// Change several blocks
///
/// Applies a patch to each of several blocks of the board, all or none. A body that
/// names no block changes nothing and is answered 204 too. For editors.
#[utoipa::path(
    patch,
    path = "/boards/{boardId}/blocks",
    operation_id = "patchBlocks",
    tag = tag::BLOCKS,
    params(BoardId, DisableNotify),
    request_body = BlockPatchBatch,
    responses((status = 204, description = "The blocks are changed.")),
)]
async fn handle_patch_blocks(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    Path(board_id): Path<String>,
    RawQuery(query): RawQuery,
    body: Bytes,
) -> ApiResult {
    let user_id = session.user_id.as_str();
    let notification = change_notification(query.as_deref());
    require_board_permission(
        &app,
        user_id,
        &board_id,
        BoardPermission::ManageBoardCards,
        CHANGES_DENIED,
    )
    .await?;
    let board = app.board(&board_id).await?;
    let (mut batch, changes): (BlockPatchBatch, _) = decode_object_with(&body, |batch| {
        take_patch_bodies(objects_mut(batch, "blockPatches"))
    })?;
    put_patch_bodies(&mut batch.block_patches, changes?)?;
    for patch in &batch.block_patches {
        app.check_body_files(&board_id, patch.body.new_body())
            .await?;
    }
    if batch.block_ids.len() != batch.block_patches.len() {
        return Err(
            AppError::bad_request("blockIds and blockPatches differ in length")
                .with_field("blockIds"),
        );
    }
    let mut seen = std::collections::HashSet::new();
    for block_id in &batch.block_ids {
        if !seen.insert(block_id.as_str()) {
            return Err(
                AppError::bad_request(format_args!("block {block_id} is listed twice"))
                    .with_field("blockIds"),
            );
        }
        block_on_board(&app, block_id, &board_id).await?;
    }

    app.patch_blocks(&board.team_id, &batch, user_id, notification)
        .await?;
    tracing::debug!(patches = batch.block_ids.len(), "PATCH Blocks");
    Ok(no_content())
}

/// Copy a block
///
/// Copies a block and the blocks directly below it, except comments. A comment cannot
/// be copied. For editors.
#[utoipa::path(
    post,
    path = "/boards/{boardId}/blocks/{blockId}/duplicate",
    operation_id = "duplicateBlock",
    tag = tag::BLOCKS,
    params(BoardId, BlockId, AsTemplate),
    responses((
        status = 201,
        description = "The copies: the copy of the block first, then those of the blocks directly below it, oldest first.",
        body = Vec<Block>,
    )),
)]
async fn handle_duplicate_block(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    Path((board_id, block_id)): Path<(String, String)>,
    RawQuery(query): RawQuery,
) -> ApiResult {
    let user_id = session.user_id.as_str();
    let duplicate_as = duplicate_as(query.as_deref());

    require_board_permission(
        &app,
        user_id,
        &board_id,
        BoardPermission::ViewBoard,
        "access denied to modify board cards",
    )
    .await?;
    let board = app.board(&board_id).await?;
    let block = block_on_board(&app, &block_id, &board.id).await?;
    if block.block_type == BlockType::Comment {
        require_board_permission(
            &app,
            user_id,
            &board_id,
            BoardPermission::CommentBoardCards,
            "access denied to comment on board cards",
        )
        .await?;
        // Comments are never copied: the store leaves them out of every duplication.
        // The original crashes here and answers 500.
        return Err(AppError::bad_request("a comment cannot be duplicated"));
    }
    require_board_permission(
        &app,
        user_id,
        &board_id,
        BoardPermission::ManageBoardCards,
        "access denied to modify board cards",
    )
    .await?;

    tracing::debug!(board_id, block_id, "DuplicateBlock");
    let blocks = app
        .duplicate_block(&board_id, &block_id, user_id, duplicate_as)
        .await?;
    Ok(created(&blocks))
}
