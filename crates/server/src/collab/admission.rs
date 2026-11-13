//! Who is let into the room of a card's body, and how: asked when a connection
//! comes in, and again while it is there.

use crate::app::{App, Asker};
use crate::error::{AppError, Reason};
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
/// A workspace is entered as a request enters it (`App::enter_workspace`): one whose
/// organization takes only sessions made through its provider lets the connection of
/// another session into no room, as it lets it follow none of its workspaces. Asked
/// again while the connection is there, it is put out once the organization takes
/// none.
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
    asker: Asker<'_>,
    workspace_id: &str,
    card_id: &str,
) -> Result<Option<Admitted>, AppError> {
    let user_id = asker.user_id;
    if user_id.is_empty() {
        return Ok(None);
    }
    let entered = match home.enter_workspace_as(asker, workspace_id).await {
        Ok(Some(entered)) => entered,
        Ok(None) => return Ok(None),
        // Not let in, which is an answer: told as a failure, it would leave who is in
        // the room there when the organization begins to take no password.
        Err(error) if error.reason() == Some(Reason::SsoRequired) => return Ok(None),
        Err(error) => return Err(error),
    };
    // For the workspace the connection named: a board of another is not there for it
    // (`App::reaches_board`), as for a request that names the workspace in its path.
    let app = entered.app.in_workspace(workspace_id);
    // The card's board and nothing else of the card: its body is read once the user
    // is let in, so that a refusal costs the same whatever the body is.
    let board_id = app.store().board_of_card(card_id).await.map_err(|error| {
        AppError::from(error).context("reading a card's board to enter its room")
    })?;
    let mode = app
        .board_access(user_id, board_id.as_deref().unwrap_or_default())
        .await?
        // A template the server ships with is every workspace's to read and nobody's
        // to edit: it has no room.
        .filter(|access| !access.board.is_builtin)
        .and_then(|access| doc_mode(access.grant));
    Ok(board_id.zip(mode).map(|(_, mode)| Admitted { app, mode }))
}
