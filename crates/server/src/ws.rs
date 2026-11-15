//! The websocket endpoint `/ws`: clients subscribe to a workspace or to blocks and are told
//! about changes as they happen.
//!
//! Whose connection it is is settled at the handshake, from the session the request
//! carries; there is no message to sign in with. A user's connection may subscribe to
//! workspaces. `SUBSCRIBE_BLOCKS` works without a session but needs the read token of the
//! shared board the blocks belong to.
//!
//! The commands and the binary frames of the bodies edited together are handed to the
//! connection's seat among their rooms (`crate::collab::Seat`).

mod connection;
mod delivery;
mod inbound;
mod presence;
mod readers;
mod subscriptions;

use delivery::{Access, Readers};
#[cfg(feature = "test-util")]
mod testing;
mod writer;
#[cfg(feature = "test-util")]
pub use testing::SubscriptionPause;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use axum::extract::ws::Utf8Bytes;
use fvoci_model::{
    Block, Board, BoardCategoryWebsocketData, BoardMember, Category, ClientConfig, LoginOrigin,
    Session,
};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::app::{Audience, Former};
use crate::auth::limiter::AddressKey;
use crate::auth::{Authenticator, now_millis};
use crate::metrics::{Disconnect, Metrics};
use crate::permissions::{BoardScope, role_of};

/// How many undelivered messages a connection may have queued before it is dropped. A
/// client that reads this slowly has lost track of the board anyway and reloads on
/// reconnect.
const OUTBOX_CAPACITY: usize = 512;

/// How long a connection that is being closed may take to send what is already on its
/// way and the closing frame. A peer that has stopped reading gets neither.
const WRITER_GRACE: Duration = Duration::from_secs(5);

/// How often a connection's session is looked up again when the connections are not
/// pinged; when they are, it is looked up at every ping.
const SESSION_CHECK_WITHOUT_PING: Duration = Duration::from_mins(1);

/// Whose connection a websocket is: the session its handshake came with, and the token
/// that session was found under.
#[derive(Clone)]
pub struct Identity {
    /// The session.
    pub session: Session,
    /// The session token of the handshake.
    pub token: String,
}

impl std::fmt::Debug for Identity {
    /// Leaves out the token.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Identity")
            .field("session", &self.session)
            .finish_non_exhaustive()
    }
}

/// A command a client sends.
#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct Command {
    action: String,
    workspace_id: String,
    board_id: String,
    card_id: String,
    read_token: String,
    #[serde(deserialize_with = "null_as_empty")]
    block_ids: Vec<String>,
}

fn null_as_empty<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<String>, D::Error> {
    Ok(Option::<Vec<String>>::deserialize(deserializer)?.unwrap_or_default())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct UpdateBlockMessage<'a> {
    action: &'static str,
    workspace_id: &'a str,
    block: &'a Block,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct UpdateBoardMessage<'a> {
    action: &'static str,
    workspace_id: &'a str,
    board: &'a Board,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct UpdateMemberMessage<'a> {
    action: &'static str,
    workspace_id: &'a str,
    member: &'a BoardMember,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct UpdateCategoryMessage<'a> {
    action: &'static str,
    workspace_id: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    category: Option<&'a Category>,
    #[serde(skip_serializing_if = "<[_]>::is_empty")]
    board_categories: &'a [BoardCategoryWebsocketData],
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CategoryReorderMessage<'a> {
    action: &'static str,
    category_order: &'a [String],
    workspace_id: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CategoryBoardsReorderMessage<'a> {
    action: &'static str,
    category_id: &'a str,
    board_order: &'a [String],
    workspace_id: &'a str,
}

/// The server stopped what a connection followed of a workspace (`ending.rs`).
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct UnsubscribedWorkspaceMessage<'a> {
    action: &'static str,
    workspace_id: &'a str,
    reason: &'static str,
}

/// The server stopped what a connection followed of a shared board's blocks.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct UnsubscribedBlocksMessage<'a> {
    action: &'static str,
    workspace_id: &'a str,
    block_ids: &'a [String],
    reason: &'static str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct UpdateClientConfigMessage<'a> {
    action: &'static str,
    client_config: &'a ClientConfig,
}

/// A message the hub sends, with the action it names: the label it is counted under.
trait Outgoing: Serialize {
    fn action(&self) -> &'static str;
}

macro_rules! outgoing {
    ($($message:ident),* $(,)?) => {
        $(impl Outgoing for $message<'_> {
            fn action(&self) -> &'static str {
                self.action
            }
        })*
    };
}

outgoing!(
    UpdateBlockMessage,
    UpdateBoardMessage,
    UpdateMemberMessage,
    UpdateCategoryMessage,
    CategoryReorderMessage,
    CategoryBoardsReorderMessage,
    UpdateClientConfigMessage,
    UnsubscribedWorkspaceMessage,
    UnsubscribedBlocksMessage,
);

type ListenerId = u64;

/// How many times a subscription or a change is checked again because access was
/// taken back meanwhile, before it is given up and its connection told to read anew
/// (1013): a check is a few reads of the store, and revocations elsewhere can keep
/// overtaking it.
const CHECKED_AT_MOST: usize = 8;

#[derive(Debug)]
struct Listener {
    /// The user of the session the handshake came with; empty for a connection without
    /// one, which stays so: no message signs a connection in.
    user_id: String,
    /// The session the handshake came with; empty for a connection without one.
    session_id: String,
    /// Where that session came from when it was made through a provider; `None` for a
    /// password, and without a session.
    login: Option<LoginOrigin>,
    /// Where the connection comes from, as readers by a public link are counted
    /// (`readers.rs`); `None` through the administration socket.
    address: Option<AddressKey>,
    outbox: mpsc::Sender<Utf8Bytes>,
    /// Cancelled when the hub gives the connection up; ends its reading side.
    closed: CancellationToken,
    /// Why the hub gave the connection up, set before `closed` is cancelled.
    closed_because: Arc<OnceLock<Disconnect>>,
    workspaces: Vec<String>,
    workspace_roles: HashMap<String, fvoci_model::WorkspaceRole>,
    blocks: Vec<String>,
    /// The shared boards whose read token this connection subscribed to blocks with.
    /// When such a board stops being shared under that token, they stop being followed.
    shared_boards: Vec<ReadPermit>,
    presence: presence::State,
}

/// A shared board followed by its read token: the workspace and the blocks the
/// connection named, which it is told of when the token stops opening the board.
#[derive(Clone)]
struct ReadPermit {
    board: String,
    token: String,
    workspace: String,
    blocks: Vec<String>,
}

impl std::fmt::Debug for ReadPermit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReadPermit")
            .field("board", &self.board)
            .field("workspace", &self.workspace)
            .field("blocks", &self.blocks)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Default)]
struct Listeners {
    next_id: ListenerId,
    /// What was taken back, for the subscriptions being checked (`revocations.rs`).
    revoked: revocations::Revocations,
    by_id: HashMap<ListenerId, Listener>,
    by_workspace: HashMap<String, Vec<ListenerId>>,
    by_block: HashMap<String, Vec<ListenerId>>,
    /// The board visits of the connections, by board.
    visits: presence::Boards,
}

impl Listener {
    /// The provider the connection's session was made through, if any.
    fn login_provider(&self) -> Option<&str> {
        self.login.as_ref().map(|login| login.provider_id.as_str())
    }

    /// Gives the connection up, for `reason`; the first reason given is the one counted.
    fn close(&self, reason: Disconnect) {
        let _ = self.closed_because.set(reason);
        self.closed.cancel();
    }
}

impl Listeners {
    /// The listeners of a user that are subscribed to the workspace: every tab of the user.
    fn of_user(&self, workspace_id: &str, user_id: &str) -> Vec<ListenerId> {
        self.by_workspace
            .get(workspace_id)
            .map(|ids| {
                ids.iter()
                    .copied()
                    .filter(|id| {
                        self.by_id
                            .get(id)
                            .is_some_and(|listener| listener.user_id == user_id)
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Queues `text` for each of `recipients`, and closes and takes out one that does
    /// not keep up. How many it was queued for.
    fn queue(&mut self, recipients: &[ListenerId], text: &Utf8Bytes) -> usize {
        let mut queued = 0;
        for id in recipients {
            let Some(listener) = self.by_id.get(id) else {
                continue;
            };
            if listener.outbox.try_send(text.clone()).is_ok() {
                queued += 1;
            } else {
                tracing::warn!(
                    listener = id,
                    "websocket: dropping a connection that does not keep up"
                );
                listener.close(Disconnect::SlowConsumer);
                self.remove(*id);
            }
        }
        queued
    }

    fn remove(&mut self, id: ListenerId) {
        let Some(listener) = self.by_id.remove(&id) else {
            return;
        };
        for workspace in &listener.workspaces {
            remove_from(&mut self.by_workspace, workspace, id);
        }
        for block in &listener.blocks {
            remove_from(&mut self.by_block, block, id);
        }
        self.visits.forget(id, &listener.presence);
    }
}

fn remove_from(index: &mut HashMap<String, Vec<ListenerId>>, key: &str, id: ListenerId) {
    if let Some(ids) = index.get_mut(key) {
        ids.retain(|other| *other != id);
        if ids.is_empty() {
            index.remove(key);
        }
    }
}

/// The set of open websocket connections and what each is subscribed to.
#[derive(Debug, Clone)]
pub struct Hub {
    inner: Arc<HubInner>,
    pub(crate) audience: Audience,
}

#[derive(Debug)]
struct HubInner {
    listeners: Mutex<Listeners>,
    auth: Authenticator,
    ping_interval: Option<Duration>,
    metrics: Metrics,
    /// Whether the task that tells visits of access changes started (`presence`).
    watching_access: std::sync::Once,
    #[cfg(feature = "test-util")]
    subscription_pause: Mutex<Option<SubscriptionPause>>,
    #[cfg(feature = "test-util")]
    dispatch_pause: Mutex<Option<SubscriptionPause>>,
    /// How long its connections may send nothing ([`inbound::QUIET_LIMIT`]).
    quiet_limit: inbound::QuietLimit,
}

impl Hub {
    /// Creates a hub. `ping_interval` is how often each connection is pinged, if at all;
    /// the connections, the messages and why connections end are counted in `metrics`.
    pub fn new(
        audience: Audience,
        auth: Authenticator,
        ping_interval: Option<Duration>,
        metrics: Metrics,
    ) -> Self {
        // The browser's Pong to a Ping is what keeps a page whose timers are stopped;
        // it needs some time to come back within the quiet limit.
        let answered_in_time = inbound::QUIET_LIMIT.saturating_sub(inbound::PONG_MARGIN);
        if ping_interval.is_none_or(|period| period >= answered_in_time) {
            tracing::warn!(
                ping = ?ping_interval,
                quiet_limit = ?inbound::QUIET_LIMIT,
                "websocket_ping_seconds is 0 or too long for the quiet limit of a connection: pages whose timers are stopped (a frozen tab) can be closed after it, and connect again"
            );
        }
        Self {
            inner: Arc::new(HubInner {
                listeners: Mutex::new(Listeners::default()),
                auth,
                ping_interval,
                metrics,
                watching_access: std::sync::Once::new(),
                #[cfg(feature = "test-util")]
                subscription_pause: Mutex::default(),
                #[cfg(feature = "test-util")]
                dispatch_pause: Mutex::default(),
                quiet_limit: inbound::QuietLimit::default(),
            }),
            audience,
        }
    }

    fn listeners(&self) -> std::sync::MutexGuard<'_, Listeners> {
        self.inner
            .listeners
            .lock()
            .expect("the listener table lock is never held across a panic")
    }

    /// Closes every connection. For shutdown: the connections are not part of the HTTP
    /// server's own connection handling once they are upgraded.
    pub fn close(&self) {
        self.close_where(Disconnect::Shutdown, |_| true);
    }

    // -- broadcasts -------------------------------------------------------------------

    /// Tells the board's members in the workspace, and the listeners of the block and of its
    /// parent, that a block changed.
    ///
    /// A read-token subscription receives no user identifiers ([`Block::hide_people`])
    /// and no identifier of what a body names outside the board
    /// ([`Block::clear_references_but`]), as over public HTTP, unless its connection's
    /// user sees the board as its member.
    pub async fn broadcast_block_change(&self, workspace_id: &str, block: &Block) {
        let followers = self.block_followers(block);
        // Most changes happen with nobody listening; do not ask the store then.
        if followers.is_empty() && !self.listeners().by_workspace.contains_key(workspace_id) {
            return;
        }
        let recipients = self.workspace_listeners(workspace_id);
        let message = UpdateBlockMessage {
            action: "UPDATE_BLOCK",
            workspace_id,
            block,
        };
        self.send(
            &recipients,
            &message,
            Access::Members {
                workspace: workspace_id,
                readers: Readers::Now(&block.board_id),
                also: None,
            },
        )
        .await;
        let by_token: Vec<_> = followers.into_iter().map(|(id, _)| id).collect();
        self.send_public_block(&by_token, workspace_id, block).await;
    }

    /// The connections that follow the block or its parent by `SUBSCRIBE_BLOCKS`, with
    /// their users (empty for a connection without a session).
    fn block_followers(&self, block: &Block) -> Vec<(ListenerId, String)> {
        let listeners = self.listeners();
        [&block.id, &block.parent_id]
            .into_iter()
            .flat_map(|block_id| listeners.by_block.get(block_id).into_iter().flatten())
            .filter_map(|id| {
                listeners
                    .by_id
                    .get(id)
                    .map(|listener| (*id, listener.user_id.clone()))
            })
            .collect()
    }

    /// Tells the same listeners that a block was deleted. The message is a block change
    /// whose block has only its identifiers and the deletion time.
    pub async fn broadcast_block_delete(&self, workspace_id: &str, block_id: &str, board_id: &str) {
        let now = now_millis();
        let block = Block {
            id: block_id.to_owned(),
            board_id: board_id.to_owned(),
            update_at: now,
            delete_at: now,
            ..Block::default()
        };
        self.broadcast_block_change(workspace_id, &block).await;
    }

    /// Tells those who read a board that is deleted by now that one of its blocks was
    /// deleted, with the message of [`Self::broadcast_block_delete`].
    ///
    /// The memberships went with the board, so the caller says who read it.
    pub async fn broadcast_block_delete_with_board(
        &self,
        workspace_id: &str,
        block_id: &str,
        board_id: &str,
        former: Former<'_>,
    ) {
        let now = now_millis();
        let block = Block {
            id: block_id.to_owned(),
            board_id: board_id.to_owned(),
            update_at: now,
            delete_at: now,
            ..Block::default()
        };
        let message = UpdateBlockMessage {
            action: "UPDATE_BLOCK",
            workspace_id,
            block: &block,
        };
        self.send_to_former(workspace_id, &message, former).await;
    }

    /// Tells the board's members in the workspace that the board changed.
    pub async fn broadcast_board_change(&self, workspace_id: &str, board: &Board) {
        let message = UpdateBoardMessage {
            action: "UPDATE_BOARD",
            workspace_id,
            board,
        };
        let recipients = self.workspace_listeners(workspace_id);
        self.send(
            &recipients,
            &message,
            Access::Members {
                workspace: workspace_id,
                readers: Readers::Now(&board.id),
                also: None,
            },
        )
        .await;
    }

    /// Tells those who read the board that it was deleted. The message is a board change
    /// whose board has only its identifiers and the deletion time.
    ///
    /// The memberships went with the board, so the caller says who read it. It is told
    /// so as well to those who read an open wiki that was made private, as they read it
    /// no longer: the caller says who read it before.
    pub async fn broadcast_board_delete(
        &self,
        workspace_id: &str,
        board_id: &str,
        former: Former<'_>,
    ) {
        let board = gone_board(workspace_id, board_id);
        let message = UpdateBoardMessage {
            action: "UPDATE_BOARD",
            workspace_id,
            board: &board,
        };
        self.send_to_former(workspace_id, &message, former).await;
    }

    /// Tells who read the board as it was, `scope`, without holding a role on it, that
    /// it is gone for them, with the message of [`Self::broadcast_board_delete`]: an
    /// open wiki made private. Its `members` read it still, and are not told.
    pub async fn broadcast_closed_to_others(
        &self,
        workspace_id: &str,
        board_id: &str,
        members: &[BoardMember],
        scope: BoardScope,
    ) {
        let board = gone_board(workspace_id, board_id);
        let message = UpdateBoardMessage {
            action: "UPDATE_BOARD",
            workspace_id,
            board: &board,
        };
        let in_workspace = self.workspace_listeners(workspace_id);
        let recipients: Vec<ListenerId> = {
            let listeners = self.listeners();
            in_workspace
                .into_iter()
                .filter(|id| {
                    listeners.by_id.get(id).is_some_and(|listener| {
                        !members.iter().any(|member| {
                            member.user_id == listener.user_id && role_of(member).is_some()
                        })
                    })
                })
                .collect()
        };
        let readers = Readers::Former {
            members: &[],
            scope,
        };
        self.send(
            &recipients,
            &message,
            Access::Members {
                workspace: workspace_id,
                readers,
                also: None,
            },
        )
        .await;
    }

    /// Tells who read a board that is gone of `message`: its members, and of an open
    /// wiki whoever works in the workspace, as they are in it now.
    async fn send_to_former(
        &self,
        workspace_id: &str,
        message: &impl Outgoing,
        former: Former<'_>,
    ) {
        let recipients = self.workspace_listeners(workspace_id);
        let readers = Readers::Former {
            members: former.members,
            scope: former.scope,
        };
        self.send(
            &recipients,
            message,
            Access::Members {
                workspace: workspace_id,
                readers,
                also: None,
            },
        )
        .await;
    }

    /// Tells the board's members in the workspace that a membership changed.
    pub async fn broadcast_member_change(
        &self,
        workspace_id: &str,
        board_id: &str,
        member: &BoardMember,
    ) {
        let message = UpdateMemberMessage {
            action: "UPDATE_MEMBER",
            workspace_id,
            member,
        };
        let recipients = self.workspace_listeners(workspace_id);
        self.send(
            &recipients,
            &message,
            Access::Members {
                workspace: workspace_id,
                readers: Readers::Now(board_id),
                also: None,
            },
        )
        .await;
    }

    /// Tells the board's members in the workspace, and the removed user, that a membership
    /// ended.
    pub async fn broadcast_member_delete(&self, workspace_id: &str, board_id: &str, user_id: &str) {
        let member = BoardMember {
            user_id: user_id.to_owned(),
            board_id: board_id.to_owned(),
            ..BoardMember::default()
        };
        let message = UpdateMemberMessage {
            action: "DELETE_MEMBER",
            workspace_id,
            member: &member,
        };
        let recipients = self.workspace_listeners(workspace_id);
        self.send(
            &recipients,
            &message,
            Access::Members {
                workspace: workspace_id,
                readers: Readers::Now(board_id),
                also: Some(user_id),
            },
        )
        .await;
    }

    /// Tells every connection that the client configuration changed.
    pub async fn broadcast_config_change(&self, client_config: &ClientConfig) {
        let message = UpdateClientConfigMessage {
            action: "UPDATE_CLIENT_CONFIG",
            client_config,
        };
        let recipients: Vec<ListenerId> = self.listeners().by_id.keys().copied().collect();
        self.send(&recipients, &message, Access::All).await;
    }

    /// Tells the category's owner that it changed.
    pub async fn broadcast_category_change(&self, category: &Category) {
        let message = UpdateCategoryMessage {
            action: "UPDATE_CATEGORY",
            workspace_id: &category.workspace_id,
            category: Some(category),
            board_categories: &[],
        };
        self.send_to_user(&category.workspace_id, &category.user_id, &message)
            .await;
    }

    /// Tells a user that boards moved between their categories or were hidden.
    pub async fn broadcast_category_board_change(
        &self,
        workspace_id: &str,
        user_id: &str,
        board_categories: &[BoardCategoryWebsocketData],
    ) {
        let message = UpdateCategoryMessage {
            action: "UPDATE_BOARD_CATEGORY",
            workspace_id,
            category: None,
            board_categories,
        };
        self.send_to_user(workspace_id, user_id, &message).await;
    }

    /// Tells a user the new order of their categories.
    pub async fn broadcast_category_reorder(
        &self,
        workspace_id: &str,
        user_id: &str,
        category_order: &[String],
    ) {
        let message = CategoryReorderMessage {
            action: "REORDER_CATEGORIES",
            category_order,
            workspace_id,
        };
        self.send_to_user(workspace_id, user_id, &message).await;
    }

    /// Tells a user the new order of the boards in one of their categories.
    pub async fn broadcast_category_boards_reorder(
        &self,
        workspace_id: &str,
        user_id: &str,
        category_id: &str,
        board_order: &[String],
    ) {
        let message = CategoryBoardsReorderMessage {
            action: "REORDER_CATEGORY_BOARDS",
            category_id,
            board_order,
            workspace_id,
        };
        self.send_to_user(workspace_id, user_id, &message).await;
    }

    fn workspace_listeners(&self, workspace_id: &str) -> Vec<ListenerId> {
        self.listeners()
            .by_workspace
            .get(workspace_id)
            .cloned()
            .unwrap_or_default()
    }

    /// Sends a message to every connection of the user that is subscribed to the workspace:
    /// every tab of the user is told. (The original tells the first one only.)
    async fn send_to_user(&self, workspace_id: &str, user_id: &str, message: &impl Outgoing) {
        let recipients = self.listeners().of_user(workspace_id, user_id);
        self.send(&recipients, message, Access::Workspace(workspace_id))
            .await;
    }

    /// Queues the text of a message whose action is `action` for the given listeners.
    /// A listener whose queue is full is dropped and its connection closed. Cloning the
    /// text copies none of it.
    fn queue_text(&self, recipients: &[ListenerId], text: &Utf8Bytes, action: &'static str) {
        if recipients.is_empty() {
            return;
        }
        let queued = self.listeners().queue(recipients, text);
        self.inner.metrics.websocket_queued(action, queued);
    }
}

/// A message as the text frame it is sent in; `None`, logged, if it cannot be written.
fn text_of(message: &impl Outgoing) -> Option<Utf8Bytes> {
    match serde_json::to_string(message) {
        Ok(text) => Some(text.into()),
        Err(error) => {
            tracing::error!(%error, action = message.action(), "websocket: a message could not be serialized");
            None
        }
    }
}

fn subscribe_to_workspace(listeners: &mut Listeners, id: ListenerId, workspace_id: &str) {
    let Some(listener) = listeners.by_id.get_mut(&id) else {
        return;
    };
    if listener
        .workspaces
        .iter()
        .any(|workspace| workspace == workspace_id)
    {
        return;
    }
    listener.workspaces.push(workspace_id.to_owned());
    listeners
        .by_workspace
        .entry(workspace_id.to_owned())
        .or_default()
        .push(id);
}

fn subscribe_to_blocks(listeners: &mut Listeners, id: ListenerId, block_ids: &[String]) {
    let Some(listener) = listeners.by_id.get_mut(&id) else {
        return;
    };
    for block_id in block_ids {
        if listener.blocks.contains(block_id) {
            continue;
        }
        listener.blocks.push(block_id.clone());
        listeners
            .by_block
            .entry(block_id.clone())
            .or_default()
            .push(id);
    }
}

fn unsubscribe_from_blocks(listeners: &mut Listeners, id: ListenerId, block_ids: &[String]) {
    if let Some(listener) = listeners.by_id.get_mut(&id) {
        listener.blocks.retain(|block| !block_ids.contains(block));
        for permit in &mut listener.shared_boards {
            permit.blocks.retain(|block| !block_ids.contains(block));
        }
        // A board none of whose blocks is followed is not followed.
        listener
            .shared_boards
            .retain(|permit| !permit.blocks.is_empty());
    }
    for block_id in block_ids {
        remove_from(&mut listeners.by_block, block_id, id);
    }
}

/// A board as it is told once gone: only its identifiers and the time it went, as
/// its deletion and the closing of an open wiki to who did not join it are told.
fn gone_board(workspace_id: &str, board_id: &str) -> Board {
    let now = now_millis();
    Board {
        id: board_id.to_owned(),
        workspace_id: workspace_id.to_owned(),
        update_at: now,
        delete_at: now,
        ..Board::default()
    }
}

mod closing;
mod ending;
mod revocations;
mod subscribe;

/// Ports of `server/ws/server_test.go`, on the listener table.
#[cfg(test)]
mod tests;
