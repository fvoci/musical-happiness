//! Moving a content block within its card.

use axum::Router;
use axum::extract::{Path, State};
use axum::routing::post;

use super::require_board_permission;
use super::{ApiResult, RequiredSession, empty_response};
use crate::app::{App, MovePosition};
use crate::permissions::BoardPermission;

/// The refusal for a user who may not manage the cards of a board.
const NO_CARD_CHANGES: &str = "access denied to modify board cards";

pub(super) fn routes() -> Router<App> {
    Router::new().route(
        "/content-blocks/{block_id}/moveto/{where}/{dst_block_id}",
        post(handle_move_block_to),
    )
}

/// `POST /content-blocks/{block_id}/moveto/{where}/{dst_block_id}`: moves a content
/// block right `before` or `after` another one in the content order of their card.
///
/// As in the original, both blocks are looked up first (404 for either, for every
/// signed-in user), then `where` is checked, and then the permission to manage the
/// cards of the moved block's board.
async fn handle_move_block_to(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    Path((block_id, position, dst_block_id)): Path<(String, String, String)>,
) -> ApiResult {
    let user_id = session.user_id.as_str();
    let block = app.block(&block_id).await?;
    let dst_block = app.block(&dst_block_id).await?;
    let position: MovePosition = position.parse()?;

    require_board_permission(
        &app,
        user_id,
        &block.board_id,
        BoardPermission::ManageBoardCards,
        NO_CARD_CHANGES,
    )
    .await?;
    // The original asks nothing about the destination, and then names the
    // destination's parent in its refusal of a move between parents
    // (server/app/content_blocks.go:11-14): anyone who may change one block anywhere
    // learns the parent of any block they know the identifier of. A destination on
    // another board takes the same permission there. Blocks of one card are on one
    // board, so a real move never comes here.
    if dst_block.board_id != block.board_id {
        require_board_permission(
            &app,
            user_id,
            &dst_block.board_id,
            BoardPermission::ManageBoardCards,
            NO_CARD_CHANGES,
        )
        .await?;
    }

    app.move_content_block(&block, &dst_block, position, user_id)
        .await?;
    tracing::debug!(block_id, dst_block_id, "MoveBlockTo");
    Ok(empty_response())
}
