//! Moving a content block within its card.

use axum::extract::{Path, State};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::contract::params::{BlockId, DestinationBlockId, MoveWhere};
use super::contract::tag;

use super::require_permission_through;
use super::{ApiResult, RequiredSession, no_content};
use crate::app::{App, MovePosition};
use crate::permissions::BoardPermission;

/// The refusal for a user who may not manage the cards of a board.
const NO_CARD_CHANGES: &str = "access denied to modify board cards";

pub(super) fn routes() -> OpenApiRouter<App> {
    OpenApiRouter::new().routes(routes!(handle_move_block_to))
}

/// Move a content block within its card
///
/// Moves a content block right before or after another one in the content order of
/// their card. For editors of the board. A block on a board the user may not see is
/// not there, whichever of the two it is.
#[utoipa::path(
    post,
    path = "/content-blocks/{blockId}/moveto/{where}/{dstBlockId}",
    operation_id = "moveContentBlock",
    tag = tag::BLOCKS,
    params(BlockId, MoveWhere, DestinationBlockId),
    responses((status = 204, description = "The block is moved.")),
)]
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
    Ok(no_content())
}
