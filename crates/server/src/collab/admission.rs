//! Who is let into the room of a card's body, and how: asked when a connection
//! comes in, and again while it is there.

use fvoci_model::BlockType;

use crate::app::App;
use crate::error::AppError;
use crate::permissions::{DocMode, doc_mode};

/// A user let into the room of a card.
#[derive(Debug, Clone)]
pub(super) struct Admitted {
    /// The application as the card's organization reaches it.
    pub(super) app: App,
    /// How the user is in the room.
    pub(super) mode: DocMode,
}

/// Lets the user into the room of the card `card_id` of a board of the workspace
/// `workspace_id`, as what they may do on that board says ([`doc_mode`]); `None` for
/// who may not read the board, for a card that is not there, or not on a board of
/// that workspace, and for a connection without a user. `home` is the application as
/// the connection's handshake reached it.
///
/// Whoever is in the workspace is asked the same of the store whether the card is
/// there or not ([`App::board_access`]), so that a card that is not there for the
/// user takes as long to refuse as one that does not exist.
///
/// # Errors
///
/// Returns the store's error if a fact cannot be read: that is no "no".
pub(super) async fn admit(
    home: &App,
    user_id: &str,
    workspace_id: &str,
    card_id: &str,
) -> Result<Option<Admitted>, AppError> {
    if user_id.is_empty() {
        return Ok(None);
    }
    let Some(entered) = home.enter_workspace(user_id, workspace_id).await? else {
        return Ok(None);
    };
    let app = entered.app;
    let card = match app.store().get_block(card_id).await {
        Ok(block) => (block.block_type == BlockType::Card).then_some(block),
        Err(error) if error.is_not_found() => None,
        Err(error) => return Err(AppError::from(error).context("reading a card to enter its room")),
    };
    let board_id = card.as_ref().map_or("", |card| card.board_id.as_str());
    let mode = app
        .board_access(user_id, board_id)
        .await?
        .filter(|access| access.board.workspace_id == workspace_id)
        .and_then(|access| doc_mode(access.grant));
    Ok(card.zip(mode).map(|(_, mode)| Admitted { app, mode }))
}
