//! What the websocket hub asks about boards, to know whom a change is told to and how
//! much of it.

use fvoci_model::{BoardMember, PersonProperties};
use fvoci_store::{Store, StoreError};

/// The boards as the websocket hub asks about them: who the members of a board are,
/// which of its properties name users, and which board a block is of. The hub is made
/// before the [`App`](super::App) that holds it, and so asks here.
///
/// An error is the store's own, for the hub's log; none of it is sent to a client.
#[derive(Debug, Clone)]
pub struct Audience {
    store: Store,
}

impl Audience {
    /// The audience of the boards in `store`.
    pub fn new(store: Store) -> Self {
        Self { store }
    }

    /// The identifier of the board the block belongs to.
    ///
    /// # Errors
    ///
    /// Returns the store's error if there is no such block, or the query fails.
    pub async fn board_of_block(&self, block_id: &str) -> Result<String, StoreError> {
        Ok(self.store.get_block(block_id).await?.board_id)
    }

    /// Whether the user works in the workspace: a member of it who is no guest, as the
    /// routes of a workspace ask it (`App::works_in_workspace`). A membership that
    /// cannot be read is no membership here: the connection is not subscribed.
    pub async fn works_in_workspace(&self, user_id: &str, workspace_id: &str) -> bool {
        match self.store.resolve_workspace(user_id, workspace_id).await {
            Ok(access) => access.is_some_and(|access| access.works_there()),
            Err(error) => {
                tracing::error!(%error, workspace_id, "websocket: reading a workspace's membership failed");
                false
            }
        }
    }

    /// The members of the board.
    ///
    /// # Errors
    ///
    /// Returns the store's error if the query fails.
    pub async fn members(&self, board_id: &str) -> Result<Vec<BoardMember>, StoreError> {
        self.store.get_members_for_board(board_id).await
    }

    /// The properties of the board's cards whose values name users.
    ///
    /// # Errors
    ///
    /// Returns the store's error if there is no such board, or it cannot be read.
    pub async fn person_properties(&self, board_id: &str) -> Result<PersonProperties, StoreError> {
        Ok(self.store.get_board(board_id).await?.person_properties())
    }
}
