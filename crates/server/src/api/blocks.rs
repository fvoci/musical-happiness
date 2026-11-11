//! Blocks: reading, inserting, patching, deleting, undeleting and duplicating.

use axum::body::Bytes;
use axum::extract::{Path, RawQuery, State};
use fvoci_model::{
    Block, BlockError, BlockPatch, BlockPatchBatch, BlockType, Board, BoardType, CommentVersion,
    generate_block_ids, stamp_modification_metadata,
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
    ApiResult, OptionalSession, RequiredSession, by_read_token, created, decode_json,
    json_response, no_content, query_param,
};
use super::{change_notification, duplicate_as, require_board_permission, user_is_guest};
use super::{
    decode_object_with, element_pointer, objects_mut, put_patch_bodies, take_block_bodies,
    take_patch_bodies,
};
use super::{
    require_block_change_permission, require_block_patch_permission, require_comment_owner_or_admin,
};
use crate::app::App;
use crate::auth::now_millis;
use crate::error::AppError;
use crate::permissions::BoardPermission;

pub(super) fn routes() -> OpenApiRouter<App> {
    OpenApiRouter::new()
        .routes(routes!(
            handle_get_blocks,
            handle_post_blocks,
            handle_patch_blocks
        ))
        .routes(routes!(handle_delete_block, handle_patch_block))
        .routes(routes!(handle_undelete_block))
        .routes(routes!(handle_get_comment_versions))
        .routes(routes!(handle_duplicate_block))
}

/// Returns a query parameter, or `None` if it is absent or empty.
fn optional_query_param(query: Option<&str>, name: &str) -> Option<String> {
    Some(query_param(query, name)).filter(|value| !value.is_empty())
}

/// The refusal to change what is on a board.
const CHANGES_DENIED: &str = "access denied to make board changes";

/// The JSON Pointers to the fields of a block and to the member of a patch that changes
/// them.
pub(super) const BLOCK_FIELDS: &str = "/fields";
pub(super) const PATCH_FIELDS: &str = "/updatedFields";

/// The JSON Pointer to the patches of a `BlockPatchBatch`.
const PATCHES: &str = "/blockPatches";

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
/// board, and for an open template everyone who works in its team besides.
async fn require_read_access(app: &App, user_id: &str, board: &Board) -> Result<(), AppError> {
    if board.is_template && board.board_type == BoardType::Open {
        if !board.is_builtin
            && !app.works_in_team(user_id, &board.team_id).await?
            && !app.is_board_reader(user_id, &board.id).await?
        {
            // A template of a team the user does not work in is not there.
            return Err(AppError::not_found("board"));
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
    let refused = |error: AppError| {
        if error.is_refusal() {
            AppError::not_found("source board")
        } else {
            error
        }
    };
    let board = app.board(source_board_id).await.map_err(refused)?;
    require_read_access(app, user_id, &board)
        .await
        .map_err(refused)
}

/// The blocks of a board
///
/// For the board's members, and for an open template everyone in its team. With the
/// board's read token no session is needed. Who reads with the token and is no member
/// of the board, without a session or signed in, is told no user's identifier:
/// `createdBy` and `modifiedBy` are empty, so is the `userId` of every mention in a
/// body, and so is each user that the board's person properties (`person`,
/// `multiPerson`, `createdBy`, `updatedBy`) name in a card's `fields.properties` and in
/// the clauses of a view's `fields.filter`; a list keeps its length, so how many people
/// a card has still shows. A card's values of properties the board does not define are
/// left out, and a view grouped by a person property keeps of `visibleOptionIds`,
/// `hiddenOptionIds`, `collapsedOptionIds` and `kanbanCalculations` only the group of
/// the cards without a value (`""`).
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

    let shared = by_read_token(&app, query, &board_id).await?;
    let has_read_token = shared.is_some();
    if user_id.is_empty() && !has_read_token {
        return Err(AppError::unauthorized("access denied to board"));
    }
    let app = shared.unwrap_or(app);
    let board = app.board(&board_id).await?;
    let by_token_alone = if has_read_token {
        !app.is_board_reader(user_id, &board_id).await?
    } else {
        require_read_access(&app, user_id, &board).await?;
        false
    };

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
    if by_token_alone {
        let people = board.person_properties();
        for block in &mut blocks {
            block.hide_people(&people);
        }
    }
    Ok(json_response(&blocks))
}

/// Checks that the images of the blocks' bodies show files of the board, or of the board
/// named by `sourceBoardId`, and copies the files the blocks show from that board.
async fn check_and_copy_body_files(
    app: &App,
    user_id: &str,
    board_id: &str,
    query: Option<&str>,
    blocks: &mut [Block],
) -> Result<(), AppError> {
    // Set when a template is made from a board or a board from a template: the files
    // the blocks refer to are then copied over from that board.
    let source_board_id = query_param(query, "sourceBoardId");
    // Copying files into the board is an editor's: a commenter, who may post comments
    // with bodies, may not upload either.
    // A request with anything but comments needed the editor's permission already.
    if !source_board_id.is_empty()
        && blocks
            .iter()
            .all(|block| block.block_type == BlockType::Comment)
        && blocks.iter().any(|block| !block.shown_files().is_empty())
    {
        require_board_permission(
            app,
            user_id,
            board_id,
            BoardPermission::ManageBoardCards,
            CHANGES_DENIED,
        )
        .await?;
    }
    // The images of a body show files of the board, or of the board they are copied
    // from.
    let files_board_id = if source_board_id.is_empty() {
        board_id
    } else {
        require_source_board_access(app, user_id, &source_board_id).await?;
        &source_board_id
    };
    for (index, block) in blocks.iter().enumerate() {
        app.check_body_files(files_board_id, block.body.as_ref())
            .await
            .map_err(|error| error.under(&element_pointer(Some(""), index)))?;
    }
    if !source_board_id.is_empty() {
        // The blocks are not stored yet: each is pointed at its copy here, and stored
        // with that below.
        app.copy_card_files_into(&source_board_id, blocks).await;
    }

    Ok(())
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
    let bodies = take_block_bodies(blocks.iter_mut().map(Value::as_object_mut), Some(""))?;
    let mut blocks: Vec<Block> =
        serde_json::from_value(Value::Array(blocks)).map_err(AppError::bad_request)?;
    // Where a block is in the request: the body is the list.
    let place = |index: usize| element_pointer(Some(""), index);
    for (index, (block, body)) in blocks.iter_mut().zip(bodies).enumerate() {
        // Refused before anything is copied for it.
        if body.is_some() && !block.block_type.has_body() {
            return Err(AppError::from(BlockError::BodyNotAllowed).under(&place(index)));
        }
        block.body = body;
    }

    let mut has_comments = false;
    let mut has_contents = false;
    for (index, block) in blocks.iter().enumerate() {
        let id = &block.id;
        let place = place(index);
        if block.block_type.is_empty() {
            return Err(
                AppError::bad_request(format_args!("missing type for block id {id}"))
                    .with_field("type")
                    .under(&place),
            );
        }
        if !block.block_type.is_current() {
            return Err(
                AppError::from(BlockError::type_not_current(&block.block_type)).under(&place),
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
                    .with_field("createAt")
                    .under(&place),
            );
        }
        if block.update_at < 1 {
            return Err(
                AppError::bad_request(format_args!("invalid updateAt for block id {id}"))
                    .with_field("updateAt")
                    .under(&place),
            );
        }
        if block.board_id != board_id {
            return Err(
                AppError::bad_request(format_args!("invalid boardId for block id {id}"))
                    .with_field("boardId")
                    .under(&place),
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

    check_and_copy_body_files(&app, user_id, &board_id, query, &mut blocks).await?;

    // The store names a block it refuses by its identifier.
    let ids: Vec<String> = blocks.iter().map(|block| block.id.clone()).collect();
    app.insert_blocks(&mut blocks, user_id, notification)
        .await
        .map_err(|error| error.among_blocks(ids.iter().map(String::as_str), ""))?;
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
    require_comment_owner_or_admin(
        &app,
        user_id,
        &block,
        "access denied to delete other users' comments",
    )
    .await?;
    app.delete_block(&block_id, user_id, notification).await?;
    tracing::debug!(board_id, block_id, "DELETE Block");
    Ok(no_content())
}

/// Bring a deleted block back
///
/// Restores a block of the board from the trash, with the blocks that went there with
/// it. A block whose parent is gone has nowhere to come back to: 404. For who may
/// delete what comes back, as `deleteBlock` has it: editors, and a comment of somebody
/// else the board's admins only (403, `access denied to undelete other users'
/// comments`). What decides is the block that was deleted, which took the others along:
/// a block below somebody else's comment is brought back by the admins only, and a
/// comment that went with its card by whoever may bring back the card.
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
    // Only who may delete it brings it back: what comes back is the whole deletion, so
    // the block that was deleted, at its top, is what decides (a block below somebody
    // else's comment brings that comment back).
    if let Some(top) = app.deletion_top(&block_id).await? {
        require_comment_owner_or_admin(
            &app,
            user_id,
            &top.block,
            "access denied to undelete other users' comments",
        )
        .await?;
    }

    let undeleted = app.undelete_block(&block_id, user_id).await?;
    tracing::debug!(block_id, "UNDELETE Block");
    Ok(json_response(&undeleted))
}

/// What a comment said before
///
/// The earlier versions of a comment's body: every version but the one it has now, with
/// who wrote it and when. The first one first; it is the text the comment was posted
/// with, by its author. The server keeps the first version and the newest ones, up to
/// its setting `max_comment_versions` in all, so the numbers of the versions may leave
/// gaps. Empty for a comment whose body was never changed; the comment's `editedAt` says
/// whether it was. For the board's members, also in the trash: the versions go only
/// when the comment is gone for good. Not for a reader by the board's read token. A
/// block that is no comment is not there (404).
#[utoipa::path(
    get,
    path = "/boards/{boardId}/blocks/{blockId}/versions",
    operation_id = "getCommentVersions",
    tag = tag::BLOCKS,
    params(BoardId, BlockId),
    responses((status = 200, description = "The earlier versions, the first one first.", body = Vec<CommentVersion>)),
)]
async fn handle_get_comment_versions(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    Path((board_id, block_id)): Path<(String, String)>,
) -> ApiResult {
    let user_id = session.user_id.as_str();
    require_board_permission(
        &app,
        user_id,
        &board_id,
        BoardPermission::ViewBoard,
        "access denied to board",
    )
    .await?;
    let comment = match block_on_board(&app, &block_id, &board_id).await {
        Ok(block) => block,
        Err(error) if error.is_not_found() => app
            .deleted_block(&block_id)
            .await?
            .map(|deleted| deleted.block)
            .filter(|block| block.board_id == board_id)
            .ok_or_else(|| AppError::not_found("comment"))?,
        Err(error) => return Err(error),
    };
    if comment.block_type != BlockType::Comment {
        return Err(AppError::not_found("comment"));
    }
    let versions = app.comment_versions(&block_id).await?;
    Ok(json_response(&versions))
}

/// Change a block
///
/// For editors. A comment is changed by its author with the commenter's role, and the
/// comment of somebody else by the board's admins only (as `deleteBlock` has it): an
/// editor may not change it. The commenter's role changes a comment's text (its body);
/// moving a comment to another card (`parentId`) or making it a block of another type
/// is for editors, and making a block of somebody else a comment for the board's admins.
/// The block's `modifiedBy` and `updateAt` say who changed it and when.
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
    // Who cannot even comment changes nothing, whatever the block.
    require_board_permission(
        &app,
        user_id,
        &board_id,
        BoardPermission::CommentBoardCards,
        CHANGES_DENIED,
    )
    .await?;
    let block = block_on_board(&app, &block_id, &board_id).await?;
    require_block_change_permission(&app, user_id, &block, CHANGES_DENIED).await?;
    let (mut patch, changes): (BlockPatch, _) =
        decode_object_with(&body, |patch| take_patch_bodies([Some(patch)], None))?;
    put_patch_bodies([&mut patch], changes?, None)?;
    require_block_patch_permission(&app, user_id, &block, &patch, CHANGES_DENIED).await?;
    app.check_body_files(&board_id, patch.body.new_body())
        .await?;

    // The store names what it refuses as a block has it; a patch has it under another
    // name.
    app.patch_block(&block_id, &patch, user_id, notification)
        .await
        .map_err(|error| error.moved(BLOCK_FIELDS, PATCH_FIELDS))?;
    tracing::debug!(board_id, block_id, "PATCH Block");
    Ok(no_content())
}

/// Change several blocks
///
/// Applies a patch to each of several blocks of the board, all or none. A body that
/// names no block changes nothing and is answered 204 too. Each block needs what
/// `patchBlock` needs: the editor's role, for a comment its author's commenter role or
/// for the comment of somebody else the board's admin role.
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
    // Who cannot even comment changes nothing, whatever the blocks.
    require_board_permission(
        &app,
        user_id,
        &board_id,
        BoardPermission::CommentBoardCards,
        CHANGES_DENIED,
    )
    .await?;
    let board = app.board(&board_id).await?;
    let (mut batch, changes): (BlockPatchBatch, _) = decode_object_with(&body, |batch| {
        take_patch_bodies(objects_mut(batch, "blockPatches"), Some(PATCHES))
    })?;
    put_patch_bodies(&mut batch.block_patches, changes?, Some(PATCHES))?;
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
    }
    for (block_id, patch) in batch.block_ids.iter().zip(&batch.block_patches) {
        let block = block_on_board(&app, block_id, &board_id).await?;
        require_block_change_permission(&app, user_id, &block, CHANGES_DENIED).await?;
        require_block_patch_permission(&app, user_id, &block, patch, CHANGES_DENIED).await?;
    }
    for (index, patch) in batch.block_patches.iter().enumerate() {
        app.check_body_files(&board_id, patch.body.new_body())
            .await
            .map_err(|error| error.under(&element_pointer(Some(PATCHES), index)))?;
    }

    app.patch_blocks(&board.team_id, &batch, user_id, notification)
        .await
        .map_err(|error| {
            error
                .moved(BLOCK_FIELDS, PATCH_FIELDS)
                .among_blocks(batch.block_ids.iter().map(String::as_str), PATCHES)
        })?;
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
