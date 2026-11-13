//! The writing side of a websocket connection: what the hub queued for it, what the
//! rooms of the bodies edited together queued for it, and the pings.

use std::time::Duration;

use axum::extract::ws::{Message, Utf8Bytes, WebSocket};
use futures_util::SinkExt as _;
use futures_util::stream::SplitSink;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::collab::{Outbox, Outgoing};

/// Starts the task that writes to a connection until the hub drops `inbox`, which it
/// does when the connection ends, or the peer takes no more. `docs` is what the rooms
/// send; `ping_interval` is how often the connection is pinged, if at all.
pub(super) fn spawn(
    mut sink: SplitSink<WebSocket, Message>,
    mut inbox: mpsc::Receiver<Utf8Bytes>,
    docs: Outbox,
    ping_interval: Option<Duration>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut ping = ping_interval.map(|period| {
            let mut interval = tokio::time::interval(period);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            interval
        });
        // Until the rooms have nothing more for the connection: it left them all.
        let mut in_rooms = true;
        loop {
            let message = tokio::select! {
                message = inbox.recv() => match message {
                    Some(text) => Message::Text(text),
                    // The hub dropped this connection.
                    None => break,
                },
                outgoing = docs.next(), if in_rooms => match outgoing {
                    Some(Outgoing::Text(text)) => Message::Text(text.into()),
                    Some(Outgoing::Binary(frame)) => Message::Binary(frame),
                    None => {
                        in_rooms = false;
                        continue;
                    }
                },
                () = next_tick(ping.as_mut()) => Message::Ping(Vec::new().into()),
            };
            if sink.send(message).await.is_err() {
                break;
            }
        }
        let _ = sink.close().await;
    })
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
