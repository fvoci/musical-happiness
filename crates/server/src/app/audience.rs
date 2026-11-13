//! What the websocket hub asks about boards, to know whom a change is told to and how
//! much of it.

mod cache;

use std::sync::Arc;

use cache::Cache;
use fvoci_model::{BoardMember, PersonProperties, WorkspaceRole};
use fvoci_store::{Store, StoreError, Tenant, VisibilityVersion};

/// The boards as the websocket hub asks about them: who the members of a board are,
/// which of its properties name users, and which board a block is of. The hub is made
/// before the [`App`](super::App) that holds it, and so asks here.
///
/// An error is the store's own, for the hub's log; none of it is sent to a client.
#[derive(Debug, Clone)]
pub struct Audience {
    store: Store,
    cache: Arc<Caches>,
}

#[derive(Debug, Default)]
struct Caches {
    workspaces: Cache<(String, String), Option<WorkspaceRole>>,
    members: Cache<(Tenant, String), Vec<BoardMember>>,
    people: Cache<(Tenant, String), PersonProperties>,
    shares: Cache<(String, String), bool>,
}

impl Audience {
    /// The audience of the boards in `store`.
    pub fn new(store: Store) -> Self {
        Self {
            store,
            cache: Arc::default(),
        }
    }

    pub(super) fn with_store(&self, store: Store) -> Self {
        Self {
            store,
            cache: Arc::clone(&self.cache),
        }
    }

    /// The committed revision used to validate cached dispatch permissions.
    pub async fn version(&self) -> VisibilityVersion {
        self.store.visibility_version().await
    }

    /// Queues a checked dispatch atomically with respect to authorization writes.
    pub fn if_current(&self, version: VisibilityVersion, send: impl FnOnce()) -> bool {
        self.store.if_visibility_current(version, send)
    }

    /// Whether a previously admitted public subscription still has a valid token.
    pub async fn shared_board(&self, board_id: &str, read_token: &str) -> bool {
        self.cache
            .shares
            .read(
                &self.store,
                (board_id.to_owned(), read_token.to_owned()),
                || async {
                    Ok(self
                        .store
                        .org_of_shared_board(board_id, read_token)
                        .await?
                        .is_some())
                },
            )
            .await
            .unwrap_or(false)
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
        match self
            .cache
            .workspaces
            .read(
                &self.store,
                (user_id.to_owned(), workspace_id.to_owned()),
                || async {
                    Ok(self
                        .store
                        .resolve_workspace(user_id, workspace_id)
                        .await?
                        .map(|access| access.workspace_role))
                },
            )
            .await
        {
            Ok(role) => role,
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
        self.cache
            .members
            .read(
                &self.store,
                (self.store.tenant(), board_id.to_owned()),
                || async { self.store.get_present_members_for_board(board_id).await },
            )
            .await
    }

    /// The properties of the board's cards whose values name users.
    ///
    /// # Errors
    ///
    /// Returns the store's error if there is no such board, or it cannot be read.
    pub async fn person_properties(&self, board_id: &str) -> Result<PersonProperties, StoreError> {
        self.cache
            .people
            .read(
                &self.store,
                (self.store.tenant(), board_id.to_owned()),
                || async { Ok(self.store.get_board(board_id).await?.person_properties()) },
            )
            .await
    }
}
