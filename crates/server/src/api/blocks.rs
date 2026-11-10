//! Blocks: reading, inserting, patching, deleting, undeleting and duplicating.

use axum::Router;
use axum::body::Bytes;
use axum::extract::{Path, RawQuery, State};
use axum::routing::{delete, get, post};
use fvoci_model::{
    Block, BlockPatch, BlockPatchBatch, BlockType, Board, BoardType, GLOBAL_TEAM_ID,
    generate_block_ids, stamp_modification_metadata,
};
use fvoci_store::DuplicateAs;

use super::{
    ApiResult, OptionalSession, RequiredSession, decode_json_or_internal, empty_response,
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

/// The 404 for a block that exists, but on another board than the one in the path.
fn not_on_board(block: &Block, board_id: &str) -> AppError {
    AppError::not_found(format_args!("block ID={} on BoardID={board_id}", block.id))
}

/// Returns the block if it belongs to the board, and the 404 of [`not_on_board`] if not.
async fn block_on_board(app: &App, block_id: &str, board_id: &str) -> Result<Block, AppError> {
    let block = app.block(block_id).await?;
    if block.board_id == board_id {
        Ok(block)
    } else {
        Err(not_on_board(&block, board_id))
    }
}

/// Checks that a signed-in user may read the blocks of a board: the members of the
/// board, and for an open template everyone in its team.
async fn require_read_access(app: &App, user_id: &str, board: &Board) -> Result<(), AppError> {
    if board.is_template && board.board_type == BoardType::Open {
        if board.team_id != GLOBAL_TEAM_ID
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
    // Single-user mode has no user records, and no guests.
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
        Err(AppError::forbidden("access denied to source board"))
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
    let board = app.store().get_board(&board_id).await?;
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
    Ok(json_response(&blocks))
}

/// `POST /boards/{board_id}/blocks`: inserts blocks under identifiers the server
/// generates; the given identifiers only link the blocks of the request to each other.
///
/// Comments need the permission to comment, everything else the permission to manage
/// cards. Preserved quirk: a request without blocks is checked for nothing and answers
/// `[]`, whoever sends it and whatever board it names.
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
    // `null` is an empty list here, as in the original.
    let mut blocks: Vec<Block> =
        decode_json_or_internal::<Option<Vec<Block>>>(&body)?.unwrap_or_default();

    let mut has_comments = false;
    let mut has_contents = false;
    for block in &blocks {
        let id = &block.id;
        if block.block_type.is_empty() {
            return Err(AppError::bad_request(format_args!(
                "missing type for block id {id}"
            )));
        }
        if block.block_type == BlockType::Comment {
            has_comments = true;
        } else {
            has_contents = true;
        }
        if block.create_at < 1 {
            return Err(AppError::bad_request(format_args!(
                "invalid createAt for block id {id}"
            )));
        }
        if block.update_at < 1 {
            return Err(AppError::bad_request(format_args!(
                "invalid UpdateAt for block id {id}"
            )));
        }
        if block.board_id != board_id {
            return Err(AppError::bad_request(format_args!(
                "invalid BoardID for block id {id}"
            )));
        }
    }
    if has_contents {
        require_board_permission(
            &app,
            user_id,
            &board_id,
            BoardPermission::ManageBoardCards,
            "access denied to make board changes",
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
        app.copy_and_update_card_files(&source_board_id, user_id, &blocks, DuplicateAs::Copy)
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
        "access denied to make board changes",
    )
    .await?;
    let block = app.block(&block_id).await?;
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
    if block.board_id != board_id {
        return Err(not_on_board(&block, &board_id));
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
    let board = app.store().get_board(&board_id).await?;
    // The original dereferences the missing history entry of a block that never
    // existed and answers 500 from its panic handler (server/api/blocks.go:458). A 404
    // says what is wrong.
    let block = app
        .last_block_history_entry(&block_id)
        .await?
        .ok_or_else(|| AppError::not_found(format_args!("block ID={block_id}")))?;
    if block.board_id != board.id {
        return Err(not_on_board(&block, &board.id));
    }
    // Preserved quirk: the message speaks of members (server/api/blocks.go:465).
    require_board_permission(
        &app,
        user_id,
        &board_id,
        BoardPermission::ManageBoardCards,
        "access denied to modify board members",
    )
    .await?;

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
        "access denied to make board changes",
    )
    .await?;
    block_on_board(&app, &block_id, &board_id).await?;
    let patch: BlockPatch = decode_json_or_internal(&body)?;

    app.patch_block(&block_id, &patch, user_id, notification)
        .await?;
    tracing::debug!(board_id, block_id, "PATCH Block");
    Ok(empty_response())
}

/// `PATCH /boards/{board_id}/blocks`: partially updates several blocks, all or none.
///
/// Preserved quirks (server/api/blocks.go:626-663): the board in the path is not looked
/// at, the blocks may be on any boards whose cards the user may manage; a block that
/// does not exist is reported as a refusal, not as missing; and the websocket
/// broadcast goes to the team with the empty identifier, because the original reads a
/// team from a path that has none. So the members of the board are not told about
/// batch patches; only listeners of the blocks themselves are.
async fn handle_patch_blocks(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    RawQuery(query): RawQuery,
    body: Bytes,
) -> ApiResult {
    /// The team the original finds in this route's path.
    const TEAM_OF_THE_PATH: &str = "";

    let user_id = session.user_id.as_str();
    let notification = change_notification(query.as_deref());
    let batch: BlockPatchBatch = decode_json_or_internal(&body)?;

    for block_id in &batch.block_ids {
        let Ok(block) = app.block(block_id).await else {
            return Err(AppError::forbidden("access denied to make board changes"));
        };
        // Preserved quirk: the message ends in a stray letter.
        require_board_permission(
            &app,
            user_id,
            &block.board_id,
            BoardPermission::ManageBoardCards,
            "access denied to make board changesa",
        )
        .await?;
    }

    app.patch_blocks(TEAM_OF_THE_PATH, &batch, user_id, notification)
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

    let board = app.store().get_board(&board_id).await?;
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
    } else {
        require_board_permission(
            &app,
            user_id,
            &board_id,
            BoardPermission::ManageBoardCards,
            "access denied to modify board cards",
        )
        .await?;
    }

    tracing::debug!(board_id, block_id, "DuplicateBlock");
    let blocks = app
        .duplicate_block(&board_id, &block_id, user_id, duplicate_as)
        .await?;
    Ok(json_response(&blocks))
}
