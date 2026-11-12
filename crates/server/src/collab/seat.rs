//! One websocket connection's place among the rooms: what the websocket hands its
//! text commands and binary frames to, and takes what is to be sent from.

use std::sync::Arc;

use bytes::Bytes;
use serde::Deserialize;
use tokio_util::sync::CancellationToken;

use super::Collab;
use super::frame::{self, Sync};
use super::messages::{self, reason_of};
use super::queue::Outgoing;
use super::rooms::Connection;
use crate::app::App;
use crate::error::AppError;

/// The longest identifier a command of the rooms may name: longer names nothing, and
/// is not answered, so that an answer never carries more than this of a command.
const LONGEST_ID: usize = 64;

/// A command of the rooms.
#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct Command {
    action: String,
    workspace_id: String,
    card_id: String,
    doc: u32,
}

/// The reading side of a connection among the rooms. Dropped, the connection is out
/// of every room.
#[derive(Debug)]
pub struct Seat {
    collab: Collab,
    /// The application as the connection's handshake reached it.
    home: App,
    connection: Arc<Connection>,
    /// The number the next room it comes into is known under on this connection.
    next_doc: u32,
}

/// The writing side of a connection among the rooms.
#[derive(Debug)]
pub struct Outbox {
    connection: Arc<Connection>,
}

impl Outbox {
    /// The next message to send to the connection; `None` once it ended. Waiting for
    /// it may be given up at any time.
    pub async fn next(&self) -> Option<Outgoing> {
        self.connection.queue.next().await
    }
}

impl Collab {
    /// The place among the rooms of a websocket connection of `user_id` (empty for a
    /// connection without a session, which is let into no room), whose handshake
    /// reached `app`.
    #[must_use]
    pub fn seat(&self, app: &App, user_id: &str) -> (Seat, Outbox) {
        let connection = self.connection(user_id);
        let seat = Seat {
            collab: self.clone(),
            home: app.clone(),
            connection: Arc::clone(&connection),
            next_doc: 1,
        };
        (seat, Outbox { connection })
    }

    /// The most bytes of a binary frame a client sends that is not refused for its
    /// size alone: the largest update and the head of its frame.
    #[must_use]
    pub fn frame_bytes(&self) -> usize {
        usize::try_from(self.inner.limits.update_bytes)
            .unwrap_or(usize::MAX)
            .saturating_add(frame::HEAD_ROOM)
    }
}

impl Seat {
    /// Cancelled when the connection is to be closed: it reads nothing of what it is
    /// sent, not even that it was put out of its rooms.
    #[must_use]
    pub fn overrun(&self) -> CancellationToken {
        self.connection.queue.overrun().clone()
    }

    /// Runs a step of a room in a task of its own and waits for it: a connection that
    /// ends meanwhile leaves the step done whole.
    async fn step<T, F>(&self, work: F) -> Option<T>
    where
        T: Send + 'static,
        F: Future<Output = T> + Send + 'static,
    {
        match self.home.spawn_tracked(work).await {
            Ok(done) => Some(done),
            Err(error) => {
                tracing::error!(%error, "a step of a room ended");
                None
            }
        }
    }

    /// Carries out a text message if it is a command of the rooms (`JOIN_DOC`,
    /// `LEAVE_DOC`); whether it was one.
    pub async fn command(&mut self, text: &str) -> bool {
        let Ok(command) = serde_json::from_str::<Command>(text) else {
            return false;
        };
        match command.action.as_str() {
            "JOIN_DOC" => self.join(command).await,
            "LEAVE_DOC" => self.leave(command.doc).await,
            _ => return false,
        }
        true
    }

    async fn join(&mut self, command: Command) {
        let Command {
            workspace_id,
            card_id,
            ..
        } = command;
        if workspace_id.len() > LONGEST_ID || card_id.len() > LONGEST_ID {
            tracing::debug!("websocket: a JOIN_DOC names what is no identifier");
            return;
        }
        let doc = self.next_doc;
        let entered = match doc.checked_add(1) {
            Some(next) => {
                self.next_doc = next;
                let (collab, home) = (self.collab.clone(), self.home.clone());
                let connection = Arc::clone(&self.connection);
                let (workspace, card) = (workspace_id.clone(), card_id.clone());
                self.step(async move {
                    collab
                        .enter(&connection, &home, doc, &workspace, &card)
                        .await
                })
                .await
            }
            None => None,
        };
        let refused = match entered {
            Some(Ok(())) => return,
            Some(Err(error)) => error,
            None => AppError::internal("the connection cannot come into a room"),
        };
        refused.log();
        self.connection.queue.notice(
            None,
            messages::refused(&workspace_id, &card_id, &reason_of(&refused)),
        );
    }

    async fn leave(&mut self, doc: u32) {
        let Some(room) = self.connection.forget(doc) else {
            return;
        };
        let (collab, connection) = (self.collab.clone(), Arc::clone(&self.connection));
        self.step(async move { collab.leave(&room, &connection, doc).await })
            .await;
    }

    /// Carries out a binary frame: a sync message for one of the connection's rooms.
    /// A frame for a room the connection is not in (it left, or was put out and told
    /// so), and one that is no frame of this shape, is dropped.
    pub async fn frame(&mut self, frame: Bytes) {
        let Some((doc, rest)) = frame::room_of(frame) else {
            return;
        };
        let Some(room) = self.connection.room(doc) else {
            return;
        };
        let Some(received) = frame::received(rest) else {
            tracing::debug!(doc, "websocket: a binary frame that is no sync message");
            return;
        };
        let (collab, connection) = (self.collab.clone(), Arc::clone(&self.connection));
        let (epoch, payload) = (received.epoch, received.payload);
        self.step(async move {
            match received.sync {
                Sync::StateVector => {
                    collab
                        .missing(&room, &connection, doc, epoch, payload)
                        .await;
                }
                Sync::Missing | Sync::Update => {
                    collab.update(&room, &connection, doc, epoch, payload).await;
                }
            }
        })
        .await;
    }

    /// Takes the connection out of every room, for good: it ended. The rooms are
    /// left in one step, which is done whole also if the wait for it is given up.
    pub async fn leave_all(&mut self) {
        self.connection.queue.close();
        let rooms = self.connection.end();
        if rooms.is_empty() {
            return;
        }
        let (collab, connection) = (self.collab.clone(), Arc::clone(&self.connection));
        self.step(async move {
            for (doc, room) in rooms {
                collab.leave(&room, &connection, doc).await;
            }
        })
        .await;
    }
}

impl Drop for Seat {
    /// A connection whose serving was given up before it left its rooms leaves them
    /// here.
    fn drop(&mut self) {
        self.connection.queue.close();
        let rooms = self.connection.end();
        if rooms.is_empty() {
            return;
        }
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            tracing::error!("a connection ended outside the runtime; its rooms keep it");
            return;
        };
        let (collab, connection) = (self.collab.clone(), Arc::clone(&self.connection));
        runtime.spawn(async move {
            for (doc, room) in rooms {
                collab.leave(&room, &connection, doc).await;
            }
        });
    }
}
