//! Blocks: reading, inserting, patching, deleting, undeleting and duplicating.

use axum::Router;
use axum::body::Bytes;
use axum::extract::{Path, RawQuery, State};
use axum::routing::{delete, get, post};
use fvoci_model::{
    Block, BlockPatch, BlockPatchBatch, BlockType, Board, BoardType, generate_block_ids,
    stamp_modification_metadata,
};

use super::{
    ApiResult, OptionalSession, RequiredSession, decode_json, decode_object, empty_response,
    has_valid_read_token, json_response, query_param,
};
use super::{change_notification, duplicate_as, require_board_permission, user_is_guest};
use crate::app::App;
use crate::auth::now_millis;
use crate::error::AppError;
use crate::permissions::{BoardPermission, TeamPermission};

pub(super) fn routes() -> Router<App> {
    Router::new()
        .route(
            "/boards/{board_id}/blocks",
            get(handle_get_blocks)
                .post(handle_post_blocks)
                .patch(handle_patch_blocks),
        )
        .route(
            "/boards/{board_id}/blocks/{block_id}",
            delete(handle_delete_block).patch(handle_patch_block),
        )
        .route(
            "/boards/{board_id}/blocks/{block_id}/undelete",
            post(handle_undelete_block),
        )
        .route(
            "/boards/{board_id}/blocks/{block_id}/duplicate",
            post(handle_duplicate_block),
        )
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
    let readable = match app.board(source_board_id).await {
        Ok(board) => require_read_access(app, user_id, &board).await.is_ok(),
        Err(_) => false,
    };
    if readable {
        Ok(())
    } else {
        // The same answer for a board that is not there and one the user may not read.
        Err(AppError::not_found("source board"))
    }
}

/// `GET /boards/{board_id}/blocks`: the blocks of a board, for its members and for
/// holders of its read token.
///
/// `all` returns every block, `block_id` the one block, and otherwise `parent_id` and
/// `type` narrow the list. The original reads no paging parameters here.
async fn handle_get_blocks(
    State(app): State<App>,
    session: OptionalSession,
    Path(board_id): Path<String>,
    RawQuery(query): RawQuery,
) -> ApiResult {
    let query = query.as_deref();
    let user_id = session.user_id();

    let has_read_token = has_valid_read_token(&app, query, &board_id).await;
    if user_id.is_empty() && !has_read_token {
        return Err(AppError::unauthorized("access denied to board"));
    }
    let board = app.board(&board_id).await?;
    if !has_read_token {
        require_read_access(&app, user_id, &board).await?;
    }

    let blocks = if !query_param(query, "all").is_empty() {
        app.blocks_for_board(&board_id).await?
    } else if let Some(block_id) = optional_query_param(query, "block_id") {
        vec![block_on_board(&app, &block_id, &board_id).await?]
    } else {
        let parent_id = optional_query_param(query, "parent_id");
        // The type is matched as it is written; an unknown one finds nothing.
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

/// `POST /boards/{board_id}/blocks`: inserts blocks under identifiers the server
/// generates; the given identifiers only link the blocks of the request to each other.
///
/// Comments need the permission to comment, everything else the permission to manage
/// cards. All blocks are inserted or none.
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
    let mut blocks: Vec<Block> = decode_json(&body)?;

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
    let source_board_id = query_param(query, "sourceBoardID");
    if !source_board_id.is_empty() {
        require_source_board_access(&app, user_id, &source_board_id).await?;
        app.copy_and_update_card_files(&source_board_id, user_id, &blocks)
            .await?;
    }

    app.insert_blocks(&mut blocks, user_id, notification)
        .await?;
    tracing::debug!(block_count = blocks.len(), "POST Blocks");
    Ok(json_response(&blocks))
}

/// `DELETE /boards/{board_id}/blocks/{block_id}`: deletes a block and its direct
/// children.
///
/// Preserved quirk: whatever the block is, the permission to manage cards is needed, so
/// a commenter cannot delete their own comment through this route
/// (server/api/blocks.go:372).
///
/// Not in the original: a comment is deleted by its author or by someone who may delete
/// the comments of others, which is the board's admins. The original lets every editor
/// delete anyone's comment.
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
    Ok(empty_response())
}

/// `POST /boards/{board_id}/blocks/{block_id}/undelete`: restores a deleted block and
/// answers with it.
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

/// `PATCH /boards/{board_id}/blocks/{block_id}`: partially updates a block.
///
/// Preserved quirk: as for deletion, the permission to manage cards is needed even for
/// the user's own comment (server/api/blocks.go:541).
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
    let patch: BlockPatch = decode_object(&body)?;

    app.patch_block(&block_id, &patch, user_id, notification)
        .await?;
    tracing::debug!(board_id, block_id, "PATCH Block");
    Ok(empty_response())
}

/// `PATCH /boards/{board_id}/blocks`: partially updates several blocks of the board,
/// all or none.
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
    let batch: BlockPatchBatch = decode_object(&body)?;
    if batch.block_ids.len() != batch.block_patches.len() {
        return Err(AppError::bad_request(
            "block_ids and block_patches differ in length",
        ));
    }
    let mut seen = std::collections::HashSet::new();
    for block_id in &batch.block_ids {
        if !seen.insert(block_id.as_str()) {
            return Err(
                AppError::bad_request(format_args!("block {block_id} is listed twice"))
                    .with_field("block_ids"),
            );
        }
        block_on_board(&app, block_id, &board_id).await?;
    }

    app.patch_blocks(&board.team_id, &batch, user_id, notification)
        .await?;
    tracing::debug!(patches = batch.block_ids.len(), "PATCH Blocks");
    Ok(empty_response())
}

/// `POST /boards/{board_id}/blocks/{block_id}/duplicate`: copies a block and its direct
/// children, except comments, and answers with the copies. `asTemplate=true` marks the
/// copy as a template.
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
    Ok(json_response(&blocks))
}
