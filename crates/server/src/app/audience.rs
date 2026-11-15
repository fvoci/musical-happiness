//! What the websocket hub asks about boards, to know whom a change is told to and how
//! much of it.

mod cache;

use std::collections::HashSet;
use std::sync::Arc;

use cache::Cache;
use fvoci_model::{Block, BoardMember, Body, PersonProperties};
use fvoci_store::{Store, StoreError, Tenant, VisibilityVersion, WorkspaceAccess};

use crate::permissions::BoardScope;

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
    workspaces: Cache<(String, String), Option<WorkspaceAccess>>,
    members: Cache<(Tenant, String), Vec<BoardMember>>,
    people: Cache<(Tenant, String), PersonProperties>,
    scopes: Cache<(Tenant, String), Option<(String, BoardScope)>>,
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

    /// The organization this audience asks about, if its handle names one.
    pub fn org_id(&self) -> Option<String> {
        self.store.tenant().org_id()
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
    ///
    /// # Errors
    ///
    /// Returns the store's error if the sharing cannot be read: that is no "no".
    pub async fn shared_board(&self, board_id: &str, read_token: &str) -> Result<bool, StoreError> {
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

    /// What the user is in the workspace, a guest too (`App::actor`), with the provider
    /// a session must be made through to open the workspace's organization; `None` for
    /// who is no member. What a subscription of the workspace is told of a board goes
    /// to the board's members only, so a guest hears the boards they were invited to
    /// and no other.
    ///
    /// Cached at the visibility revision of this audience's organization, which a
    /// change of a membership or of the organization's provider moves on. So the
    /// workspace must be of that organization: the one of the change being sent,
    /// which is written through this audience's store. A workspace of any organization
    /// is asked with [`Self::current_workspace_access`].
    ///
    /// # Errors
    ///
    /// Returns the store's error if the membership cannot be read: that is no "not a
    /// member".
    pub async fn workspace_access(
        &self,
        user_id: &str,
        workspace_id: &str,
    ) -> Result<Option<WorkspaceAccess>, StoreError> {
        self.cache
            .workspaces
            .read(
                &self.store,
                (user_id.to_owned(), workspace_id.to_owned()),
                || async { self.store.resolve_workspace(user_id, workspace_id).await },
            )
            .await
    }

    /// As [`Self::workspace_access`], read from the store and not cached: for what is
    /// asked of a workspace of any organization (a subscription, what is left of a
    /// user's after their roles change), which no one organization's revision covers.
    ///
    /// # Errors
    ///
    /// As [`Self::workspace_access`].
    pub async fn current_workspace_access(
        &self,
        user_id: &str,
        workspace_id: &str,
    ) -> Result<Option<WorkspaceAccess>, StoreError> {
        self.store.resolve_workspace(user_id, workspace_id).await
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
                || async { self.store.effective_board_members(board_id).await },
            )
            .await
    }

    /// Who the board is there for besides its members ([`BoardScope::of`]), if it is a
    /// board of the workspace `workspace_id`; `None` for a board of another workspace,
    /// and for one that is not there, in the trash among them.
    ///
    /// # Errors
    ///
    /// Returns the store's error if the board cannot be read.
    pub async fn board_scope(
        &self,
        workspace_id: &str,
        board_id: &str,
    ) -> Result<Option<BoardScope>, StoreError> {
        let placed = self
            .cache
            .scopes
            .read(
                &self.store,
                (self.store.tenant(), board_id.to_owned()),
                || async {
                    match self.store.get_board(board_id).await {
                        Ok(board) => Ok(Some((board.workspace_id.clone(), BoardScope::of(&board)))),
                        Err(error) if error.is_not_found() => Ok(None),
                        Err(error) => Err(error),
                    }
                },
            )
            .await?;
        Ok(placed
            .filter(|(of, _)| of == workspace_id)
            .map(|(_, scope)| scope))
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

    /// What whoever reads the block's board by its link is not told of the block: the
    /// person properties of the board ([`Self::person_properties`]), and the cards of
    /// the board among what the block's body names, which are all that reader is shown
    /// of the references of the body ([`Block::clear_references_but`]). The cards are read
    /// anew each time, and only for a body that names something: a card that comes or
    /// goes changes no version of the audience.
    ///
    /// # Errors
    ///
    /// Returns the store's error if either cannot be read: the block is not sent then.
    pub async fn unsaid_by_link(&self, block: &Block) -> Result<UnsaidByLink, StoreError> {
        let people = self.person_properties(&block.board_id).await?;
        let named = block
            .body
            .as_ref()
            .map(Body::reference_ids)
            .unwrap_or_default();
        let shown = self
            .store
            .cards_on_board_among(&block.board_id, &named)
            .await?;
        Ok(UnsaidByLink { people, shown })
    }
}

/// Who read a board that is gone: its members when it went, and who it was there for
/// besides them (an open wiki is read by whoever works in its workspace), of the
/// workspace the change is told in.
#[derive(Debug, Clone, Copy)]
pub struct Former<'a> {
    /// The board's members, as they were.
    pub members: &'a [BoardMember],
    /// Who the board was there for besides its members.
    pub scope: BoardScope,
}

/// What a block is sent without to whoever reads its board by the link alone
/// ([`Audience::unsaid_by_link`]).
#[derive(Debug)]
pub struct UnsaidByLink {
    /// The person properties of the board, whose values go out empty.
    pub people: PersonProperties,
    /// The cards of the board among what the body names: every other reference goes
    /// out without its identifier.
    pub shown: HashSet<String>,
}
