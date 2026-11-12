//! The websocket endpoint `/ws`: clients subscribe to a workspace or to blocks and are told
//! about changes as they happen.
//!
//! Whose connection it is is settled at the handshake, from the session the request
//! carries; there is no message to sign in with. A user's connection may subscribe to
//! workspaces. `SUBSCRIBE_BLOCKS` works without a session but needs the read token of the
//! shared board the blocks belong to.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use axum::extract::ws::{Message, Utf8Bytes, WebSocket};
use futures_util::{SinkExt as _, StreamExt as _};
use fvoci_model::{
    Block, Board, BoardCategoryWebsocketData, BoardMember, Category, ClientConfig,
    PersonProperties, Session,
};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::app::Audience;
use crate::auth::{Authenticator, now_millis};
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
    outbox: mpsc::Sender<Utf8Bytes>,
    /// Cancelled when the hub gives the connection up; ends its reading side.
    closed: CancellationToken,
    /// Why the hub gave the connection up, set before `closed` is cancelled.
    closed_because: Arc<OnceLock<Disconnect>>,
    workspaces: Vec<String>,
    blocks: Vec<String>,
    /// The shared boards whose read token this connection subscribed to blocks with.
    /// When such a board stops being shared under that token, the connection goes.
    shared_boards: Vec<String>,
}

#[derive(Debug, Default)]
struct Listeners {
    next_id: ListenerId,
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
    /// the blocks of shared boards.
    pub async fn serve(&self, socket: WebSocket, identity: Option<Identity>) {
        let (mut sink, mut stream) = socket.split();
        let (outbox, mut inbox) = mpsc::channel::<Utf8Bytes>(OUTBOX_CAPACITY);
        let closed = CancellationToken::new();
        let closed_because = Arc::new(OnceLock::new());
        let (user_id, session_id, token) = match identity {
            Some(identity) => (
                identity.session.user_id,
                identity.session.id,
                Some(identity.token),
            ),
            None => (String::new(), String::new(), None),
        };

        let id = {
            let mut listeners = self.listeners();
            listeners.next_id += 1;
            let id = listeners.next_id;
            listeners.by_id.insert(
                id,
                Listener {
                    user_id,
                    session_id,
                    outbox,
                    closed: closed.clone(),
                    closed_because: Arc::clone(&closed_because),
                    workspaces: Vec::new(),
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
        let mut writer = tokio::spawn(async move {
            let mut ping = ping_interval.map(|period| {
                let mut interval = tokio::time::interval(period);
                interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                interval
            });
            loop {
                let message = tokio::select! {
                    message = inbox.recv() => match message {
                        Some(text) => Message::Text(text),
                        // The hub dropped this connection.
                        None => break,
                    },
                    () = next_tick(ping.as_mut()) => Message::Ping(Vec::new().into()),
                };
                if sink.send(message).await.is_err() {
                    break;
                }
            }
            let _ = sink.close().await;
        });

        let reason = tokio::select! {
            // First: when the hub gave the connection up, its reason is the one counted,
            // also if the client closed it at the same moment.
            biased;
            // The hub gave the connection up: it does not keep up, or its session or its
            // read token ended.
            () = closed.cancelled() => {
                closed_because.get().copied().unwrap_or(Disconnect::Shutdown)
            }
            reason = self.read_commands(id, &mut stream) => reason,
            // The session ended by its idle or absolute limit, which nobody announces:
            // the sessions that ended so are deleted only by the hourly clean-up.
            () = self.session_ends(token.as_deref(), session_check) => Disconnect::SessionEnded,
        };
        metrics.websocket_closed(reason);

        // Removing the listener drops its outbox, which ends the writer once it has sent
        // what was queued. A peer that does not read would hold the writer forever.
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
    ) -> Disconnect {
        loop {
            let text = match stream.next().await {
                Some(Ok(Message::Text(text))) => text,
                Some(Ok(Message::Close(_))) | None => return Disconnect::ClientClosed,
                Some(Ok(Message::Binary(_) | Message::Ping(_) | Message::Pong(_))) => continue,
                Some(Err(_)) => return Disconnect::ReadError,
            };
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

    /// Closes every connection of the user. For when the user's sessions are ended: a
    /// connection asks for its session once, when it is made, and whoever ends a session
    /// closes its connections.
    pub fn close_user(&self, user_id: &str) {
        self.close_where(Disconnect::SessionEnded, |listener| {
            listener.user_id == user_id
        });
    }

    /// Closes the connections that follow blocks of a board by its read token. For when
    /// that token stops opening the board: sharing is turned off, the board is shared
    /// anew under another token, or the board is deleted. A reader with the new link
    /// connects again.
    pub fn close_readers_of_board(&self, board_id: &str) {
        self.close_where(Disconnect::ShareEnded, |listener| {
            listener.shared_boards.iter().any(|board| board == board_id)
        });
    }

    /// Closes the connections of a session. For when the session ends: at logout, and
    /// when it has gone unused for too long or reached its end.
    pub fn close_session(&self, session_id: &str) {
        self.close_where(Disconnect::SessionEnded, |listener| {
            listener.session_id == session_id
        });
    }

    /// Closes the connections of the user that authenticated with another session than
    /// the given one. For when a user changes their password: the session it was
    /// changed through stays, and so do its connections.
    pub fn close_other_sessions_of_user(&self, user_id: &str, kept_session_id: &str) {
        self.close_where(Disconnect::SessionEnded, |listener| {
            listener.user_id == user_id && listener.session_id != kept_session_id
        });
    }

    /// Closes the connections `closes` picks, for `reason`.
    fn close_where(&self, reason: Disconnect, closes: impl Fn(&Listener) -> bool) {
        let mut listeners = self.listeners();
        let ids: Vec<ListenerId> = listeners
            .by_id
            .iter()
            .filter(|(_, listener)| closes(listener))
            .map(|(id, _)| *id)
            .collect();
        for id in ids {
            if let Some(listener) = listeners.by_id.get(&id) {
                listener.close(reason);
            }
            listeners.remove(id);
        }
    }

    /// Handles one command. Returns `false` if the connection must be closed.
    async fn handle(&self, id: ListenerId, command: Command) -> bool {
        match command.action.as_str() {
            "SUBSCRIBE_BLOCKS" | "UNSUBSCRIBE_BLOCKS" => {
                let Some(board_id) = self.board_of_read_token(&command).await else {
                    tracing::error!(action = %command.action, "websocket: rejected an invalid read token");
                    return true;
                };
                let mut listeners = self.listeners();
                if command.action == "SUBSCRIBE_BLOCKS" {
                    subscribe_to_blocks(&mut listeners, id, &command.block_ids);
                    if let Some(listener) = listeners.by_id.get_mut(&id)
                        && !listener.shared_boards.contains(&board_id)
                    {
                        listener.shared_boards.push(board_id);
                    }
                } else {
                    unsubscribe_from_blocks(&mut listeners, id, &command.block_ids);
                }
                return true;
            }
            _ => {}
        }

        let user_id = self
            .listeners()
            .by_id
            .get(&id)
            .map(|listener| listener.user_id.clone())
            .unwrap_or_default();
        if user_id.is_empty() {
            tracing::error!(action = %command.action, "websocket: rejected an unauthenticated message");
            return true;
        }

        match command.action.as_str() {
            "SUBSCRIBE_WORKSPACE" => {
                let workspace = &command.workspace_id;
                if self.audience.is_in_workspace(&user_id, workspace).await {
                    subscribe_to_workspace(&mut self.listeners(), id, &command.workspace_id);
                } else {
                    tracing::error!(workspace_id = %command.workspace_id, user_id, "websocket: no workspace access");
                }
            }
            "UNSUBSCRIBE_WORKSPACE" => {
                let mut listeners = self.listeners();
                if let Some(listener) = listeners.by_id.get_mut(&id) {
                    listener
                        .workspaces
                        .retain(|workspace| *workspace != command.workspace_id);
                }
                remove_from(&mut listeners.by_workspace, &command.workspace_id, id);
            }
            other => tracing::error!(action = other, "websocket: invalid action"),
        }
        true
    }

    /// A block subscription needs a workspace, blocks that all belong to one board, and that
    /// board's read token. Returns the board, or `None` if the subscription is refused.
    async fn board_of_read_token(&self, command: &Command) -> Option<String> {
        if command.workspace_id.is_empty() {
            return None;
        }
        let mut board_id = String::new();
        for block_id in &command.block_ids {
            let of_block = self.audience.board_of_block(block_id).await.ok()?;
            if board_id.is_empty() {
                board_id = of_block;
            } else if board_id != of_block {
                return None;
            }
        }
        match self
            .inner
            .auth
            .is_valid_read_token(&board_id, &command.read_token)
            .await
        {
            Ok(true) => Some(board_id),
            Ok(false) => None,
            Err(error) => {
                tracing::error!(%error, workspace_id = %command.workspace_id, "websocket: checking a read token failed");
                None
            }
        }
    }

    // -- broadcasts -------------------------------------------------------------------

    /// Tells the board's members in the workspace, and the listeners of the block and of its
    /// parent, that a block changed.
    ///
    /// A listener of the block whose connection has no session, or whose user is no
    /// member of the board who may read it, follows it by the read token alone and is
    /// told no user's identifier ([`Block::hide_people`]), as over HTTP (`getBlocks`).
    pub async fn broadcast_block_change(&self, workspace_id: &str, block: &Block) {
        let followers = self.block_followers(block);
        // Most changes happen with nobody listening; do not ask the store then.
        if followers.is_empty() && !self.listeners().by_workspace.contains_key(workspace_id) {
            return;
        }
        let members = match self.audience.members(&block.board_id).await {
            Ok(members) => members,
            Err(error) => {
                tracing::error!(%error, workspace_id, board_id = %block.board_id, "websocket: reading the board's members failed");
                return;
            }
        };
        let mut recipients = self.listeners_among(workspace_id, &members, None);
        let mut by_token = Vec::new();
        // A connection that listens both as a board member and to the block itself
        // gets the message once per subscription.
        for (id, user_id) in followers {
            let reader = !user_id.is_empty()
                && members
                    .iter()
                    .any(|member| member.user_id == user_id && role_of(member).is_some());
            if reader {
                recipients.push(id);
            } else {
                by_token.push(id);
            }
        }
        let message = UpdateBlockMessage {
            action: "UPDATE_BLOCK",
            workspace_id,
            block,
        };
        self.send(&recipients, &message);
        if by_token.is_empty() {
            return;
        }
        // A block without fields names nobody there, such as the block of a deletion.
        let people = if block.fields.is_empty() {
            Ok(PersonProperties::default())
        } else {
            self.audience.person_properties(&block.board_id).await
        };
        let people = match people {
            Ok(people) => people,
            Err(error) => {
                // Without the board's person properties the block cannot be told
                // without its people: it is not told.
                tracing::error!(%error, workspace_id, board_id = %block.board_id, "websocket: reading the board of a followed block failed");
                return;
            }
        };
        let mut without_people = block.clone();
        without_people.hide_people(&people);
        let message = UpdateBlockMessage {
            action: "UPDATE_BLOCK",
            workspace_id,
            block: &without_people,
        };
        self.send(&by_token, &message);
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
    pub fn broadcast_block_delete_with_board(
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
        self.send(&recipients, &message);
    }

    /// Tells the board's members in the workspace that the board changed.
    pub async fn broadcast_board_change(&self, workspace_id: &str, board: &Board) {
        let message = UpdateBoardMessage {
            action: "UPDATE_BOARD",
            workspace_id,
            board,
        };
        let recipients = self
            .listeners_for_workspace_and_board(workspace_id, &board.id, None)
            .await;
        self.send(&recipients, &message);
    }

    /// Tells those who were the board's members that the board was deleted. The message
    /// is a board change whose board has only its identifiers and the deletion time.
    ///
    /// The memberships went with the board, so the caller says who the members were.
    pub fn broadcast_board_delete(
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
        self.send(&recipients, &message);
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
        let recipients = self
            .listeners_for_workspace_and_board(workspace_id, board_id, None)
            .await;
        self.send(&recipients, &message);
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
        let recipients = self
            .listeners_for_workspace_and_board(workspace_id, board_id, Some(user_id))
            .await;
        self.send(&recipients, &message);
    }

    /// Tells every connection that the client configuration changed.
    pub fn broadcast_config_change(&self, client_config: &ClientConfig) {
        let message = UpdateClientConfigMessage {
            action: "UPDATE_CLIENT_CONFIG",
            client_config,
        };
        let recipients: Vec<ListenerId> = self.listeners().by_id.keys().copied().collect();
        self.send(&recipients, &message);
    }

    /// Tells the category's owner that it changed.
    pub fn broadcast_category_change(&self, category: &Category) {
        let message = UpdateCategoryMessage {
            action: "UPDATE_CATEGORY",
            workspace_id: &category.workspace_id,
            category: Some(category),
            board_categories: &[],
        };
        self.send_to_user(&category.workspace_id, &category.user_id, &message);
    }

    /// Tells a user that boards moved between their categories or were hidden.
    pub fn broadcast_category_board_change(
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
        self.send_to_user(workspace_id, user_id, &message);
    }

    /// Tells a user the new order of their categories.
    pub fn broadcast_category_reorder(
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
        self.send_to_user(workspace_id, user_id, &message);
    }

    /// Tells a user the new order of the boards in one of their categories.
    pub fn broadcast_category_boards_reorder(
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
        self.send_to_user(workspace_id, user_id, &message);
    }

    /// The workspace's listeners whose user is a member of the board, or is `also_user`.
    async fn listeners_for_workspace_and_board(
        &self,
        workspace_id: &str,
        board_id: &str,
        also_user: Option<&str>,
    ) -> Vec<ListenerId> {
        // Most changes happen with nobody listening; do not ask the store then.
        if !self.listeners().by_workspace.contains_key(workspace_id) {
            return Vec::new();
        }
        let members = match self.audience.members(board_id).await {
            Ok(members) => members,
            Err(error) => {
                tracing::error!(%error, workspace_id, board_id, "websocket: reading the board's members failed");
                return Vec::new();
            }
        };
        self.listeners_among(workspace_id, &members, also_user)
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
    fn send_to_user(&self, workspace_id: &str, user_id: &str, message: &impl Outgoing) {
        let recipients = self.listeners().of_user(workspace_id, user_id);
        self.send(&recipients, message);
    }

    /// Queues a message for the given listeners. A listener whose queue is full is
    /// dropped and its connection closed.
    fn send(&self, recipients: &[ListenerId], message: &impl Outgoing) {
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

/// Waits for the next tick of the interval, or forever if there is none.
async fn next_tick(interval: Option<&mut tokio::time::Interval>) {
    match interval {
        Some(interval) => {
            interval.tick().await;
        }
        None => std::future::pending().await,
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

/// Ports of `server/ws/server_test.go`, on the listener table.
#[cfg(test)]
mod tests;
