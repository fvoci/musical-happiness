//! What the websocket hub asks about boards, to know whom a change is told to and how
//! much of it.

use fvoci_model::{BoardMember, PersonProperties, WorkspaceRole};
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

    /// The board of a block in the named workspace, if its public read token opens it.
    ///
    /// # Errors
    ///
    /// Returns the store's error if either lookup fails.
    pub async fn board_of_shared_block(
        &self,
        workspace_id: &str,
        block_id: &str,
        read_token: &str,
    ) -> Result<Option<String>, StoreError> {
        self.store
            .board_of_shared_block(workspace_id, block_id, read_token)
            .await
    }

    /// What the user is in the workspace, a guest too (`App::actor`). What
    /// a subscription of the workspace is told of a board goes to the board's members
    /// only, so a guest hears the boards they were invited to and no other. A
    /// membership that cannot be read is no membership here: the connection is not
    /// subscribed.
    pub async fn workspace_role(&self, user_id: &str, workspace_id: &str) -> Option<WorkspaceRole> {
        match self.store.resolve_workspace(user_id, workspace_id).await {
            Ok(access) => access.map(|access| access.workspace_role),
            Err(error) => {
                tracing::error!(%error, workspace_id, "websocket: reading a workspace's membership failed");
                None
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
