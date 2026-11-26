//! Shared helpers for comment endpoints on room-bearing blocks (`page_comments`,
//! `unit_comments`).

use axum::body::Bytes;
use fvoci_model::{
    Block, BlockPatch, BlockType, Body, BodyChange, RefusalReason, Refusals, new_id,
};
use serde::de::DeserializeOwned;

use super::workspace::InWorkspace;
use super::{
    ApiResult, BlockAction, created, decode_with_document, json_response, no_content,
    require_block_permission, require_delete_permission, require_deletion_permission,
};
use crate::app::ChangeNotification;
use crate::error::AppError;

/// Refusal message when a comment body is empty or omitted.
const TEXT_MISSING: &str = "a comment has a text";

/// Refusal message when `readAt` is omitted on a comment patch.
const READ_AT_MISSING: &str =
    "a comment is changed with readAt, the updatedAt of the comment as it was read";

/// Extracts the non-empty [`Body`] from a comment request's [`BodyChange`].
pub(super) fn text_of(change: BodyChange) -> Result<Body, AppError> {
    match change {
        BodyChange::Set(body) => Ok(body),
        BodyChange::Keep | BodyChange::Clear => {
            let mut refused = Refusals::new();
            refused.refuse("body", RefusalReason::Required, TEXT_MISSING);
            Err(refused.into())
        }
    }
}

/// Whether `block` is a comment directly on `parent` within the same container.
pub(super) fn is_comment_on(block: &Block, parent: &Block) -> bool {
    block.block_type == BlockType::Comment
        && block.container_id == parent.container_id
        && block.parent_id == parent.id
}

/// Resolves a live comment under an already-authorized `parent` block (`None` when absent
/// or not a comment on `parent`).
pub(super) async fn find_live_comment_on(
    ws: &InWorkspace,
    parent: &Block,
    comment_id: &str,
) -> Result<Option<Block>, AppError> {
    match ws.app.block(comment_id).await {
        Ok(block) if is_comment_on(&block, parent) => Ok(Some(block)),
        Ok(_) => Ok(None),
        Err(error) if error.is_not_found() => Ok(None),
        Err(error) => Err(error),
    }
}

/// Resolves a required live comment under an already-authorized `parent` block (404 when
/// absent).
pub(super) async fn existing_comment_on(
    ws: &InWorkspace,
    parent: &Block,
    comment_id: &str,
) -> Result<Block, AppError> {
    find_live_comment_on(ws, parent, comment_id)
        .await?
        .ok_or_else(|| AppError::not_found("comment"))
}

/// Lists comments on an already-authorized `parent` block, oldest first.
pub(super) async fn list_comments_on(
    ws: &InWorkspace,
    container_id: &str,
    parent_id: &str,
) -> ApiResult {
    let mut comments = ws
        .app
        .blocks(container_id, Some(parent_id), Some(&BlockType::Comment))
        .await?;
    comments.sort_by(|one, other| (one.created_at, &one.id).cmp(&(other.created_at, &other.id)));
    Ok(json_response(&comments))
}

/// Creates a comment on an already-authorized `parent` block from a decoded payload `T`
/// (whose `body` is carried by `decode_with_document`).
pub(super) async fn create_comment_on<T: DeserializeOwned>(
    ws: &InWorkspace,
    container_id: &str,
    parent: Block,
    parent_what: &'static str,
    raw_body: &Bytes,
) -> Result<Block, AppError> {
    let user_id = ws.session.user_id.as_str();
    let (_new, document): (T, _) = decode_with_document(raw_body)?;
    let text = text_of(document)?;
    ws.app.check_body_files(container_id, Some(&text)).await?;
    let mut comment = Block {
        id: new_id(),
        container_id: container_id.to_owned(),
        parent_id: parent.id,
        block_type: BlockType::Comment,
        body: Some(text),
        ..Block::default()
    };
    if let Err(error) = ws
        .app
        .insert_block(&mut comment, user_id, ChangeNotification::Send)
        .await
    {
        return Err(match ws.app.block(&comment.parent_id).await {
            Err(gone) if gone.is_not_found() => AppError::not_found(parent_what),
            _ => error,
        });
    }
    Ok(comment)
}

/// Patches an already-resolved `comment` block using `read_at_of` to extract `readAt` from
/// the request payload `T`.
pub(super) async fn patch_comment_on<T: DeserializeOwned>(
    ws: &InWorkspace,
    container_id: &str,
    comment: &Block,
    changes_denied: &str,
    raw_body: &Bytes,
    read_at_of: impl FnOnce(&T) -> Option<i64>,
) -> Result<Block, AppError> {
    let user_id = ws.session.user_id.as_str();
    require_block_permission(
        &ws.app,
        user_id,
        comment,
        BlockAction::Change,
        changes_denied,
    )
    .await?;
    let (change, document): (T, _) = decode_with_document(raw_body)?;
    let Some(read_at) = read_at_of(&change) else {
        let mut refused = Refusals::new();
        refused.refuse("readAt", RefusalReason::Required, READ_AT_MISSING);
        return Err(refused.into());
    };
    let text = text_of(document)?;
    ws.app.check_body_files(container_id, Some(&text)).await?;
    let patch = BlockPatch {
        body: BodyChange::Set(text),
        read_at: Some(read_at),
        ..BlockPatch::default()
    };
    ws.app
        .patch_block(&comment.id, &patch, user_id, ChangeNotification::Send)
        .await
}

/// Deletes an already-resolved `comment` block after checking comment delete permission.
pub(super) async fn delete_comment_on(
    ws: &InWorkspace,
    comment: &Block,
    changes_denied: &str,
) -> ApiResult {
    let user_id = ws.session.user_id.as_str();
    require_delete_permission(&ws.app, user_id, comment, changes_denied).await?;
    ws.app
        .delete_block(&comment.id, user_id, ChangeNotification::Send)
        .await?;
    Ok(no_content())
}

/// Restores a comment deleted on its own onto an already-authorized live `parent` block.
pub(super) async fn restore_comment_on(
    ws: &InWorkspace,
    parent: &Block,
    comment_id: &str,
    changes_denied: &str,
) -> Result<Block, AppError> {
    let user_id = ws.session.user_id.as_str();
    let deletion = ws.app.deletion(comment_id).await?;
    let Some((top, below)) = deletion
        .split_first()
        .filter(|(top, _)| top.id == comment_id && is_comment_on(top, parent))
    else {
        return Err(AppError::not_found("comment"));
    };
    let deleted_by = ws.app.deleted_by(&top.id).await?;
    require_deletion_permission(
        &ws.app,
        user_id,
        top,
        below,
        BlockAction::Undelete {
            deleted_by: &deleted_by,
        },
        changes_denied,
    )
    .await?;
    ws.app.undelete_block(comment_id, user_id).await
}

/// Lists prior versions of a live or singly-deleted comment on `parent`.
pub(super) async fn get_comment_versions_on(
    ws: &InWorkspace,
    parent: &Block,
    live: Option<Block>,
    comment_id: &str,
) -> ApiResult {
    let id = match live {
        Some(comment) => comment.id,
        None => {
            ws.app
                .deleted_block(comment_id)
                .await?
                .map(|deleted| deleted.block)
                .filter(|block| is_comment_on(block, parent))
                .ok_or_else(|| AppError::not_found("comment"))?
                .id
        }
    };
    let versions = ws.app.comment_versions(&id).await?;
    Ok(json_response(&versions))
}

pub(super) fn created_comment(comment: &Block) -> axum::response::Response {
    created(comment)
}
