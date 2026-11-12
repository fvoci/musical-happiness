//! The rooms: the connections that hold the document of one card's body together.
//!
//! One at a time works on a room (its turn): a connection that comes in is sent the
//! whole document, an update is taken into the document and logged
//! ([`Collab::take`]) and only then sent to the others, and what is sent anew
//! after the document was made again is sent to all, in the order the log has. A
//! room that the last connection left closes its document, which writes its body to
//! the card.
//!
//! While a room is there a task of its own hears what happens to its document
//! ([`Collab::subscribe`]): made again, or opened again after its worker ended, the
//! document is sent to everyone in the room anew.
//!
//! Every step of a room runs in a task of its own that the caller waits for
//! (`Seat`): a connection that ends while its update is taken leaves the update
//! logged and sent to the others, never logged and not sent.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bytes::Bytes;
use tokio::sync::broadcast;
use tokio::sync::broadcast::error::RecvError;
use tokio_util::sync::CancellationToken;

use super::frame::{self, Sync};
use super::messages::{self, reason_of};
use super::queue::{Capacity, Queue, Queued};
use super::{Collab, DocChange, DocEvent, Loaded, admission, place, refusals};
use crate::app::App;
use crate::error::{AppError, Reason};
use crate::permissions::DocMode;

/// How many messages may wait for one connection, of all its rooms.
pub(super) const QUEUE_MESSAGES: usize = 512;

/// How long after a room could not be sent its document anew, because the document
/// cannot be worked on for now, it is tried again.
const SEND_ANEW_AGAIN: Duration = Duration::from_secs(2);

/// How many times the bytes of the largest document may wait for one connection, of
/// all its rooms: a document whole, and as much again of what came after it.
const QUEUE_DOCUMENTS: usize = 2;

/// The rooms there are.
#[derive(Debug, Default)]
pub(super) struct Rooms {
    by_key: Mutex<HashMap<String, Arc<Room>>>,
    /// The number of the next connection.
    next_connection: AtomicU64,
    /// The server stops: nobody comes in any more, and no update is taken.
    ended: AtomicBool,
}

/// A connection as the rooms know it.
#[derive(Debug)]
pub(super) struct Connection {
    id: u64,
    /// The user of the connection's session; empty for a connection without one.
    pub(super) user_id: String,
    pub(super) queue: Queue,
    joined: Mutex<Joined>,
}

/// The rooms a connection is in.
#[derive(Debug, Default)]
struct Joined {
    /// By the number each is known under on this connection.
    rooms: HashMap<u32, Arc<Room>>,
    /// The connection ended: it comes into no room any more.
    ended: bool,
}

impl Connection {
    /// The room the connection knows as `doc`, if it is in it.
    pub(super) fn room(&self, doc: u32) -> Option<Arc<Room>> {
        self.joined().rooms.get(&doc).map(Arc::clone)
    }

    /// Takes the room the connection knows as `doc` from those it is in.
    pub(super) fn forget(&self, doc: u32) -> Option<Arc<Room>> {
        self.joined().rooms.remove(&doc)
    }

    /// Takes every room from those it is in, for good: the connection ended, and a
    /// step that would let it into a room after this finds that it did.
    pub(super) fn end(&self) -> Vec<(u32, Arc<Room>)> {
        let mut joined = self.joined();
        joined.ended = true;
        joined.rooms.drain().collect()
    }

    /// Counts a room among those the connection is in, as `doc`; `false` if the
    /// connection ended, and is in no room.
    fn join(&self, doc: u32, room: &Arc<Room>) -> bool {
        let mut joined = self.joined();
        if joined.ended {
            return false;
        }
        joined.rooms.insert(doc, Arc::clone(room));
        true
    }

    fn joined(&self) -> std::sync::MutexGuard<'_, Joined> {
        self.joined
            .lock()
            .expect("the rooms of a connection are never held across a panic")
    }
}

/// The connections that hold one card's document.
#[derive(Debug)]
pub(super) struct Room {
    /// Its key among the rooms: the card's place ([`place::key_of`]).
    key: String,
    card_id: String,
    /// The application as the card's organization reaches it.
    app: App,
    turn: tokio::sync::Mutex<Turn>,
    /// Cancelled when the room is closed: its task ends.
    gone: CancellationToken,
}

/// What the one that works on a room holds.
#[derive(Debug, Default)]
struct Turn {
    /// Which making of the document the members hold.
    epoch: i64,
    members: Vec<Member>,
    /// The room is no longer among the rooms: who waited for it looks again.
    closed: bool,
}

#[derive(Debug)]
struct Member {
    connection: Arc<Connection>,
    /// The number the connection knows the room under.
    doc: u32,
    mode: DocMode,
}

impl Turn {
    fn index_of(&self, connection: &Connection, doc: u32) -> Option<usize> {
        self.members
            .iter()
            .position(|member| member.connection.id == connection.id && member.doc == doc)
    }

    /// Puts a member out of the room and tells its connection why, in place of what
    /// waited for it of the room.
    fn put_out(&mut self, index: usize, reason: &str) {
        let member = self.members.swap_remove(index);
        member.connection.forget(member.doc);
        member
            .connection
            .queue
            .notice(Some(member.doc), messages::left(member.doc, reason));
    }

    /// Queues what `make` makes for each member `to` picks. A member whose connection
    /// has no room for it is put out: it does not read, or it ended, and the others
    /// go on.
    fn send<const N: usize>(
        &mut self,
        to: impl Fn(&Member) -> bool,
        make: impl Fn(&Member) -> [Queued; N],
    ) {
        let mut index = 0;
        while index < self.members.len() {
            let member = &self.members[index];
            if to(member) && member.connection.queue.push(make(member)).is_err() {
                self.put_out(index, Reason::ConnectionTooSlow.as_str());
            } else {
                index += 1;
            }
        }
    }

    /// Sends every member the document anew, in place of the sync messages that
    /// waited for them: those were of the document before, or are in this one.
    fn send_anew(&mut self, loaded: &Loaded) {
        self.epoch = loaded.epoch;
        let epoch = loaded.epoch;
        let whole = frame::message(Sync::Missing, &loaded.state);
        let mut index = 0;
        while index < self.members.len() {
            let member = &self.members[index];
            let doc = member.doc;
            let anew = [
                Queued::text(doc, messages::reset(doc, epoch)),
                Queued::sync(doc, epoch, &whole),
            ];
            if member.connection.queue.push_anew(doc, anew).is_err() {
                self.put_out(index, Reason::ConnectionTooSlow.as_str());
            } else {
                index += 1;
            }
        }
    }
}

impl Collab {
    /// A connection of `user_id` for the rooms, in none yet.
    pub(super) fn connection(&self, user_id: &str) -> Arc<Connection> {
        let rooms = &self.inner.rooms;
        Arc::new(Connection {
            id: rooms.next_connection.fetch_add(1, Ordering::Relaxed),
            user_id: user_id.to_owned(),
            queue: Queue::new(Capacity {
                messages: QUEUE_MESSAGES,
                bytes: usize::try_from(self.inner.limits.state_bytes)
                    .unwrap_or(usize::MAX)
                    .saturating_mul(QUEUE_DOCUMENTS),
            }),
            joined: Mutex::new(Joined::default()),
        })
    }

    /// Nobody comes into a room any more, and the rooms take no update: the server
    /// stops.
    pub(super) fn end_rooms(&self) {
        self.inner.rooms.ended.store(true, Ordering::Relaxed);
    }

    fn rooms_ended(&self) -> bool {
        self.inner.rooms.ended.load(Ordering::Relaxed)
    }

    /// How many rooms there are, for the tests of what a connection leaves behind.
    #[cfg(feature = "test-util")]
    #[doc(hidden)]
    #[must_use]
    pub fn rooms(&self) -> usize {
        self.inner
            .rooms
            .by_key
            .lock()
            .expect("the rooms are never held across a panic")
            .len()
    }

    /// The room of a card of `app`'s organization, made if there is none, with the
    /// task that hears what happens to its document.
    fn room(&self, app: &App, card_id: &str) -> Arc<Room> {
        let key = place::key_of(app.store().tenant(), card_id);
        let mut rooms = self
            .inner
            .rooms
            .by_key
            .lock()
            .expect("the rooms are never held across a panic");
        if let Some(room) = rooms.get(&key) {
            return Arc::clone(room);
        }
        let room = Arc::new(Room {
            key: key.clone(),
            card_id: card_id.to_owned(),
            app: app.clone(),
            turn: tokio::sync::Mutex::new(Turn::default()),
            gone: CancellationToken::new(),
        });
        rooms.insert(key, Arc::clone(&room));
        // Subscribed before anyone is in the room: whatever happens to the document
        // from its first opening on is heard.
        let events = self.subscribe();
        tokio::spawn(self.clone().watch(Arc::clone(&room), events));
        room
    }

    /// Hears what happens to the document of a room for as long as the room is
    /// there, and has the room sent the document anew.
    async fn watch(self, room: Arc<Room>, mut events: broadcast::Receiver<DocEvent>) {
        let tenant = room.app.store().tenant();
        // Whether the room is still to be sent its document: it could not be opened.
        let mut owed = false;
        loop {
            let heard = tokio::select! {
                () = room.gone.cancelled() => return,
                () = tokio::time::sleep(SEND_ANEW_AGAIN), if owed => None,
                event = events.recv() => match event {
                    Ok(event) if event.tenant == tenant && event.card_id == room.card_id => {
                        Some(event.change)
                    }
                    Ok(_) => continue,
                    // Some were missed: the document may be another by now.
                    Err(RecvError::Lagged(_)) => None,
                    Err(RecvError::Closed) => return,
                },
            };
            let mut turn = room.turn.lock().await;
            // The server stops: the documents are written and closed, and none is
            // opened again for a room whose connections are about to end.
            if turn.closed || self.rooms_ended() {
                return;
            }
            // Nobody yet: who comes in opens the document as it is.
            if turn.members.is_empty() {
                continue;
            }
            owed = !self.anew(&room, &mut turn, heard).await;
            self.shut_if_empty(&room, &mut turn).await;
        }
    }

    /// Lets a connection into the room of a card of a board of `workspace_id`, to be
    /// known there as `doc`, and sends it `DOC_JOINED` and the whole document. A
    /// connection that is in the room already is in it anew: what it knew the room
    /// as before is no room of its any more. `home` is the application as the
    /// connection's handshake reached it.
    ///
    /// # Errors
    ///
    /// Returns a not-found error for who is not let in ([`admission::admit`]), which
    /// is also what a card that is not there answers, the errors of opening the
    /// document ([`Collab::open`]), and `connection_too_slow` for a connection that
    /// has no room for the document among what waits for it. A connection that is
    /// refused stays in the room as it was.
    pub(super) async fn enter(
        &self,
        connection: &Arc<Connection>,
        home: &App,
        doc: u32,
        workspace_id: &str,
        card_id: &str,
    ) -> Result<(), AppError> {
        let user_id = &connection.user_id;
        let admitted = admission::admit(home, user_id, workspace_id, card_id)
            .await?
            .ok_or_else(|| AppError::not_found("card"))?;
        loop {
            let room = self.room(&admitted.app, card_id);
            let mut turn = room.turn.lock().await;
            if turn.closed {
                continue;
            }
            if self.rooms_ended() {
                self.shut_if_empty(&room, &mut turn).await;
                return Err(refusals::unavailable());
            }
            let loaded = match self.open(&room.app, card_id).await {
                Ok(loaded) => loaded,
                Err(error) => {
                    self.shut_if_empty(&room, &mut turn).await;
                    return Err(error);
                }
            };
            let epoch = loaded.epoch;
            let whole = frame::message(Sync::Missing, &loaded.state);
            let joined = messages::joined(workspace_id, card_id, doc, epoch, admitted.mode);
            let queued = connection
                .queue
                .push([Queued::text(doc, joined), Queued::sync(doc, epoch, &whole)]);
            // Counted among the connection's rooms before it is a member: a connection
            // that ended meanwhile is no member of a room nobody would take it out of.
            if queued.is_err() || !connection.join(doc, &room) {
                connection.queue.forget(doc);
                self.shut_if_empty(&room, &mut turn).await;
                return Err(refusals::too_slow());
            }
            if let Some(index) = turn
                .members
                .iter()
                .position(|member| member.connection.id == connection.id)
            {
                let before = turn.members.swap_remove(index);
                connection.forget(before.doc);
                connection.queue.forget(before.doc);
            }
            if !turn.members.is_empty() && turn.epoch != epoch {
                // Made again since the members were sent it, and not yet told.
                turn.send_anew(&loaded);
            }
            turn.epoch = epoch;
            turn.members.push(Member {
                connection: Arc::clone(connection),
                doc,
                mode: admitted.mode,
            });
            return Ok(());
        }
    }

    /// Takes a connection out of a room it knew as `doc`.
    pub(super) async fn leave(&self, room: &Arc<Room>, connection: &Connection, doc: u32) {
        let mut turn = room.turn.lock().await;
        if let Some(index) = turn.index_of(connection, doc) {
            turn.members.swap_remove(index);
        }
        connection.queue.forget(doc);
        self.shut_if_empty(room, &mut turn).await;
    }

    /// Takes an update a connection sent into the room's document, and sends it on:
    /// `DOC_ACK` to the connection once the log has it, the update to the others, and
    /// what a repair deleted to all. An update that is not taken is answered with
    /// `DOC_NACK` and goes to nobody.
    pub(super) async fn update(
        &self,
        room: &Arc<Room>,
        connection: &Connection,
        doc: u32,
        epoch: i64,
        update: Bytes,
    ) {
        let mut turn = room.turn.lock().await;
        let Some(index) = turn.index_of(connection, doc) else {
            return;
        };
        let sender = connection.id;
        let to_sender = |member: &Member| member.connection.id == sender;
        let refusal = if turn.members[index].mode == DocMode::Read {
            Some(reason_of(&refusals::read_only()))
        } else if self.rooms_ended() {
            Some(reason_of(&refusals::unavailable()))
        } else if epoch == turn.epoch {
            None
        } else {
            Some(reason_of(&refusals::out_of_step()))
        };
        let taken = match refusal {
            Some(reason) => Err(reason),
            None => self
                .take(
                    &room.app,
                    &room.card_id,
                    epoch,
                    &connection.user_id,
                    &update,
                )
                .await
                .map_err(|error| {
                    error.log();
                    reason_of(&error)
                }),
        };
        match taken {
            Err(reason) => turn.send(to_sender, |_| {
                [Queued::text(doc, messages::nack(doc, epoch, &reason))]
            }),
            Ok(taken) => {
                turn.send(to_sender, |_| {
                    [Queued::text(doc, messages::ack(doc, epoch, taken.seq))]
                });
                if taken.remade {
                    // The update is in the body the document was made again from.
                    // If it cannot be opened for now, the room's task hears of the
                    // making and tries again.
                    self.anew(room, &mut turn, None).await;
                } else {
                    let update = frame::message(Sync::Update, &update);
                    turn.send(
                        |member| !to_sender(member),
                        |member| [Queued::sync(member.doc, epoch, &update)],
                    );
                    if let Some(repair) = &taken.repair {
                        let repair = frame::message(Sync::Update, repair);
                        turn.send(
                            |_| true,
                            |member| [Queued::sync(member.doc, epoch, &repair)],
                        );
                    }
                }
            }
        }
        self.shut_if_empty(room, &mut turn).await;
    }

    /// Sends a connection what the room's document has that a holder of
    /// `state_vector` has not, as a sync step 2: the answer to a sync step 1. What
    /// cannot be answered (another epoch, bytes that are no state vector) is not.
    pub(super) async fn missing(
        &self,
        room: &Arc<Room>,
        connection: &Connection,
        doc: u32,
        epoch: i64,
        state_vector: Bytes,
    ) {
        let mut turn = room.turn.lock().await;
        if turn.index_of(connection, doc).is_none() || epoch != turn.epoch || self.rooms_ended() {
            return;
        }
        match self
            .diff(&room.app, &room.card_id, epoch, &state_vector)
            .await
        {
            Ok(missing) => {
                let missing = frame::message(Sync::Missing, &missing);
                turn.send(
                    |member| member.connection.id == connection.id,
                    |_| [Queued::sync(doc, epoch, &missing)],
                );
            }
            Err(error) => error.log(),
        }
        self.shut_if_empty(room, &mut turn).await;
    }

    /// Sends every member of a held room the document as it is now, after `heard`
    /// happened to it (`None` if what happened is not known). A document that was
    /// made again and is the one the members hold already is not sent: who came in
    /// since, or the update that made it again, brought it.
    ///
    /// `false` if the document cannot be worked on for now (`document_unavailable`):
    /// the members stay, with the document they hold, and are still to be sent it.
    /// If it cannot be opened for another reason (its card is gone, the server holds
    /// as many documents as it may) the members are put out, with that reason.
    async fn anew(&self, room: &Room, turn: &mut Turn, heard: Option<DocChange>) -> bool {
        let loaded = match self.open(&room.app, &room.card_id).await {
            Ok(loaded) => loaded,
            Err(error) if error.reason() == Some(Reason::DocumentUnavailable) => return false,
            Err(error) => {
                error.log();
                let reason = reason_of(&error);
                while !turn.members.is_empty() {
                    turn.put_out(0, &reason);
                }
                return true;
            }
        };
        let made_again = matches!(heard, Some(DocChange::Remade | DocChange::Replaced));
        if made_again && loaded.epoch == turn.epoch {
            // Sent already, by the join or the update that found it made again.
            return true;
        }
        turn.send_anew(&loaded);
        true
    }

    /// Closes a held room that nobody is in: its document is closed, which writes its
    /// body to the card, and whoever comes next makes the room anew. The room is held
    /// until the document is closed, so that the next opens it after.
    async fn shut_if_empty(&self, room: &Arc<Room>, turn: &mut Turn) {
        if !turn.members.is_empty() || turn.closed {
            return;
        }
        turn.closed = true;
        room.gone.cancel();
        if let Err(error) = self.close(&room.app, &room.card_id).await {
            tracing::error!(%error, card_id = room.card_id, "cannot write a card's body as its room closes");
        }
        let mut rooms = self
            .inner
            .rooms
            .by_key
            .lock()
            .expect("the rooms are never held across a panic");
        if rooms
            .get(&room.key)
            .is_some_and(|there| Arc::ptr_eq(there, room))
        {
            rooms.remove(&room.key);
        }
    }
}
