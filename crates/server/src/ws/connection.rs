//! The lifetime of a websocket connection and its command reader.

use futures_util::StreamExt as _;

use super::{
    Command, Hub, Identity, Listener, ListenerId, OUTBOX_CAPACITY, SESSION_CHECK_WITHOUT_PING,
    WRITER_GRACE, presence, subscribe, writer,
};
use crate::app::App;
use crate::collab::Seat;
use crate::error::ErrorKind;
use crate::metrics::Disconnect;
use axum::extract::ws::{Message, Utf8Bytes, WebSocket};
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

impl Hub {
    /// Serves one websocket connection until it closes. `identity` is the session the
    /// handshake came with, if any; without one it follows shared blocks and announces
    /// anonymous visits by read token. `app` is the application the handshake reached.
    pub async fn serve(&self, app: &App, socket: WebSocket, identity: Option<Identity>) {
        let (sink, mut stream) = socket.split();
        let (outbox, inbox) = mpsc::channel::<Utf8Bytes>(OUTBOX_CAPACITY);
        let closed = CancellationToken::new();
        let closed_because = Arc::new(OnceLock::new());
        let who = subscribe::Who::of(identity);
        let (mut docs, docs_outbox) = app.collab().seat(app, who.asker());
        let token = who.token;
        let docs_overrun = docs.overrun();
        self.watch_access_changes(app);

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
                    blocks: Vec::new(),
                    shared_boards: Vec::new(),
                    presence: presence::State::default(),
                },
            );
            id
        };
        let _connection = Connection {
            hub: self.clone(),
            id,
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
        let mut writer = writer::spawn(sink, inbox, docs_outbox, ping_interval);

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
            _ = &mut writer => Disconnect::ReadError,
            () = Box::pin(self.presence_loop(app, id)) => Disconnect::ReadError,
            () = docs_overrun.cancelled() => Disconnect::SlowConsumer,
            reason = self.read_commands(app, id, &mut stream, &mut docs) => reason,
            // The session ended by its idle or absolute limit, which nobody announces:
            // the sessions that ended so are deleted only by the hourly clean-up.
            () = self.session_ends(token.as_deref(), session_check) => Disconnect::SessionEnded,
        };
        metrics.websocket_closed(reason);
        self.listeners().remove(id);
        docs.leave_all().await;

        // Removing the listener drops its outbox, which ends the writer once it has sent
        // what was queued. A peer that does not read would hold the writer forever.
        self.listeners().remove(id);
        if !writer.is_finished()
            && tokio::time::timeout(WRITER_GRACE, &mut writer)
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
        app: &App,
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
            if !self.handle(app, id, command).await {
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
}

/// Removal also happens if the serving future is cancelled or unwinds.
struct Connection {
    hub: Hub,
    id: ListenerId,
}

impl Drop for Connection {
    fn drop(&mut self) {
        self.hub.listeners().remove(self.id);
    }
}
