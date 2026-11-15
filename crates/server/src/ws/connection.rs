//! The lifetime of a websocket connection and its command reader.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use axum::extract::ws::{Message, Utf8Bytes, WebSocket};
use futures_util::{Stream, StreamExt as _};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use super::inbound::{Came, Inbound};
use super::{
    Command, Hub, Identity, Listener, ListenerId, OUTBOX_CAPACITY, SESSION_CHECK_WITHOUT_PING,
    WRITER_GRACE, presence, subscribe, writer,
};
use crate::app::App;
use crate::auth::limiter::AddressKey;
use crate::collab::Seat;
use crate::error::ErrorKind;
use crate::metrics::Disconnect;

impl Hub {
    /// Serves one websocket connection until it closes. `identity` is the session the
    /// handshake came with, if any; without one it follows shared blocks and announces
    /// anonymous visits by read token. `app` is the application the handshake reached,
    /// `address` the client's (none through the administration socket).
    pub async fn serve(
        &self,
        app: &App,
        socket: WebSocket,
        identity: Option<Identity>,
        address: Option<IpAddr>,
    ) {
        let (sink, stream) = socket.split();
        let (outbox, inbox) = mpsc::channel::<Utf8Bytes>(OUTBOX_CAPACITY);
        let mut inbound = Inbound::new(stream, outbox.clone(), self.quiet_limit());
        let closed = CancellationToken::new();
        let closed_because = Arc::new(OnceLock::new());
        let mut who = subscribe::Who::of(identity);
        let (mut docs, docs_outbox) = app.collab().seat(app, who.asker());
        let token = who.token.take();
        let docs_overrun = docs.overrun();
        self.watch_access_changes(app);

        let id = self.add_listener(Listener {
            address: address.map(AddressKey::from),
            closed: closed.clone(),
            closed_because: Arc::clone(&closed_because),
            ..Listener::new(who, outbox)
        });
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
            self.listeners().remove(id);
            if error.kind() == ErrorKind::Unauthorized {
                metrics.websocket_closed(Disconnect::SessionEnded);
            } else {
                // Not ended, unread: the page is told to connect again, and does with
                // its spread wait.
                tracing::warn!(%error, "a websocket connection ends: its session could not be read");
                metrics.websocket_closed(Disconnect::StoreUnavailable);
                writer::close(sink, Some(Disconnect::StoreUnavailable)).await;
            }
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
            // The hub gave the connection up: it does not keep up, its session ended, or
            // what it may be told could not be read.
            () = closed.cancelled() => {
                closed_because.get().copied().unwrap_or(Disconnect::Shutdown)
            }
            // It reads nothing of what the rooms send it.
            _ = &mut writer => Disconnect::ReadError,
            never = Box::pin(self.presence_loop(app, id)) => match never {},
            () = docs_overrun.cancelled() => Disconnect::SlowConsumer,
            reason = self.read_commands(app, id, &mut inbound, &mut docs) => reason,
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
        if !writer.is_finished()
            && tokio::time::timeout(WRITER_GRACE, &mut writer)
                .await
                .is_err()
        {
            writer.abort();
        }
    }

    /// Counts a connection among the hub's, under a number of its own.
    fn add_listener(&self, listener: Listener) -> ListenerId {
        let mut listeners = self.listeners();
        listeners.next_id += 1;
        let id = listeners.next_id;
        listeners.by_id.insert(id, listener);
        id
    }

    /// Reads the commands of a connection and carries them out, until the client
    /// closes it, goes away, sends what is no websocket frame or nothing at all for
    /// the quiet limit; returns which. It reads on while it carries one out
    /// ([`Inbound::meanwhile`]).
    async fn read_commands(
        &self,
        app: &App,
        id: ListenerId,
        inbound: &mut Inbound<impl Stream<Item = Result<Message, axum::Error>> + Unpin>,
        docs: &mut Seat,
    ) -> Disconnect {
        loop {
            let text = match inbound.next().await {
                Ok(Came::Text(text)) => text,
                Ok(Came::Binary(frame)) => {
                    inbound.meanwhile(docs.frame(frame)).await;
                    continue;
                }
                Err(reason) => return reason,
            };
            // A command is small, whatever a frame of a body edited together may be.
            if text.len() > crate::api::WEBSOCKET_MESSAGE_LIMIT {
                return Disconnect::ReadError;
            }
            if inbound.meanwhile(docs.command(&text)).await {
                continue;
            }
            let Ok(command) = serde_json::from_str::<Command>(&text) else {
                tracing::error!("websocket: a command could not be parsed");
                continue;
            };
            // No command closes a connection now; were one to, the client asked for it.
            // Boxed: what a command holds is large, and is no part of what reading holds.
            if !Box::pin(inbound.meanwhile(self.handle(app, id, command))).await {
                return Disconnect::ClientClosed;
            }
        }
    }

    /// How long a connection may send nothing: [`super::inbound::QUIET_LIMIT`], or
    /// what a test set.
    fn quiet_limit(&self) -> Duration {
        #[cfg(feature = "test-util")]
        if let Some(limit) = *self.inner.quiet_limit.lock().expect("lock poisoned") {
            return limit;
        }
        super::inbound::QUIET_LIMIT
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

impl Listener {
    /// A connection of `who` (whose token is no more needed) that follows nothing yet,
    /// from no known address, sending to `outbox`.
    fn new(who: subscribe::Who, outbox: mpsc::Sender<Utf8Bytes>) -> Self {
        Self {
            user_id: who.user_id,
            session_id: who.session_id,
            login: who.login,
            address: None,
            outbox,
            closed: CancellationToken::new(),
            closed_because: Arc::default(),
            workspaces: Vec::new(),
            workspace_roles: HashMap::new(),
            blocks: Vec::new(),
            shared_boards: Vec::new(),
            presence: presence::State::default(),
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
