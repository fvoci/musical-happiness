//! Who is let into the room of a card's body, and how: asked when a connection
//! comes in, and again while it is there.

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
/// there or not ([`App::board_access`]), and of the card only which board it is on
/// (`Store::board_of_card`), never its body: a card that is not there for the user
/// takes as long to refuse as one that does not exist, however large its body is.
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
    // The card's board and nothing else of the card: its body is read once the user
    // is let in, so that a refusal costs the same whatever the body is.
    let board_id = app.store().board_of_card(card_id).await.map_err(|error| {
        AppError::from(error).context("reading a card's board to enter its room")
    })?;
    let mode = app
        .board_access(user_id, board_id.as_deref().unwrap_or_default())
        .await?
        .filter(|access| access.board.workspace_id == workspace_id)
        .and_then(|access| doc_mode(access.grant));
    Ok(board_id.zip(mode).map(|(_, mode)| Admitted { app, mode }))
}
