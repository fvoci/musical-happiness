//! Blocks: reading, inserting, patching, deleting, undeleting and duplicating.

use axum::body::Bytes;
use axum::extract::{Path, RawQuery};
use fvoci_model::{
    Block, BlockError, BlockPatch, BlockPatchBatch, BlockType, Body, CommentVersion, Container,
    RelationProperties, UnmatchedRelation, renew_block_ids, stamp_modification_metadata,
};

use serde_json::Value;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::contract::bodies::NewBlock;
use super::contract::params::MemberWorkspaceId;
use super::contract::params::{
    AllBlocks, AsTemplate, BlockId, BlockTypeFilter, BoardId, DisableNotify, OneBlockId, ParentId,
    SourceBoardId,
};
use super::contract::tag;
use super::workspace::InWorkspace;

use super::{ApiResult, created, decode_json, json_response, no_content, query_param};
use super::{
    BlockAction, require_block_patch_permission, require_block_permission,
    require_delete_permission, require_deletion_permission,
};
use super::{change_notification, duplicate_as, require_container_permission};
use super::{
    decode_object_with, element_pointer, objects_mut, put_patch_bodies, take_block_bodies,
    take_patch_bodies,
};
use crate::app::App;
use crate::auth::now_millis;
use crate::error::AppError;
use crate::permissions::ContainerPermission;

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
async fn block_on_container(
    app: &App,
    block_id: &str,
    container_id: &str,
) -> Result<Block, AppError> {
    let block = app.block(block_id).await?;
    if block.container_id == container_id {
        Ok(block)
    } else {
        Err(AppError::not_found("block"))
    }
}

/// Checks that a signed-in user may read the blocks of a board: the members of the
/// board, and for an open template everyone who works in its workspace besides.
async fn require_read_access(ws: &InWorkspace, container: &Container) -> Result<(), AppError> {
    let (app, user_id) = (&ws.app, ws.session.user_id.as_str());
    require_container_permission(
        app,
        user_id,
        &container.id,
        ContainerPermission::View,
        "access denied to board",
    )
    .await?;
    // A guest of the path's workspace reads a template only as a member of it: not an
    // open template of the workspace, nor one the server ships with.
    if container.is_template
        && ws.role == fvoci_model::WorkspaceRole::Guest
        && !app.invited_to_container(user_id, &container.id).await?
    {
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
    ws: &InWorkspace,
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
    let board = ws.app.container(source_board_id).await.map_err(refused)?;
    require_read_access(ws, &board).await.map_err(refused)
}

/// The blocks of a board
///
/// For the board's members, and for an open template everyone who works in its workspace.
/// A guest of the path's workspace reads a template's blocks only as a member of that
/// template (403 otherwise).
///
/// `all` asks for every block and `blockId` for one, which is 404 if the board has no
/// such block. Otherwise `parentId` and `type` narrow the list, and without either the
/// list is every block of the board. A parameter with an empty value counts as left
/// out. Oldest first; the blocks that one request made come in the order that request
/// had them in.
#[utoipa::path(
    get,
    path = "/w/{workspaceId}/boards/{boardId}/blocks",
    operation_id = "getBlocks",
    tag = tag::BLOCKS,
    params(MemberWorkspaceId, BoardId, AllBlocks, OneBlockId, ParentId, BlockTypeFilter),
    responses((status = 200, description = "The blocks.", body = Vec<Block>)),
)]
async fn handle_get_blocks(
    ws: InWorkspace,
    Path((_, board_id)): Path<(String, String)>,
    RawQuery(query): RawQuery,
) -> ApiResult {
    let board = ws.app.container(&board_id).await?;
    require_read_access(&ws, &board).await?;
    let blocks = asked_blocks(&ws.app, &board_id, query.as_deref()).await?;
    tracing::debug!(board_id, block_count = blocks.len(), "GetBlocks");
    Ok(json_response(&blocks))
}

/// The blocks of a board that the query of a request asks for, as `getBlocks` describes
/// them; for whoever may read the board.
pub(super) async fn asked_blocks(
    app: &App,
    container_id: &str,
    query: Option<&str>,
) -> Result<Vec<Block>, AppError> {
    if !query_param(query, "all").is_empty() {
        app.blocks_for_container(container_id).await
    } else if let Some(block_id) = optional_query_param(query, "blockId") {
        Ok(vec![
            block_on_container(app, &block_id, container_id).await?,
        ])
    } else {
        let parent_id = optional_query_param(query, "parentId");
        // The type is matched as it is written: one that no block has finds nothing. The
        // word `unknown` is the exception, and narrows nothing.
        let block_type = optional_query_param(query, "type").map(BlockType::from);
        app.blocks(container_id, parent_id.as_deref(), block_type.as_ref())
            .await
    }
}

/// Checks that the images and file blocks of the blocks' bodies name files of the
/// board, or of the board named by `sourceBoardId`, and copies the files the blocks
/// show from that board.
async fn check_and_copy_body_files(
    ws: &InWorkspace,
    board_id: &str,
    query: Option<&str>,
    blocks: &mut [Block],
) -> Result<(), AppError> {
    let (app, user_id) = (&ws.app, ws.session.user_id.as_str());
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
        require_container_permission(
            app,
            user_id,
            board_id,
            ContainerPermission::ManageCards,
            CHANGES_DENIED,
        )
        .await?;
    }
    // The images of a body show files of the board, or of the board they are copied
    // from.
    let files_board_id = if source_board_id.is_empty() {
        board_id
    } else {
        require_source_board_access(ws, &source_board_id).await?;
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

/// Gives each new block the body the request had for it. A body on a block of a type
/// without one, and a file block in a comment's, are refused here, before anything is
/// copied for them, at the block's place in the request.
fn give_bodies(blocks: &mut [Block], bodies: Vec<Option<Body>>) -> Result<(), AppError> {
    for (index, (block, body)) in blocks.iter_mut().zip(bodies).enumerate() {
        let refused = if body.is_some() && !block.block_type.has_body() {
            Some(BlockError::BodyNotAllowed)
        } else if block.block_type == BlockType::Comment
            && body.as_ref().is_some_and(Body::has_file_blocks)
        {
            Some(BlockError::FileBlockInComment)
        } else {
            None
        };
        if let Some(refused) = refused {
            return Err(AppError::from(refused).under(&element_pointer(Some(""), index)));
        }
        block.body = body;
    }
    Ok(())
}

/// Insert blocks
///
/// Inserts blocks under identifiers the server makes; the identifiers of the request
/// only link its blocks to each other. All blocks are inserted or none, and they are
/// listed afterwards in the order the request had them in. Comments need the role of a
/// commenter, everything else that of an editor. A comment is on a card of the board,
/// and no block is below a comment (400 at the block's `parentId`). A comment's body
/// has no file block (400 at its `body`), and nothing is copied for one that has.
#[utoipa::path(
    post,
    path = "/w/{workspaceId}/boards/{boardId}/blocks",
    operation_id = "createBlocks",
    tag = tag::BLOCKS,
    params(MemberWorkspaceId, BoardId, SourceBoardId, DisableNotify),
    request_body = Vec<NewBlock>,
    responses((
        status = 201,
        description = "The blocks as they were stored, in the order of the request.",
        body = Vec<Block>,
    )),
)]
async fn handle_post_blocks(
    ws: InWorkspace,
    Path((_, board_id)): Path<(String, String)>,
    RawQuery(query): RawQuery,
    body: Bytes,
) -> ApiResult {
    let query = query.as_deref();
    let user_id = ws.session.user_id.as_str();
    let notification = change_notification(query);
    // Which permission is needed depends on the blocks, so the body is read first; but
    // whoever may not even see the board learns nothing from what the body is like.
    require_container_permission(
        &ws.app,
        user_id,
        &board_id,
        ContainerPermission::View,
        CHANGES_DENIED,
    )
    .await?;
    // A list first, so that a request that is none is refused as before, and the bodies
    // apart, so that one that may not be stored is refused as a body.
    let mut blocks: Vec<Value> = decode_json(&body)?;
    let bodies = take_block_bodies(blocks.iter_mut().map(Value::as_object_mut), Some(""))?;
    let mut blocks: Vec<Block> =
        serde_json::from_value(Value::Array(blocks)).map_err(AppError::bad_request)?;
    give_bodies(&mut blocks, bodies)?;
    // Where a block is in the request: the body is the list.
    let place = |index: usize| element_pointer(Some(""), index);

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
        if block.container_id != board_id {
            return Err(AppError::bad_request(format_args!(
                "invalid containerId for block id {id}"
            ))
            .with_field("containerId")
            .under(&place));
        }
    }
    if has_contents {
        require_container_permission(
            &ws.app,
            user_id,
            &board_id,
            ContainerPermission::ManageCards,
            CHANGES_DENIED,
        )
        .await?;
    }
    if has_comments {
        require_container_permission(
            &ws.app,
            user_id,
            &board_id,
            ContainerPermission::CommentCards,
            "access denied to post card comments",
        )
        .await?;
    }

    renew_request_ids(&ws.app, &board_id, &mut blocks).await?;
    stamp_modification_metadata(user_id, &mut blocks, now_millis());

    check_and_copy_body_files(&ws, &board_id, query, &mut blocks).await?;

    // The store names a block it refuses by its identifier.
    let ids: Vec<String> = blocks.iter().map(|block| block.id.clone()).collect();
    ws.app
        .insert_blocks(&mut blocks, user_id, notification)
        .await
        .map_err(|error| error.among_blocks(ids.iter().map(String::as_str), ""))?;
    tracing::debug!(block_count = blocks.len(), "POST Blocks");
    Ok(created(&blocks))
}

/// Gives the blocks of a request to a board new identifiers ([`renew_block_ids`]): a
/// card relation value names the request's cards by their new identifiers, and keeps
/// what else it names, for the store to check.
async fn renew_request_ids(
    app: &App,
    container_id: &str,
    blocks: &mut [Block],
) -> Result<(), AppError> {
    let relations = if blocks
        .iter()
        .any(|block| block.block_type == BlockType::Card)
    {
        RelationProperties::of_board(
            container_id,
            &app.container(container_id).await?.card_properties,
        )
    } else {
        RelationProperties::new()
    };
    renew_block_ids(blocks, &relations, UnmatchedRelation::Kept);
    Ok(())
}

/// Delete a block
///
/// The block goes to the trash with the blocks below it, and `undeleteBlock` brings it
/// back until the trash is emptied of it. For editors; a comment is its author's to
/// delete from the commenter's role up, and a comment of somebody else the board's
/// admins' only (403, `access denied to delete other users' comments`), also as a
/// reply below a comment the user deletes.
#[utoipa::path(
    delete,
    path = "/w/{workspaceId}/boards/{boardId}/blocks/{blockId}",
    operation_id = "deleteBlock",
    tag = tag::BLOCKS,
    params(MemberWorkspaceId, BoardId, BlockId, DisableNotify),
    responses((status = 204, description = "The block is deleted.")),
)]
async fn handle_delete_block(
    ws: InWorkspace,
    Path((_, board_id, block_id)): Path<(String, String, String)>,
    RawQuery(query): RawQuery,
) -> ApiResult {
    let user_id = ws.session.user_id.as_str();
    let notification = change_notification(query.as_deref());
    // Nobody below the commenter's role deletes anything, whatever the block is.
    require_container_permission(
        &ws.app,
        user_id,
        &board_id,
        ContainerPermission::CommentCards,
        CHANGES_DENIED,
    )
    .await?;
    let block = block_on_container(&ws.app, &block_id, &board_id).await?;
    require_delete_permission(&ws.app, user_id, &block, CHANGES_DENIED).await?;
    ws.app
        .delete_block(&block_id, user_id, notification)
        .await?;
    tracing::debug!(board_id, block_id, "DELETE Block");
    Ok(no_content())
}

/// Bring a deleted block back
///
/// Restores a block of the board from the trash, with the blocks that went there with
/// it. A block whose parent is gone has nowhere to come back to: 404; one whose parent
/// may no longer hold it (a comment off a card, a block below a comment): 409. For who may
/// delete what comes back, as `deleteBlock` has it: editors, a comment its author from
/// the commenter's role up if they deleted it themselves, and a comment of somebody
/// else the board's admins only (403, `access denied to undelete other users' comments`),
/// as one of the user's that somebody else deleted (403, `a comment somebody else
/// deleted is brought back by the board's admins`). What decides is what was
/// deleted, as `deleteBlock` decided it: a comment with every reply that went with it,
/// so a block below somebody else's comment is brought back by the admins only, and a
/// card alone, so a comment that went with its card comes back for whoever may bring
/// back the card.
#[utoipa::path(
    post,
    path = "/w/{workspaceId}/boards/{boardId}/blocks/{blockId}/undelete",
    operation_id = "undeleteBlock",
    tag = tag::BLOCKS,
    params(MemberWorkspaceId, BoardId, BlockId),
    responses((status = 200, description = "The block as it is again.", body = Block)),
)]
async fn handle_undelete_block(
    ws: InWorkspace,
    Path((_, board_id, block_id)): Path<(String, String, String)>,
) -> ApiResult {
    let user_id = ws.session.user_id.as_str();
    // Nobody below the commenter's role brings anything back, whatever the block is.
    require_container_permission(
        &ws.app,
        user_id,
        &board_id,
        ContainerPermission::CommentCards,
        CHANGES_DENIED,
    )
    .await?;
    let board = ws.app.container(&board_id).await?;
    // What is restored is what the trash has of this board: the whole deletion the
    // block went with, all of one board. A block that exists, one that never did and
    // one of another board are all not there to restore.
    let deletion = ws.app.deletion(&block_id).await?;
    let Some((top, below)) = deletion
        .split_first()
        .filter(|(top, _)| top.container_id == board.id)
    else {
        return Err(AppError::not_found("block"));
    };
    // Only who may delete it brings it back, as it was deleted: from the block at the
    // top (a block below somebody else's comment brings that comment back). A comment
    // somebody else deleted is not its author's to bring back, but the board's admins'.
    let deleted_by = ws.app.deleted_by(&top.id).await?;
    require_deletion_permission(
        &ws.app,
        user_id,
        top,
        below,
        BlockAction::Undelete {
            deleted_by: &deleted_by,
        },
        CHANGES_DENIED,
    )
    .await?;

    let undeleted = ws.app.undelete_block(&block_id, user_id).await?;
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
    path = "/w/{workspaceId}/boards/{boardId}/blocks/{blockId}/versions",
    operation_id = "getCommentVersions",
    tag = tag::BLOCKS,
    params(MemberWorkspaceId, BoardId, BlockId),
    responses((status = 200, description = "The earlier versions, the first one first.", body = Vec<CommentVersion>)),
)]
async fn handle_get_comment_versions(
    ws: InWorkspace,
    Path((_, board_id, block_id)): Path<(String, String, String)>,
) -> ApiResult {
    let user_id = ws.session.user_id.as_str();
    require_container_permission(
        &ws.app,
        user_id,
        &board_id,
        ContainerPermission::View,
        "access denied to board",
    )
    .await?;
    let comment = match block_on_container(&ws.app, &block_id, &board_id).await {
        Ok(block) => block,
        Err(error) if error.is_not_found() => ws
            .app
            .deleted_block(&block_id)
            .await?
            .map(|deleted| deleted.block)
            .filter(|block| block.container_id == board_id)
            .ok_or_else(|| AppError::not_found("comment"))?,
        Err(error) => return Err(error),
    };
    if comment.block_type != BlockType::Comment {
        return Err(AppError::not_found("comment"));
    }
    let versions = ws.app.comment_versions(&block_id).await?;
    Ok(json_response(&versions))
}

/// Change a block
///
/// For editors. A comment is changed by its author with the commenter's role, and the
/// comment of somebody else by the board's admins only (as `deleteBlock` has it): an
/// editor may not change it. The commenter's role changes a comment's text (its body);
/// moving a comment to another card (`parentId`) or making it a block of another type
/// is for editors, and making a block of somebody else a comment for the board's admins.
/// A comment goes only on a card of the board, no block goes below a comment (400 at
/// `parentId`), and a card with comments stays a card (400 at `type`). A comment's body
/// has no file block, nor does a block made a comment (400 at `body`). The block's
/// `modifiedBy` and `updateAt` say who changed it and when.
#[utoipa::path(
    patch,
    path = "/w/{workspaceId}/boards/{boardId}/blocks/{blockId}",
    operation_id = "patchBlock",
    tag = tag::BLOCKS,
    params(MemberWorkspaceId, BoardId, BlockId, DisableNotify),
    request_body = BlockPatch,
    responses((status = 204, description = "The block is changed.")),
)]
async fn handle_patch_block(
    ws: InWorkspace,
    Path((_, board_id, block_id)): Path<(String, String, String)>,
    RawQuery(query): RawQuery,
    body: Bytes,
) -> ApiResult {
    let user_id = ws.session.user_id.as_str();
    let notification = change_notification(query.as_deref());
    // Who cannot even comment changes nothing, whatever the block.
    require_container_permission(
        &ws.app,
        user_id,
        &board_id,
        ContainerPermission::CommentCards,
        CHANGES_DENIED,
    )
    .await?;
    let block = block_on_container(&ws.app, &block_id, &board_id).await?;
    require_block_permission(
        &ws.app,
        user_id,
        &block,
        BlockAction::Change,
        CHANGES_DENIED,
    )
    .await?;
    let (mut patch, changes): (BlockPatch, _) =
        decode_object_with(&body, |patch| take_patch_bodies([Some(patch)], None))?;
    put_patch_bodies([&mut patch], changes?, None)?;
    require_block_patch_permission(&ws.app, user_id, &block, &patch, CHANGES_DENIED).await?;
    ws.app
        .check_body_files(&board_id, patch.body.new_body())
        .await?;

    // The store names what it refuses as a block has it; a patch has it under another
    // name.
    ws.app
        .patch_block(&block_id, &patch, user_id, notification)
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
    path = "/w/{workspaceId}/boards/{boardId}/blocks",
    operation_id = "patchBlocks",
    tag = tag::BLOCKS,
    params(MemberWorkspaceId, BoardId, DisableNotify),
    request_body = BlockPatchBatch,
    responses((status = 204, description = "The blocks are changed.")),
)]
async fn handle_patch_blocks(
    ws: InWorkspace,
    Path((_, board_id)): Path<(String, String)>,
    RawQuery(query): RawQuery,
    body: Bytes,
) -> ApiResult {
    let user_id = ws.session.user_id.as_str();
    let notification = change_notification(query.as_deref());
    // Who cannot even comment changes nothing, whatever the blocks.
    require_container_permission(
        &ws.app,
        user_id,
        &board_id,
        ContainerPermission::CommentCards,
        CHANGES_DENIED,
    )
    .await?;
    let board = ws.app.container(&board_id).await?;
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
        let block = block_on_container(&ws.app, block_id, &board_id).await?;
        require_block_permission(
            &ws.app,
            user_id,
            &block,
            BlockAction::Change,
            CHANGES_DENIED,
        )
        .await?;
        require_block_patch_permission(&ws.app, user_id, &block, patch, CHANGES_DENIED).await?;
    }
    for (index, patch) in batch.block_patches.iter().enumerate() {
        ws.app
            .check_body_files(&board_id, patch.body.new_body())
            .await
            .map_err(|error| error.under(&element_pointer(Some(PATCHES), index)))?;
    }

    ws.app
        .patch_blocks(&board.workspace_id, &batch, user_id, notification)
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
    path = "/w/{workspaceId}/boards/{boardId}/blocks/{blockId}/duplicate",
    operation_id = "duplicateBlock",
    tag = tag::BLOCKS,
    params(MemberWorkspaceId, BoardId, BlockId, AsTemplate),
    responses((
        status = 201,
        description = "The copies: the copy of the block first, then those of the blocks directly below it, oldest first.",
        body = Vec<Block>,
    )),
)]
async fn handle_duplicate_block(
    ws: InWorkspace,
    Path((_, board_id, block_id)): Path<(String, String, String)>,
    RawQuery(query): RawQuery,
) -> ApiResult {
    let user_id = ws.session.user_id.as_str();
    let duplicate_as = duplicate_as(query.as_deref());

    require_container_permission(
        &ws.app,
        user_id,
        &board_id,
        ContainerPermission::View,
        "access denied to modify board cards",
    )
    .await?;
    let board = ws.app.container(&board_id).await?;
    let block = block_on_container(&ws.app, &block_id, &board.id).await?;
    if block.block_type == BlockType::Comment {
        require_container_permission(
            &ws.app,
            user_id,
            &board_id,
            ContainerPermission::CommentCards,
            "access denied to comment on board cards",
        )
        .await?;
        // Comments are never copied: the store leaves them out of every duplication.
        // The original crashes here and answers 500.
        return Err(AppError::bad_request("a comment cannot be duplicated"));
    }
    require_container_permission(
        &ws.app,
        user_id,
        &board_id,
        ContainerPermission::ManageCards,
        "access denied to modify board cards",
    )
    .await?;

    tracing::debug!(board_id, block_id, "DuplicateBlock");
    let blocks = ws
        .app
        .duplicate_block(&board_id, &block_id, user_id, duplicate_as)
        .await?;
    Ok(created(&blocks))
}
