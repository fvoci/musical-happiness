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

mod delivery;
mod subscriptions;

use delivery::Access;
#[cfg(feature = "test-util")]
mod testing;
mod writer;
#[cfg(feature = "test-util")]
pub use testing::SubscriptionPause;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use axum::extract::ws::{Message, Utf8Bytes, WebSocket};
use futures_util::StreamExt as _;
use fvoci_model::{
    Block, Board, BoardCategoryWebsocketData, BoardMember, Category, ClientConfig, Session,
};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::app::{App, Audience};
use crate::auth::{Authenticator, now_millis};
use crate::collab::Seat;
use crate::error::ErrorKind;
use crate::metrics::{Disconnect, Metrics};
use crate::permissions::role_of;

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
);

type ListenerId = u64;

#[derive(Debug)]
struct Listener {
    /// The user of the session the handshake came with; empty for a connection without
    /// one, which stays so: no message signs a connection in.
    user_id: String,
    /// The session the handshake came with; empty for a connection without one.
    session_id: String,
    /// The provider that session was made through; `None` for a password, and without
    /// a session.
    login_provider: Option<String>,
    outbox: mpsc::Sender<Utf8Bytes>,
    /// Cancelled when the hub gives the connection up; ends its reading side.
    closed: CancellationToken,
    /// Why the hub gave the connection up, set before `closed` is cancelled.
    closed_because: Arc<OnceLock<Disconnect>>,
    workspaces: Vec<String>,
    workspace_roles: HashMap<String, fvoci_model::WorkspaceRole>,
    /// Current and pending document participation, independent of sidebar subscriptions.
    documents: crate::collab::Participation,
    blocks: Vec<String>,
    /// The shared boards whose read token this connection subscribed to blocks with.
    /// When such a board stops being shared under that token, the connection goes.
    shared_boards: Vec<ReadPermit>,
}

#[derive(Clone)]
struct ReadPermit {
    board: String,
    token: String,
}

impl std::fmt::Debug for ReadPermit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReadPermit")
            .field("board", &self.board)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Default)]
struct Listeners {
    next_id: ListenerId,
    /// Changes whenever access is revoked, including before a subscription is registered.
    revision: u64,
    by_id: HashMap<ListenerId, Listener>,
    by_workspace: HashMap<String, Vec<ListenerId>>,
    by_block: HashMap<String, Vec<ListenerId>>,
}

impl Listener {
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
    #[cfg(feature = "test-util")]
    subscription_pause: Mutex<Option<SubscriptionPause>>,
    #[cfg(feature = "test-util")]
    dispatch_pause: Mutex<Option<SubscriptionPause>>,
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
        Self {
            inner: Arc::new(HubInner {
                listeners: Mutex::new(Listeners::default()),
                auth,
                ping_interval,
                metrics,
                #[cfg(feature = "test-util")]
                subscription_pause: Mutex::default(),
                #[cfg(feature = "test-util")]
                dispatch_pause: Mutex::default(),
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

    /// Serves one websocket connection until it closes. `identity` is the session the
    /// handshake came with, if any; without one the connection can only subscribe to
    /// the blocks of shared boards. `app` is the application the handshake reached.
    pub async fn serve(&self, app: &App, socket: WebSocket, identity: Option<Identity>) {
        let (sink, mut stream) = socket.split();
        let (outbox, inbox) = mpsc::channel::<Utf8Bytes>(OUTBOX_CAPACITY);
        let closed = CancellationToken::new();
        let closed_because = Arc::new(OnceLock::new());
        let who = subscribe::Who::of(identity);
        let (mut docs, docs_outbox) = app.collab().seat(app, who.asker());
        let token = who.token;
        let docs_overrun = docs.overrun();

        let id = {
            let mut listeners = self.listeners();
            listeners.next_id += 1;
            let id = listeners.next_id;
            listeners.by_id.insert(
                id,
                Listener {
                    user_id: who.user_id,
                    session_id: who.session_id,
                    login_provider: who.login_provider,
                    outbox,
                    closed: closed.clone(),
                    closed_because: Arc::clone(&closed_because),
                    workspaces: Vec::new(),
                    workspace_roles: HashMap::new(),
                    documents: docs.participation(),
                    blocks: Vec::new(),
                    shared_boards: Vec::new(),
                },
            );
            id
        };
        let metrics = &self.inner.metrics;
        let _open = metrics.websocket_opened();

        // The sessions of a user can be ended between the handshake and the lines above
        // that made this a connection of that user; the closing of the user's
        // connections would then have passed this one by. Looked up once more, now that
        // it is closable, the session is either still there or the connection ends.
        if let Some(token) = &token
            && let Err(error) = self.inner.auth.find_session(token).await
        {
            if error.kind() != ErrorKind::Unauthorized {
                tracing::warn!(%error, "a websocket connection ends: its session could not be read");
            }
            self.listeners().remove(id);
            metrics.websocket_closed(Disconnect::SessionEnded);
            return;
        }

        let ping_interval = self.inner.ping_interval;
        let session_check = ping_interval.unwrap_or(SESSION_CHECK_WITHOUT_PING);
        let mut writer = writer::spawn(
            sink,
            inbox,
            docs_outbox,
            ping_interval,
            closed.clone(),
            Arc::clone(&closed_because),
        );

        let reason = tokio::select! {
            // First: when the hub gave the connection up, its reason is the one counted,
            // also if the client closed it at the same moment.
            biased;
            // The hub gave the connection up: it does not keep up, or its session or its
            // read token ended.
            () = closed.cancelled() => {
                closed_because.get().copied().unwrap_or(Disconnect::Shutdown)
            }
            // It reads nothing of what the rooms send it.
            () = docs_overrun.cancelled() => Disconnect::SlowConsumer,
            reason = self.read_commands(id, &mut stream, &mut docs) => reason,
            // The session ended by its idle or absolute limit, which nobody announces:
            // the sessions that ended so are deleted only by the hourly clean-up.
            () = self.session_ends(token.as_deref(), session_check) => Disconnect::SessionEnded,
        };
        let _ = closed_because.set(reason);
        closed.cancel();
        metrics.websocket_closed(reason);
        docs.leave_all().await;

        // The cancelled writer discards queued changes. A peer that does not read
        // must not hold its closing handshake forever.
        self.listeners().remove(id);
        if tokio::time::timeout(WRITER_GRACE, &mut writer)
            .await
            .is_err()
        {
            writer.abort();
        }
    }

    /// Reads the commands of a connection and carries them out, until the client
    /// closes it, goes away or sends what is no websocket frame; returns which.
    async fn read_commands(
        &self,
        id: ListenerId,
        stream: &mut (impl futures_util::Stream<Item = Result<Message, axum::Error>> + Unpin),
        docs: &mut Seat,
    ) -> Disconnect {
        loop {
            let text = match stream.next().await {
                Some(Ok(Message::Text(text))) => text,
                Some(Ok(Message::Close(_))) | None => return Disconnect::ClientClosed,
                Some(Ok(Message::Binary(frame))) => {
                    docs.frame(frame).await;
                    continue;
                }
                Some(Ok(Message::Ping(_) | Message::Pong(_))) => continue,
                Some(Err(_)) => return Disconnect::ReadError,
            };
            // A command is small, whatever a frame of a body edited together may be.
            if text.len() > crate::api::WEBSOCKET_MESSAGE_LIMIT {
                return Disconnect::ReadError;
            }
            if docs.command(&text).await {
                continue;
            }
            let Ok(command) = serde_json::from_str::<Command>(&text) else {
                tracing::error!("websocket: a command could not be parsed");
                continue;
            };
            // No command closes a connection now; were one to, the client asked for it.
            if !self.handle(id, command).await {
                return Disconnect::ClientClosed;
            }
        }
    }

    /// Returns once the session of `token` has ended, looked up every `period`; never
    /// for a connection without a session. A store that cannot be read ends nothing.
    async fn session_ends(&self, token: Option<&str>, period: Duration) {
        let Some(token) = token else {
            return std::future::pending().await;
        };
        let mut checks = tokio::time::interval(period);
        checks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        // The first tick is at once; the session was looked up just before.
        checks.tick().await;
        loop {
            checks.tick().await;
            match self.inner.auth.find_session(token).await {
                Ok(_) => {}
                Err(error) if error.kind() == ErrorKind::Unauthorized => return,
                Err(error) => {
                    tracing::warn!(%error, "a websocket connection's session could not be read");
                }
            }
        }
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
    /// A read-token subscription receives no user identifiers, including when its
    /// connection also has a session ([`Block::hide_people`]), as over public HTTP.
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
                board: &block.board_id,
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

    /// Tells those who were the members of a board that is deleted by now that one of
    /// its blocks was deleted, with the message of [`Self::broadcast_block_delete`].
    ///
    /// The memberships went with the board, so the caller says who the members were.
    pub async fn broadcast_block_delete_with_board(
        &self,
        workspace_id: &str,
        block_id: &str,
        board_id: &str,
        former_members: &[BoardMember],
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
        let recipients = self.listeners_among(workspace_id, former_members, None);
        self.send(&recipients, &message, Access::Workspace(workspace_id))
            .await;
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
                board: &board.id,
                also: None,
            },
        )
        .await;
    }

    /// Tells those who were the board's members that the board was deleted. The message
    /// is a board change whose board has only its identifiers and the deletion time.
    ///
    /// The memberships went with the board, so the caller says who the members were.
    pub async fn broadcast_board_delete(
        &self,
        workspace_id: &str,
        board_id: &str,
        former_members: &[BoardMember],
    ) {
        let now = now_millis();
        let board = Board {
            id: board_id.to_owned(),
            workspace_id: workspace_id.to_owned(),
            update_at: now,
            delete_at: now,
            ..Board::default()
        };
        let message = UpdateBoardMessage {
            action: "UPDATE_BOARD",
            workspace_id,
            board: &board,
        };
        let recipients = self.listeners_among(workspace_id, former_members, None);
        self.send(&recipients, &message, Access::Workspace(workspace_id))
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
                board: board_id,
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
                board: board_id,
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

    /// The workspace's listeners whose user is one of `members` with a role, or `also_user`.
    fn listeners_among(
        &self,
        workspace_id: &str,
        members: &[BoardMember],
        also_user: Option<&str>,
    ) -> Vec<ListenerId> {
        let listeners = self.listeners();
        let Some(workspace_listeners) = listeners.by_workspace.get(workspace_id) else {
            return Vec::new();
        };
        workspace_listeners
            .iter()
            .copied()
            .filter(|id| {
                listeners.by_id.get(id).is_some_and(|listener| {
                    also_user == Some(listener.user_id.as_str())
                        || members.iter().any(|member| {
                            member.user_id == listener.user_id && role_of(member).is_some()
                        })
                })
            })
            .collect()
    }

    /// Sends a message to every connection of the user that is subscribed to the workspace:
    /// every tab of the user is told. (The original tells the first one only.)
    async fn send_to_user(&self, workspace_id: &str, user_id: &str, message: &impl Outgoing) {
        let recipients = self.listeners().of_user(workspace_id, user_id);
        self.send(&recipients, message, Access::Workspace(workspace_id))
            .await;
    }

    /// Queues a message for the given listeners. A listener whose queue is full is
    /// dropped and its connection closed.
    fn queue(&self, recipients: &[ListenerId], message: &impl Outgoing) {
        if recipients.is_empty() {
            return;
        }
        // Shared by all recipients: cloning it copies no text.
        let text: Utf8Bytes = match serde_json::to_string(message) {
            Ok(text) => text.into(),
            Err(error) => {
                tracing::error!(%error, "websocket: a message could not be serialized");
                return;
            }
        };
        let mut listeners = self.listeners();
        let mut queued = 0;
        for id in recipients {
            let Some(listener) = listeners.by_id.get(id) else {
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
                listeners.remove(*id);
            }
        }
        drop(listeners);
        self.inner
            .metrics
            .websocket_queued(message.action(), queued);
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
    }
    for block_id in block_ids {
        remove_from(&mut listeners.by_block, block_id, id);
    }
}

mod closing;
mod subscribe;

/// Ports of `server/ws/server_test.go`, on the listener table.
#[cfg(test)]
mod tests;
