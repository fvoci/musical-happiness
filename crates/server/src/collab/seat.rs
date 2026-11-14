//! One websocket connection's place among the rooms: what the websocket hands its
//! text commands and binary frames to, and takes what is to be sent from.

use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use serde::Deserialize;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

use super::frame::{self, Carried, Received, Sync};
use super::messages::{self, reason_of};
use super::queue::Outgoing;
use super::rooms::{Connection, Entering, Holding};
use super::{Collab, caret, refusals};
use crate::app::{App, Asker};
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
    /// Of a `JOIN_DOC`: the making of the document the connection holds, if it names
    /// one (`state_vector`).
    epoch: i64,
    /// Of a `JOIN_DOC`: the state vector of the document the connection holds, in
    /// base64; empty for a connection that holds none.
    state_vector: String,
}

/// What a command or frame that is answered with a whole document counts as, in
/// frames: `JOIN_DOC`, and a sync step 1.
const WHOLE_DOCUMENT: u64 = 5;

/// How many seconds' worth of frames a connection may have saved up at most while
/// the server was at its steps.
const MOST_SAVED_SECONDS: u64 = 60;

/// How much a connection may send of the rooms: `per_second` frames a second, and
/// as many at once after a pause.
///
/// It is counted when the server comes to a frame, not when the client sent it: the
/// time the server spent on the connection's own steps is time the client could send
/// in, and what it could have sent then is not lost to it, so that a client within
/// its rate is not past it because the server was slow.
#[derive(Debug)]
struct FrameRate {
    per_second: u32,
    /// The thousandths of a frame the connection may send now.
    left: u64,
    /// Up to when `left` is counted.
    counted_to: Instant,
    /// How long, since then, the server spent on the connection's steps.
    at_steps: Duration,
}

impl FrameRate {
    fn new(per_second: u32) -> Self {
        Self {
            per_second,
            left: u64::from(per_second) * 1000,
            counted_to: Instant::now(),
            at_steps: Duration::ZERO,
        }
    }

    /// Counts `frames` more; whether they are within what the connection may send.
    fn admits(&mut self, frames: u64) -> bool {
        let rate = u64::from(self.per_second);
        let (full, most) = (rate * 1000, rate * 1000 * MOST_SAVED_SECONDS);
        // In whole milliseconds, each worth `rate` thousandths; the rest is counted
        // the next time.
        let millis = |time: Duration| u64::try_from(time.as_millis()).unwrap_or(u64::MAX);
        let passed = millis(Instant::now().duration_since(self.counted_to));
        let at_steps = millis(self.at_steps).min(passed);
        self.counted_to += Duration::from_millis(passed);
        self.at_steps = self
            .at_steps
            .saturating_sub(Duration::from_millis(at_steps));
        // What the connection did not send while it could is not saved past a
        // second's worth; what it could not send while the server was busy is.
        let while_free = (passed - at_steps).saturating_mul(rate);
        if self.left < full {
            self.left = self.left.saturating_add(while_free).min(full);
        }
        self.left = self
            .left
            .saturating_add(at_steps.saturating_mul(rate))
            .min(most);
        let cost = frames.saturating_mul(1000);
        let admitted = self.left >= cost;
        if admitted {
            self.left -= cost;
        }
        admitted
    }
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
    rate: FrameRate,
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
    /// The place among the rooms of a websocket connection of `asker` (a user that is
    /// empty for a connection without a session, which is let into no room), whose
    /// handshake reached `app`.
    #[must_use]
    pub fn seat(&self, app: &App, asker: Asker<'_>) -> (Seat, Outbox) {
        let connection = self.connection(asker);
        let seat = Seat {
            collab: self.clone(),
            home: app.clone(),
            connection: Arc::clone(&connection),
            next_doc: 1,
            rate: FrameRate::new(self.frames_per_second()),
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
    async fn step<T, F>(&mut self, work: F) -> Option<T>
    where
        T: Send + 'static,
        F: Future<Output = T> + Send + 'static,
    {
        let began = Instant::now();
        let done = self.home.spawn_tracked(work).await;
        self.rate.at_steps += began.elapsed();
        match done {
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
            epoch,
            state_vector,
            ..
        } = command;
        let admitted = self.rate.admits(WHOLE_DOCUMENT);
        if workspace_id.len() > LONGEST_ID || card_id.len() > LONGEST_ID {
            tracing::debug!("websocket: a JOIN_DOC names what is no identifier");
            return;
        }
        let doc = self.next_doc;
        let entered = if !admitted {
            Some(Err(refusals::too_many_frames(self.rate.per_second)))
        } else if let Some(next) = doc.checked_add(1) {
            self.next_doc = next;
            let (collab, home) = (self.collab.clone(), self.home.clone());
            let connection = Arc::clone(&self.connection);
            let (workspace, card) = (workspace_id.clone(), card_id.clone());
            let holding = Holding::of(epoch, &state_vector);
            // Boxed: what letting a connection in holds is large, and is no part of
            // what serving the connection holds.
            self.step(Box::pin(async move {
                let entering = Entering {
                    workspace_id: &workspace,
                    card_id: &card,
                    holding,
                };
                collab.enter(&connection, &home, doc, entering).await
            }))
            .await
        } else {
            None
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
        // Counted, and carried out whatever the count says: it makes room.
        let _ = self.rate.admits(1);
        let Some(room) = self.connection.forget(doc) else {
            return;
        };
        let (collab, connection) = (self.collab.clone(), Arc::clone(&self.connection));
        self.step(async move { collab.leave(&room, &connection, doc).await })
            .await;
    }

    /// Carries out a binary frame: a sync message or a caret for one of the
    /// connection's rooms. A frame for a room the connection is not in (it left, or
    /// was put out and told so), and one that is no frame of this shape, is dropped.
    pub async fn frame(&mut self, frame: Bytes) {
        // Counted whatever it is, and whatever it is for: a frame that is none, or
        // for no room of the connection's, costs a count too.
        let read = frame::room_of(frame).map(|(doc, rest)| (doc, frame::received(rest)));
        let asks_whole = |received: &Received| {
            matches!(
                received.carried,
                Carried::Sync {
                    sync: Sync::StateVector,
                    ..
                }
            )
        };
        let cost = match &read {
            Some((_, Some(received))) if asks_whole(received) => WHOLE_DOCUMENT,
            _ => 1,
        };
        let admitted = self.rate.admits(cost);
        let Some((doc, received)) = read else {
            return;
        };
        let Some(room) = self.connection.room(doc) else {
            return;
        };
        let (collab, connection) = (self.collab.clone(), Arc::clone(&self.connection));
        match received {
            Some(Received {
                epoch,
                carried: Carried::Carets(update),
            }) => {
                // One past the rate, or larger than a caret is, is dropped: the room
                // missed nothing of the document, and a caret is said again.
                if admitted && update.len() <= caret::MOST_BYTES {
                    self.step(async move {
                        collab.caret(&room, &connection, doc, epoch, update).await;
                    })
                    .await;
                }
            }
            _ if !admitted => {
                // What the frame carried is lost to the room: the connection is out
                // of step with it, and comes in anew.
                let reason = reason_of(&refusals::too_many_frames(self.rate.per_second));
                self.step(async move { collab.put_out(&room, &connection, doc, &reason).await })
                    .await;
            }
            Some(Received {
                epoch,
                carried: Carried::Sync { sync, payload },
            }) => {
                self.step(async move {
                    match sync {
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
            None => tracing::debug!(doc, "websocket: a binary frame that is no frame of a room"),
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A connection sends as many frames at once as it may in a second, and then
    /// one more with every part of a second that passes; a command or frame that is
    /// answered with a whole document counts as five.
    #[tokio::test(start_paused = true)]
    async fn frames_are_admitted_at_their_rate() {
        let mut rate = FrameRate::new(10);
        assert!((0..10).all(|_| rate.admits(1)), "ten at once");
        assert!(!rate.admits(1), "and no eleventh");
        tokio::time::advance(Duration::from_millis(99)).await;
        assert!(!rate.admits(1), "not yet a tenth of a second");
        tokio::time::advance(Duration::from_millis(1)).await;
        assert!(rate.admits(1), "one more");
        assert!(!rate.admits(1), "and only one");
        // Parts of a millisecond are not lost between two counts.
        for _ in 0..200 {
            tokio::time::advance(Duration::from_micros(500)).await;
            let _ = rate.admits(1);
        }
        tokio::time::advance(Duration::from_millis(500)).await;
        assert!(rate.admits(WHOLE_DOCUMENT), "five after half a second");
        assert!(!rate.admits(1), "which were all");
        // A pause saves a second's worth and no more.
        tokio::time::advance(Duration::from_secs(30)).await;
        assert!(
            rate.admits(WHOLE_DOCUMENT) && rate.admits(WHOLE_DOCUMENT),
            "ten again"
        );
        assert!(!rate.admits(1), "and no more for the wait");
    }

    /// What a client sent within its rate while the server was at the connection's
    /// steps is admitted when the server comes to it, all at once.
    #[tokio::test(start_paused = true)]
    async fn the_time_of_the_servers_steps_is_not_the_clients() {
        let mut rate = FrameRate::new(10);
        assert!((0..10).all(|_| rate.admits(1)), "ten at once");
        // A step of six seconds, during which sixty frames came in.
        tokio::time::advance(Duration::from_secs(6)).await;
        rate.at_steps += Duration::from_secs(6);
        assert!((0..60).all(|_| rate.admits(1)), "sixty that waited");
        assert!(!rate.admits(1), "and no more");
        // However long the server took, no more than a minute's worth is saved.
        tokio::time::advance(Duration::from_secs(600)).await;
        rate.at_steps += Duration::from_secs(600);
        assert!((0..600).all(|_| rate.admits(1)), "a minute's worth");
        assert!(!rate.admits(1), "and no more");
    }
}
