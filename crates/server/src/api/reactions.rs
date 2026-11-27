//! Reactions: an emoji that someone who may comment in a board, a wiki or a course
//! answers one of its comments, cards, pages or units with, and takes off again. The
//! lists of comments and the reads of a card, a page and a unit give them
//! (`CommentWithReactions` and the others); the container's members are told of them
//! as they change (`reactionsChanged`). A public link's reader is given none.

use axum::extract::Path;
use fvoci_model::{Block, BlockType, ContainerKinds, Emoji};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::contract::params::{
    MemberWorkspaceId, ReactedBlockId, ReactedContainerId, ReactionEmoji,
};
use super::contract::tag;
use super::workspace::{InContainer, InWorkspace};
use super::{ApiResult, Delegated, Write, no_content, require_container_permission};
use crate::app::App;
use crate::error::AppError;
use crate::permissions::ContainerPermission;

pub(super) fn routes() -> OpenApiRouter<App> {
    OpenApiRouter::new().routes(routes!(handle_add_reaction, handle_remove_reaction))
}

const DENIED: &str = "access denied to react in the container";

/// The types of the blocks that are answered with reactions.
const REACTED_TO: [BlockType; 4] = [
    BlockType::Comment,
    BlockType::Card,
    BlockType::Page,
    BlockType::Unit,
];

/// React to a comment, a card, a page or a unit
///
/// Adds the user's reaction with the emoji to the block: a comment, a card, a page or a
/// unit of a board, a wiki or a course. For its members from the commenter's role up
/// (whoever reads an open board without joining it is refused with 403, as a viewer
/// is); a diary's entries are their owner's alone, and a diary is not there (404). One
/// that is there already is no error, and nothing changes. A block has at most 20
/// different emoji: one more is 400 with the reason `too_many_reactions` and the
/// `limit`, and nothing changes; anybody's emoji that is there is taken. Neither the
/// block's `updatedAt` nor who changed it last changes, and nobody is notified; the
/// members who read the container are told the block's reactions as they are now
/// (`reactionsChanged`).
#[utoipa::path(
    put,
    path = "/w/{workspaceId}/containers/{containerId}/blocks/{blockId}/reactions/{emoji}",
    operation_id = "addReaction",
    tag = tag::CONTAINERS,
    params(MemberWorkspaceId, ReactedContainerId, ReactedBlockId, ReactionEmoji),
    responses((status = 204, description = "The reaction is there.")),
)]
async fn handle_add_reaction(
    Delegated(InContainer(ws), ..): Delegated<Write, InContainer>,
    Path((_, container_id, block_id, emoji)): Path<(String, String, String, String)>,
) -> ApiResult {
    let (ws, block, emoji) = reacted(ws, &container_id, &block_id, &emoji).await?;
    ws.app
        .add_reaction(&ws.workspace_id, &block, &ws.caller.user_id, &emoji)
        .await?;
    Ok(no_content())
}

/// Take a reaction off
///
/// Takes the user's reaction with the emoji off the block, for whom `addReaction`
/// takes it from. One that is not there is no error, and nothing changes. As adding
/// one, nothing of the block changes, and the members who read the container are told
/// its reactions as they are now (`reactionsChanged`).
#[utoipa::path(
    delete,
    path = "/w/{workspaceId}/containers/{containerId}/blocks/{blockId}/reactions/{emoji}",
    operation_id = "removeReaction",
    tag = tag::CONTAINERS,
    params(MemberWorkspaceId, ReactedContainerId, ReactedBlockId, ReactionEmoji),
    responses((status = 204, description = "The reaction is not there.")),
)]
async fn handle_remove_reaction(
    Delegated(InContainer(ws), ..): Delegated<Write, InContainer>,
    Path((_, container_id, block_id, emoji)): Path<(String, String, String, String)>,
) -> ApiResult {
    let (ws, block, emoji) = reacted(ws, &container_id, &block_id, &emoji).await?;
    ws.app
        .remove_reaction(&ws.workspace_id, &block, &ws.caller.user_id, &emoji)
        .await?;
    Ok(no_content())
}

/// The workspace as one who reacts reaches it, the block of the path, and the emoji:
/// one who may comment in the container of the path, which is no diary, on a block of
/// it that is answered with reactions.
///
/// # Errors
///
/// Returns 400 for an emoji that is none, what `require_container_permission`
/// refuses with, and 404 for a block that is not there.
async fn reacted(
    ws: InWorkspace,
    container_id: &str,
    block_id: &str,
    emoji: &str,
) -> Result<(InWorkspace, Block, Emoji), AppError> {
    let emoji: Emoji = emoji
        .parse()
        .map_err(|error| AppError::bad_request(error).with_parameter("emoji"))?;
    let ws = ws.reaching(ContainerKinds::SHARED);
    require_container_permission(
        &ws.app,
        &ws.caller.user_id,
        container_id,
        ContainerPermission::CommentCards,
        DENIED,
    )
    .await?;
    let block = match ws.app.block(block_id).await {
        Ok(block) => block,
        Err(error) if error.is_not_found() => return Err(AppError::not_found("block")),
        Err(error) => return Err(error),
    };
    if block.container_id != container_id || !REACTED_TO.contains(&block.block_type) {
        return Err(AppError::not_found("block"));
    }
    Ok((ws, block, emoji))
}
