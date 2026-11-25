//! The comments on a page of a wiki: listing, writing, changing and deleting them, and
//! what one said before.
//!
//! A comment on a page is a block of the type `comment` below the page, as a comment on
//! a card is below the card. Who may do what to one is what the card's comments let
//! (`api/access.rs`), and what is stored and told is what the card's are (`App`'s
//! blocks); only the routes are a wiki's, under the handle that reaches wikis
//! (`InWiki`), so that no operation of a board's blocks reaches a page's comment.

use axum::body::Bytes;
use axum::extract::Path;
use fvoci_model::{Block, BlockPatch, BlockType, Body, BodyChange, CommentVersion};
use fvoci_model::{RefusalReason, Refusals, new_id};
use serde::Deserialize;
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::contract::params::{CommentId, MemberWorkspaceId, PageId, WikiId};
use super::contract::tag;
use super::wikis::{READ_DENIED, page_with_permission};
use super::workspace::{InWiki, InWorkspace};
use super::{
    ApiResult, BlockAction, created, decode_with_document, json_response, no_content,
    require_block_permission, require_delete_permission, require_deletion_permission,
};
use crate::app::{App, ChangeNotification};
use crate::error::AppError;
use crate::permissions::ContainerPermission;

pub(super) fn routes() -> OpenApiRouter<App> {
    OpenApiRouter::new()
        .routes(routes!(
            handle_get_page_comments,
            handle_create_page_comment
        ))
        .routes(routes!(
            handle_patch_page_comment,
            handle_delete_page_comment
        ))
        .routes(routes!(handle_get_page_comment_versions))
        .routes(routes!(handle_undelete_page_comment))
}

/// What a comment is changed by, and what a comment is refused with that a request does
/// not change.
const CHANGES_DENIED: &str = "access denied to change the comments of the page";

/// What a comment is refused with that has no text.
const TEXT_MISSING: &str = "a comment has a text";

/// What a change of a comment is refused with that does not say what it was read at.
const READ_AT_MISSING: &str =
    "a comment is changed with readAt, the updateAt of the comment as it was read";

/// A comment to write on a page.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct NewPageComment {
    /// The comment's text, as a page's body is written; neither empty nor `null`.
    #[serde(default)]
    #[schema(value_type = Body)]
    body: BodyChange,
}

/// What a comment on a page is changed to: its text, written over the comment as it was
/// read.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PageCommentPatch {
    /// The comment's new text, as a page's body is written; neither empty nor `null`.
    #[serde(default)]
    #[schema(value_type = Body)]
    body: BodyChange,
    /// The `updateAt` of the comment as the client read it: if the comment has changed
    /// since, the patch is refused with 409 and changes nothing.
    #[schema(value_type = i64)]
    read_at: Option<i64>,
}

/// The comment a route of `…/comments/{commentId}` names, and the page and the wiki it
/// is named under.
#[derive(Debug, Clone, Copy)]
struct CommentPath<'a> {
    wiki: &'a str,
    page: &'a str,
    comment: &'a str,
}

/// The text of a comment as a request sends it: refused at `/body` unless it is one.
fn text_of(change: BodyChange) -> Result<Body, AppError> {
    match change {
        BodyChange::Set(body) => Ok(body),
        BodyChange::Keep | BodyChange::Clear => {
            let mut refused = Refusals::new();
            refused.refuse("body", RefusalReason::Required, TEXT_MISSING);
            Err(refused.into())
        }
    }
}

/// Returns the comment of the page for a handler of `…/comments/{commentId}`, if the
/// user has the permission on the wiki, with the page: their blocks, read once each.
/// Who may not do that to the page is told what [`page_with_permission`] tells, with
/// `denial`; a block that is no comment on this page is no comment, whatever else it
/// is. `None` is no comment there now.
async fn comment_with_permission(
    ws: &InWorkspace,
    named: CommentPath<'_>,
    permission: ContainerPermission,
    denial: &str,
) -> Result<(Block, Option<Block>), AppError> {
    let page = page_with_permission(ws, named.wiki, named.page, permission, denial).await?;
    match ws.app.block(named.comment).await {
        Ok(block) if is_comment_on(&block, &page) => Ok((page, Some(block))),
        Ok(_) => Ok((page, None)),
        Err(error) if error.is_not_found() => Ok((page, None)),
        Err(error) => Err(error),
    }
}

/// The comment of [`comment_with_permission`], which is to be there: 404 for none.
async fn existing_comment(
    ws: &InWorkspace,
    named: CommentPath<'_>,
    permission: ContainerPermission,
) -> Result<Block, AppError> {
    let (_, comment) = comment_with_permission(ws, named, permission, CHANGES_DENIED).await?;
    comment.ok_or_else(|| AppError::not_found("comment"))
}

/// Whether the block is a comment on the page.
fn is_comment_on(block: &Block, page: &Block) -> bool {
    block.block_type == BlockType::Comment
        && block.container_id == page.container_id
        && block.parent_id == page.id
}

/// The comments on a page
///
/// Every comment on the page, oldest first, as blocks of the `type` `comment` whose
/// parent is the page; each says who wrote it (`createdBy`), and one whose text was
/// changed when that last happened (`editedAt`). For who reads the wiki: its members,
/// and of an open wiki whoever works in the workspace. A page in the trash is not
/// there (404).
#[utoipa::path(
    get,
    path = "/w/{workspaceId}/wikis/{wikiId}/pages/{pageId}/comments",
    operation_id = "getPageComments",
    tag = tag::WIKIS,
    params(MemberWorkspaceId, WikiId, PageId),
    responses((status = 200, description = "The comments, oldest first.", body = Vec<Block>)),
)]
async fn handle_get_page_comments(
    InWiki(ws): InWiki,
    Path((_, wiki_id, page_id)): Path<(String, String, String)>,
) -> ApiResult {
    let page = page_with_permission(
        &ws,
        &wiki_id,
        &page_id,
        ContainerPermission::View,
        READ_DENIED,
    )
    .await?;
    let mut comments = ws
        .app
        .blocks(&wiki_id, Some(&page.id), Some(&BlockType::Comment))
        .await?;
    comments.sort_by(|one, other| (one.create_at, &one.id).cmp(&(other.create_at, &other.id)));
    Ok(json_response(&comments))
}

/// Write a comment on a page
///
/// For the wiki's members from the commenter's role up. The server chooses the
/// identifier, the author and the times. Every image of the text shows a file of the
/// wiki (400 otherwise), and the text has no file block (400). Who reads the wiki is
/// told with `blockChanged`, as of a comment on a card.
#[utoipa::path(
    post,
    path = "/w/{workspaceId}/wikis/{wikiId}/pages/{pageId}/comments",
    operation_id = "createPageComment",
    tag = tag::WIKIS,
    params(MemberWorkspaceId, WikiId, PageId),
    request_body = NewPageComment,
    responses((status = 201, description = "The comment as it was stored.", body = Block)),
)]
async fn handle_create_page_comment(
    InWiki(ws): InWiki,
    Path((_, wiki_id, page_id)): Path<(String, String, String)>,
    body: Bytes,
) -> ApiResult {
    let user_id = ws.session.user_id.as_str();
    let page = page_with_permission(
        &ws,
        &wiki_id,
        &page_id,
        ContainerPermission::CommentCards,
        "access denied to comment on the page",
    )
    .await?;
    let (mut new, document): (NewPageComment, _) = decode_with_document(&body)?;
    new.body = document;
    let text = text_of(new.body)?;
    ws.app.check_body_files(&wiki_id, Some(&text)).await?;
    let mut comment = Block {
        id: new_id(),
        container_id: wiki_id.clone(),
        parent_id: page.id,
        block_type: BlockType::Comment,
        body: Some(text),
        ..Block::default()
    };
    let stored = ws
        .app
        .insert_block(&mut comment, user_id, ChangeNotification::Send)
        .await;
    if let Err(error) = stored {
        // The page went to the trash since it was read: the comment has nowhere to go.
        return Err(match ws.app.block(&comment.parent_id).await {
            Err(gone) if gone.is_not_found() => AppError::not_found("page"),
            _ => error,
        });
    }
    tracing::debug!(wiki_id, page_id, comment_id = %comment.id, user_id, "CreatePageComment");
    Ok(created(&comment))
}

/// Change a comment on a page
///
/// Writes a new text over the comment, as the card's comments are changed: by its
/// author from the commenter's role up, and the comment of somebody else by the wiki's
/// admins only (403, `access denied to change other users' comments`); an editor may
/// not change it. The text it had is kept (`getPageCommentVersions`), and the comment's
/// `editedAt` says when it last changed. The text has no file block (400).
#[utoipa::path(
    patch,
    path = "/w/{workspaceId}/wikis/{wikiId}/pages/{pageId}/comments/{commentId}",
    operation_id = "patchPageComment",
    tag = tag::WIKIS,
    params(MemberWorkspaceId, WikiId, PageId, CommentId),
    request_body = PageCommentPatch,
    responses((status = 200, description = "The comment as it is now.", body = Block)),
)]
async fn handle_patch_page_comment(
    InWiki(ws): InWiki,
    Path((_, wiki_id, page_id, comment_id)): Path<(String, String, String, String)>,
    body: Bytes,
) -> ApiResult {
    let user_id = ws.session.user_id.as_str();
    let named = CommentPath {
        wiki: &wiki_id,
        page: &page_id,
        comment: &comment_id,
    };
    // Who cannot even comment changes nothing, whatever the comment.
    let comment = existing_comment(&ws, named, ContainerPermission::CommentCards).await?;
    require_block_permission(
        &ws.app,
        user_id,
        &comment,
        BlockAction::Change,
        CHANGES_DENIED,
    )
    .await?;
    let (mut change, document): (PageCommentPatch, _) = decode_with_document(&body)?;
    change.body = document;
    let Some(read_at) = change.read_at else {
        let mut refused = Refusals::new();
        refused.refuse("readAt", RefusalReason::Required, READ_AT_MISSING);
        return Err(refused.into());
    };
    let text = text_of(change.body)?;
    ws.app.check_body_files(&wiki_id, Some(&text)).await?;
    let patch = BlockPatch {
        body: BodyChange::Set(text),
        read_at: Some(read_at),
        ..BlockPatch::default()
    };
    let patched = ws
        .app
        .patch_block(&comment.id, &patch, user_id, ChangeNotification::Send)
        .await?;
    tracing::debug!(wiki_id, page_id, comment_id, user_id, "PatchPageComment");
    Ok(json_response(&patched))
}

/// Delete a comment on a page
///
/// The comment goes to the trash, as a comment on a card does: by its author from the
/// commenter's role up, and the comment of somebody else by the wiki's admins only
/// (403, `access denied to delete other users' comments`). Who reads the wiki is told
/// with `blockChanged` with `deletedAt` set.
#[utoipa::path(
    delete,
    path = "/w/{workspaceId}/wikis/{wikiId}/pages/{pageId}/comments/{commentId}",
    operation_id = "deletePageComment",
    tag = tag::WIKIS,
    params(MemberWorkspaceId, WikiId, PageId, CommentId),
    responses((status = 204, description = "The comment is deleted.")),
)]
async fn handle_delete_page_comment(
    InWiki(ws): InWiki,
    Path((_, wiki_id, page_id, comment_id)): Path<(String, String, String, String)>,
) -> ApiResult {
    let user_id = ws.session.user_id.as_str();
    let named = CommentPath {
        wiki: &wiki_id,
        page: &page_id,
        comment: &comment_id,
    };
    let comment = existing_comment(&ws, named, ContainerPermission::CommentCards).await?;
    require_delete_permission(&ws.app, user_id, &comment, CHANGES_DENIED).await?;
    ws.app
        .delete_block(&comment.id, user_id, ChangeNotification::Send)
        .await?;
    tracing::debug!(wiki_id, page_id, comment_id, user_id, "DeletePageComment");
    Ok(no_content())
}

/// Bring a deleted comment on a page back
///
/// A comment deleted on its own comes back onto its page, as `undeleteBlock` brings a
/// card's comment back: for its author from the commenter's role up if they deleted it
/// themselves, and for the wiki's admins whoever wrote or deleted it (403, `access denied
/// to undelete other users' comments`, and for the author of a comment somebody else
/// deleted, `a comment somebody else deleted is brought back by the board's admins`). A
/// comment that went to the trash with its page comes back with the page
/// (`undeletePage`), not on its own; that one, one not in the trash, and one of another
/// page are not there (404), nor is any while the page is in the trash. Who reads the
/// wiki is told with `blockChanged`.
#[utoipa::path(
    post,
    path = "/w/{workspaceId}/wikis/{wikiId}/pages/{pageId}/comments/{commentId}/undelete",
    operation_id = "undeletePageComment",
    tag = tag::WIKIS,
    params(MemberWorkspaceId, WikiId, PageId, CommentId),
    responses((status = 200, description = "The comment as it is again.", body = Block)),
)]
async fn handle_undelete_page_comment(
    InWiki(ws): InWiki,
    Path((_, wiki_id, page_id, comment_id)): Path<(String, String, String, String)>,
) -> ApiResult {
    let user_id = ws.session.user_id.as_str();
    // Nobody below the commenter's role brings anything back, whatever the comment.
    let page = page_with_permission(
        &ws,
        &wiki_id,
        &page_id,
        ContainerPermission::CommentCards,
        CHANGES_DENIED,
    )
    .await?;
    // What the trash has of this page: a comment deleted on its own, at the top of its
    // deletion.
    let deletion = ws.app.deletion(&comment_id).await?;
    let Some((top, below)) = deletion
        .split_first()
        .filter(|(top, _)| top.id == comment_id && is_comment_on(top, &page))
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
        CHANGES_DENIED,
    )
    .await?;
    let comment = ws.app.undelete_block(&comment_id, user_id).await?;
    tracing::debug!(wiki_id, page_id, comment_id, user_id, "UndeletePageComment");
    Ok(json_response(&comment))
}

/// What a comment on a page said before
///
/// The earlier texts of the comment, as `getCommentVersions` gives a card's comment's:
/// every version but the one it has now, the first one first, with who wrote it and
/// when. For who reads the wiki (as `getPageComments`), also of a comment in the trash
/// while its page is there: the versions go only when the comment is gone for good. A
/// page in the trash is not there (404).
#[utoipa::path(
    get,
    path = "/w/{workspaceId}/wikis/{wikiId}/pages/{pageId}/comments/{commentId}/versions",
    operation_id = "getPageCommentVersions",
    tag = tag::WIKIS,
    params(MemberWorkspaceId, WikiId, PageId, CommentId),
    responses((status = 200, description = "The earlier versions, the first one first.", body = Vec<CommentVersion>)),
)]
async fn handle_get_page_comment_versions(
    InWiki(ws): InWiki,
    Path((_, wiki_id, page_id, comment_id)): Path<(String, String, String, String)>,
) -> ApiResult {
    let named = CommentPath {
        wiki: &wiki_id,
        page: &page_id,
        comment: &comment_id,
    };
    let (page, live) =
        comment_with_permission(&ws, named, ContainerPermission::View, READ_DENIED).await?;
    let comment = match live {
        Some(comment) => comment.id,
        // A comment that was deleted on its own is in the trash below a page that is not.
        None => {
            ws.app
                .deleted_block(&comment_id)
                .await?
                .map(|deleted| deleted.block)
                .filter(|block| is_comment_on(block, &page))
                .ok_or_else(|| AppError::not_found("comment"))?
                .id
        }
    };
    let versions = ws.app.comment_versions(&comment).await?;
    Ok(json_response(&versions))
}
