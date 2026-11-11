//! The metrics of the websocket connections: how many are open, the messages the hub
//! sends them by kind, and why they ended.

use prometheus::{IntCounterVec, IntGauge, Opts, Registry};

use super::{Metrics, PREFIX};

/// Why a websocket connection ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Disconnect {
    /// The client closed it, or went away.
    ClientClosed,
    /// What the client sent could not be read as websocket frames: a message over the
    /// limit, a broken frame.
    ReadError,
    /// Its session ended: logout, a changed password, the idle or absolute limit, or
    /// the user went.
    SessionEnded,
    /// The read token it followed a shared board with stopped opening the board.
    ShareEnded,
    /// It did not read its messages as fast as they came.
    SlowConsumer,
    /// The server stopped.
    Shutdown,
}

impl Disconnect {
    /// The value of the `reason` label.
    fn as_str(self) -> &'static str {
        match self {
            Self::ClientClosed => "client_closed",
            Self::ReadError => "read_error",
            Self::SessionEnded => "session_ended",
            Self::ShareEnded => "share_ended",
            Self::SlowConsumer => "slow_consumer",
            Self::Shutdown => "shutdown",
        }
    }
}

/// The metrics of the websocket connections.
#[derive(Debug)]
pub(super) struct WebsocketMetrics {
    connections: IntGauge,
    sent: IntCounterVec,
    disconnections: IntCounterVec,
}

impl WebsocketMetrics {
    /// Makes the metrics and registers them in `registry`.
    pub(super) fn register(registry: &Registry) -> prometheus::Result<Self> {
        let connections = IntGauge::with_opts(
            Opts::new(
                "websocket_connections",
                "How many websocket connections are open.",
            )
            .namespace(PREFIX),
        )?;
        let sent = IntCounterVec::new(
            Opts::new(
                "websocket_messages_sent_total",
                "How many messages the server queued for websocket connections, one per \
                 connection, by the message's action.",
            )
            .namespace(PREFIX),
            &["action"],
        )?;
        let disconnections = IntCounterVec::new(
            Opts::new(
                "websocket_disconnections_total",
                "How many websocket connections ended, by reason: client_closed, \
                 read_error, session_ended, share_ended, slow_consumer, shutdown.",
            )
            .namespace(PREFIX),
            &["reason"],
        )?;
        registry.register(Box::new(connections.clone()))?;
        registry.register(Box::new(sent.clone()))?;
        registry.register(Box::new(disconnections.clone()))?;
        Ok(Self {
            connections,
            sent,
            disconnections,
        })
    }
}

/// An open websocket connection, counted while it lives.
#[derive(Debug)]
pub(crate) struct OpenConnection(Option<IntGauge>);

impl Drop for OpenConnection {
    fn drop(&mut self) {
        if let Some(gauge) = &self.0 {
            gauge.dec();
        }
    }
}

impl Metrics {
    /// Counts a websocket connection as open until the returned value is dropped.
    pub(crate) fn websocket_opened(&self) -> OpenConnection {
        OpenConnection(self.kept.as_ref().map(|kept| {
            let gauge = kept.websocket.connections.clone();
            gauge.inc();
            gauge
        }))
    }

    /// Counts `count` messages of the action `action` as sent. The action is one of the
    /// hub's own, never one a client named.
    pub(crate) fn websocket_sent(&self, action: &'static str, count: usize) {
        if let Some(kept) = &self.kept
            && count > 0
        {
            kept.websocket
                .sent
                .with_label_values(&[action])
                .inc_by(u64::try_from(count).unwrap_or(u64::MAX));
        }
    }

    /// Counts a websocket connection as ended, for `reason`.
    pub(crate) fn websocket_closed(&self, reason: Disconnect) {
        if let Some(kept) = &self.kept {
            kept.websocket
                .disconnections
                .with_label_values(&[reason.as_str()])
                .inc();
        }
    }
}
