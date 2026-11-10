//! Moving a content block within its card.

use axum::Router;
use axum::extract::{Path, State};
use axum::routing::post;

use super::require_permission_through;
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
/// The path names two blocks and no board. A block on a board the user may not read
/// does not exist for them, whichever of the two it is.
async fn handle_move_block_to(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    Path((block_id, position, dst_block_id)): Path<(String, String, String)>,
) -> ApiResult {
    const WHAT: &str = "block";
    let user_id = session.user_id.as_str();
    let block = app.block(&block_id).await?;
    require_permission_through(
        &app,
        user_id,
        &block.board_id,
        BoardPermission::ManageBoardCards,
        NO_CARD_CHANGES,
        WHAT,
    )
    .await?;
    let dst_block = app.block(&dst_block_id).await?;
    if dst_block.board_id != block.board_id {
        // Blocks of one card are on one board, so a real move never comes here. The
        // destination is checked like the block itself, so that nothing is said about a
        // block of a board the user may not read.
        require_permission_through(
            &app,
            user_id,
            &dst_block.board_id,
            BoardPermission::ManageBoardCards,
            NO_CARD_CHANGES,
            WHAT,
        )
        .await?;
    }
    let position: MovePosition = position.parse()?;

    app.move_content_block(&block, &dst_block, position, user_id)
        .await?;
    tracing::debug!(block_id, dst_block_id, "MoveBlockTo");
    Ok(empty_response())
}
