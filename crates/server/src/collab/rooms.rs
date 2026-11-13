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
//! document is sent to everyone in the room anew. Another asks again, every
//! `collab_recheck_seconds`, whether each connection may be in the room, and how
//! ([`admission::admit`]): who may no longer read is put out, and who may no longer
//! write reads from then on. When an organization begins to take only sessions made
//! through its provider, whoever it shuts out is put out of its rooms at once
//! ([`Collab::put_out_shut_out`]).
//!
//! Every step of a room runs in a task of its own that the caller waits for
//! (`Seat`): a connection that ends while its update is taken leaves the update
//! logged and sent to the others, never logged and not sent.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bytes::Bytes;
use tokio::sync::broadcast;
use tokio::sync::broadcast::error::RecvError;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

use super::frame::{self, Sync};
use super::messages::{self, reason_of};
use super::queue::{Capacity, NotQueued, Queue, Queued};
use super::{Collab, DocChange, DocEvent, Loaded, admission, place, refusals};
use crate::app::{App, Asker};
use crate::config::collab::CollabSettings;
use crate::error::{AppError, Reason};
use crate::permissions::{DocMode, session_opens};

/// How long after a room could not be sent its document anew, because the document
/// cannot be worked on for now, it is tried again.
const SEND_ANEW_AGAIN: Duration = Duration::from_secs(2);

/// How long after the store did not answer of somebody in a room it is asked of
/// them once more, before they are put out.
const ASKED_ONCE_MORE_AFTER: Duration = Duration::from_secs(2);

/// How many times a connection coming into a room is admitted again because an
/// organization began to take only its provider's sessions meanwhile, before it is
/// refused as when the document cannot be worked on.
const ADMITTED_AT_MOST: usize = 3;

/// How many users of a room the store is asked about at once, when the room asks
/// again who may be in it.
const ASKED_AT_ONCE: usize = 8;

/// How many times the bytes of the largest document may wait for one connection, of
/// all its rooms: a document whole, and as much again of what came after it.
const QUEUE_DOCUMENTS: usize = 2;

/// The rooms there are.
#[derive(Debug)]
pub(super) struct Rooms {
    by_key: Mutex<HashMap<String, Arc<Room>>>,
    /// How often a room asks again whether its members may be in it.
    recheck: Duration,
    /// In how many rooms a connection may be.
    per_connection: usize,
    /// How many connections may be in a room.
    per_room: usize,
    /// How many of the connections in a room may be one user's.
    per_user: usize,
    /// How many commands and frames a connection may send in a second.
    frames_per_second: u32,
    /// How many messages may wait for a connection, of all its rooms.
    queue_messages: usize,
    /// The number of the next connection.
    next_connection: AtomicU64,
    /// How many times an organization began to take only sessions made through its
    /// provider: a connection let in before that is asked again as it comes in.
    walls_raised: AtomicU64,
    /// Cancelled when the server stops: nobody comes in any more, no update is taken,
    /// and the tasks of the rooms end.
    stopping: CancellationToken,
}

impl Rooms {
    /// No rooms, held to what `settings` say.
    pub(super) fn new(settings: &CollabSettings) -> Self {
        Self {
            by_key: Mutex::new(HashMap::new()),
            recheck: Duration::from_secs(settings.collab_recheck_seconds),
            per_connection: as_count(settings.collab_documents_per_connection),
            per_room: as_count(settings.collab_connections_per_document),
            per_user: as_count(settings.collab_connections_per_user),
            frames_per_second: settings.collab_frames_per_second,
            queue_messages: as_count(settings.collab_queue_messages),
            next_connection: AtomicU64::new(0),
            walls_raised: AtomicU64::new(0),
            stopping: CancellationToken::new(),
        }
    }
}

/// Waits until `at`, or for ever if there is no such time.
async fn wait_until(at: Option<Instant>) {
    match at {
        Some(at) => tokio::time::sleep_until(at).await,
        None => std::future::pending().await,
    }
}

/// A setting that counts things, as the count of a collection.
fn as_count(setting: u32) -> usize {
    usize::try_from(setting).unwrap_or(usize::MAX)
}

/// A connection as the rooms know it.
#[derive(Debug)]
pub(super) struct Connection {
    id: u64,
    /// The user of the connection's session; empty for a connection without one.
    pub(super) user_id: String,
    /// The provider the connection's session was made through; none for a password,
    /// and for a connection without a session.
    pub(super) made_through: Option<String>,
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
    /// Whose the connection is, for what is asked of an organization for it.
    pub(super) fn asker(&self) -> Asker<'_> {
        Asker {
            user_id: &self.user_id,
            made_through: self.made_through.as_deref(),
        }
    }

    /// The room the connection knows as `doc`, if it is in it.
    pub(super) fn room(&self, doc: u32) -> Option<Arc<Room>> {
        self.joined().rooms.get(&doc).map(Arc::clone)
    }

    /// Takes the room the connection knows as `doc` from those it is in.
    pub(super) fn forget(&self, doc: u32) -> Option<Arc<Room>> {
        self.joined().rooms.remove(&doc)
    }

    /// Whether the connection is in as many rooms as `limit` and in none of the card
    /// `card_id`: it comes into no room of that card, whatever else is asked.
    fn is_full_without(&self, limit: usize, card_id: &str) -> bool {
        let joined = self.joined();
        joined.rooms.len() >= limit && !joined.rooms.values().any(|room| room.card_id == card_id)
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
    /// The workspace the connection named as it came in: the card's board is of it.
    workspace_id: String,
    mode: DocMode,
}

/// A member as a room asks about it again.
#[derive(Debug)]
struct Asked {
    connection: u64,
    doc: u32,
    who: Who,
}

/// Whom the store is asked about: a user, the provider their session was made
/// through, and the workspace they named as they came in.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct Who {
    user_id: String,
    made_through: Option<String>,
    workspace_id: String,
}

impl Who {
    /// Who asks, for what is asked of an organization for them.
    fn asker(&self) -> Asker<'_> {
        Asker {
            user_id: &self.user_id,
            made_through: self.made_through.as_deref(),
        }
    }
}

/// What the store answered of one: how they are in the room, `None` for who is not
/// let in; or the reason it could not be asked.
type Answer = Result<Option<DocMode>, String>;

impl Turn {
    /// How many of the members are connections of the user.
    fn of_user(&self, user_id: &str) -> usize {
        self.members
            .iter()
            .filter(|member| member.connection.user_id == user_id)
            .count()
    }

    fn index_of(&self, connection: &Connection, doc: u32) -> Option<usize> {
        self.members
            .iter()
            .position(|member| member.connection.id == connection.id && member.doc == doc)
    }

    /// Puts a member out of the room and tells its connection why, in place of the
    /// sync messages that waited for it of the room and after the answers it was
    /// given there.
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
        self.queue_for_each(|member| {
            if to(member) {
                member.connection.queue.push(make(member))
            } else {
                Ok(())
            }
        });
    }

    /// Queues the answer to something a connection sent (`DOC_ACK`, `DOC_NACK`) for
    /// it, whatever waits for it: the answer to an update that is logged is never
    /// lost. A connection for which it was one too many is put out after it.
    fn answer(&mut self, connection: &Connection, doc: u32, answer: String) {
        let Some(index) = self.index_of(connection, doc) else {
            return;
        };
        if connection.queue.answer(Queued::text(doc, answer)).is_err() {
            self.put_out(index, Reason::ConnectionTooSlow.as_str());
        }
    }

    /// Has `queue` queue something for each member, and puts out who has no room
    /// for it.
    fn queue_for_each(&mut self, queue: impl Fn(&Member) -> Result<(), NotQueued>) {
        let mut index = 0;
        while index < self.members.len() {
            if queue(&self.members[index]).is_err() {
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
        self.queue_for_each(|member| {
            let doc = member.doc;
            let anew = [
                Queued::text(doc, messages::reset(doc, epoch)),
                Queued::sync(doc, epoch, &whole),
            ];
            member.connection.queue.push_anew(doc, anew)
        });
    }
}

impl Collab {
    /// A connection of `asker` for the rooms, in none yet.
    pub(super) fn connection(&self, asker: Asker<'_>) -> Arc<Connection> {
        let rooms = &self.inner.rooms;
        Arc::new(Connection {
            id: rooms.next_connection.fetch_add(1, Ordering::Relaxed),
            user_id: asker.user_id.to_owned(),
            made_through: asker.made_through.map(str::to_owned),
            queue: Queue::new(Capacity {
                messages: rooms.queue_messages,
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
        self.inner.rooms.stopping.cancel();
    }

    /// How many commands and frames of the rooms a connection may send in a second.
    pub(super) fn frames_per_second(&self) -> u32 {
        self.inner.rooms.frames_per_second
    }

    fn rooms_ended(&self) -> bool {
        self.inner.rooms.stopping.is_cancelled()
    }

    /// Whether a card of `app`'s organization has a room.
    pub(super) fn has_room(&self, app: &App, card_id: &str) -> bool {
        let key = place::key_of(app.store().tenant(), card_id);
        self.inner
            .rooms
            .by_key
            .lock()
            .expect("the rooms are never held across a panic")
            .contains_key(&key)
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

    /// The room of a card of `app`'s organization, made if there is none, with its
    /// two tasks: the one that hears what happens to its document, and the one that
    /// asks again whether its members may be in it.
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
        tokio::spawn(self.clone().ask_again(Arc::clone(&room)));
        room
    }

    /// Hears what happens to the document of a room for as long as the room is
    /// there, and has the room sent the document anew.
    async fn watch(self, room: Arc<Room>, mut events: broadcast::Receiver<DocEvent>) {
        let tenant = room.app.store().tenant();
        let stopping = self.inner.rooms.stopping.clone();
        // When the room is tried to be sent its document again, which could not be
        // opened: kept across what is heard of other cards meanwhile.
        let mut again_at: Option<Instant> = None;
        loop {
            let heard = tokio::select! {
                () = room.gone.cancelled() => return,
                () = stopping.cancelled() => return,
                () = wait_until(again_at) => None,
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
            again_at = if self.anew(&room, &mut turn, heard).await {
                None
            } else {
                Some(Instant::now() + SEND_ANEW_AGAIN)
            };
            self.shut_if_empty(&room, &mut turn).await;
        }
    }

    /// Asks again, every `collab_recheck_seconds`, whether each member of a room may
    /// be in it, for as long as the room is there. A task of its own: the room's
    /// other task hears of its document meanwhile.
    async fn ask_again(self, room: Arc<Room>) {
        let mut due = tokio::time::interval(self.inner.rooms.recheck);
        due.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        // The first tick is at once; who is in the room was asked as they came in.
        due.tick().await;
        let stopping = self.inner.rooms.stopping.clone();
        loop {
            tokio::select! {
                () = room.gone.cancelled() => return,
                () = stopping.cancelled() => return,
                _ = due.tick() => {}
            }
            // Given up when the server stops: nothing is asked of a store that is
            // about to close.
            tokio::select! {
                () = stopping.cancelled() => return,
                () = self.recheck(&room) => {}
            }
        }
    }

    /// Puts out of the rooms of the organization of `app` (a handle of one
    /// organization) every connection whose session was not made through `provider_id`,
    /// as the organization begins to take only sessions made through that provider: the
    /// wall's own rule ([`session_opens`]), so nothing is asked of the store. It waits
    /// for each room's turn, nothing more: a room it empties is closed by a task of its
    /// own. For after that is stored, and before the request that stored it answers:
    /// what such a connection sends after that is taken by no room. A connection whose
    /// admission was answered before is admitted again as it comes in
    /// ([`Self::enter`]), so none is let in past it.
    pub(crate) async fn put_out_shut_out(&self, app: &App, provider_id: &str) {
        let rooms = &self.inner.rooms;
        // Before the rooms are read: a connection that comes into one later sees it.
        rooms.walls_raised.fetch_add(1, Ordering::SeqCst);
        let tenant = app.store().tenant();
        let of_organization: Vec<Arc<Room>> = rooms
            .by_key
            .lock()
            .expect("the rooms are never held across a panic")
            .values()
            .filter(|room| room.app.store().tenant() == tenant)
            .cloned()
            .collect();
        // As for a card that is not there: the organization is told to nobody.
        let reason = reason_of(&AppError::not_found("card"));
        for room in of_organization {
            let mut turn = room.turn.lock().await;
            // From the last: a member put out takes the place of none not yet looked at.
            for index in (0..turn.members.len()).rev() {
                let made_through = turn.members[index].connection.made_through.as_deref();
                if !session_opens(Some(provider_id), made_through) {
                    turn.put_out(index, &reason);
                }
            }
            if turn.members.is_empty() {
                drop(turn);
                // Closing writes the body to the card: not for the request to wait on.
                let collab = self.clone();
                tokio::spawn(async move {
                    let mut turn = room.turn.lock().await;
                    collab.shut_if_empty(&room, &mut turn).await;
                });
            }
        }
    }

    /// Asks again whether each member of a room may be in it, and how: who may no
    /// longer read the card's board is put out, and who is to be in the room otherwise
    /// than they are is told so (`DOC_MODE`). Nobody stays unasked: a member the store
    /// could not be asked about, or was not asked about within the time of one
    /// asking, is asked about once more two seconds later, and put out if the store
    /// does not answer then either. The store is asked while the room is not held; what
    /// a member sent meanwhile was taken as they were.
    async fn recheck(&self, room: &Arc<Room>) {
        let asked: Vec<Asked> = {
            let turn = room.turn.lock().await;
            turn.members
                .iter()
                .map(|member| Asked {
                    connection: member.connection.id,
                    doc: member.doc,
                    who: Who {
                        user_id: member.connection.user_id.clone(),
                        made_through: member.connection.made_through.clone(),
                        workspace_id: member.workspace_id.clone(),
                    },
                })
                .collect()
        };
        // Each user once, however many connections they have in the room.
        let mut users: Vec<Who> = asked.iter().map(|asked| asked.who.clone()).collect();
        users.sort_unstable();
        users.dedup();
        let mut answers = self.answers(room, users.clone()).await;
        // Whom the store did not answer of is asked once more, a little later: a store
        // that fails for a moment empties no room. Who is still not answered of then
        // is put out.
        users.retain(|who| !matches!(answers.get(who), Some(Ok(_))));
        if !users.is_empty() {
            tokio::time::sleep(ASKED_ONCE_MORE_AFTER).await;
            answers.extend(self.answers(room, users).await);
        }
        let mut turn = room.turn.lock().await;
        if turn.closed || self.rooms_ended() {
            return;
        }
        for Asked {
            connection,
            doc,
            who,
        } in asked
        {
            let is_asked =
                |member: &Member| member.connection.id == connection && member.doc == doc;
            let Some(index) = turn.members.iter().position(is_asked) else {
                continue;
            };
            match answers.get(&who) {
                Some(Ok(Some(mode))) if *mode == turn.members[index].mode => {}
                Some(Ok(Some(mode))) => {
                    let mode = *mode;
                    turn.members[index].mode = mode;
                    turn.send(is_asked, |_| [Queued::text(doc, messages::mode(doc, mode))]);
                }
                Some(Ok(None)) => turn.put_out(index, &reason_of(&AppError::not_found("card"))),
                Some(Err(reason)) => turn.put_out(index, reason),
                None => turn.put_out(index, &reason_of(&refusals::unavailable())),
            }
        }
        self.shut_if_empty(room, &mut turn).await;
    }

    /// What the store answers of each of `users`: how they are in the room, `None` for who is not
    /// let in, or the reason the store could not be asked, which is logged. Some at
    /// a time, and all within the time between two askings: who is not answered by
    /// then has no answer.
    async fn answers(&self, room: &Room, users: Vec<Who>) -> HashMap<Who, Answer> {
        use futures_util::StreamExt as _;

        let (app, card_id) = (room.app.clone(), room.card_id.clone());
        let asking = futures_util::stream::iter(users)
            .map(move |who| {
                let (app, card_id) = (app.clone(), card_id.clone());
                async move {
                    let admitted =
                        admission::admit(&app, who.asker(), &who.workspace_id, &card_id).await;
                    let answer = match admitted {
                        Ok(admitted) => Ok(admitted.map(|admitted| admitted.mode)),
                        Err(error) => {
                            error.log();
                            Err(reason_of(&error))
                        }
                    };
                    (who, answer)
                }
            })
            .buffer_unordered(ASKED_AT_ONCE);
        tokio::pin!(asking);
        let late = tokio::time::sleep(self.inner.rooms.recheck);
        tokio::pin!(late);
        let mut answers = HashMap::new();
        loop {
            tokio::select! {
                answer = asking.next() => match answer {
                    Some((who, answer)) => {
                        answers.insert(who, answer);
                    }
                    None => break,
                },
                () = &mut late => {
                    tracing::warn!(card_id = room.card_id, "who is in a room was not asked about in time");
                    break;
                }
            }
        }
        answers
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
        let limits = &self.inner.rooms;
        // Before anything is asked of the store: a connection that is in as many
        // rooms as it may be has nothing asked for it.
        if connection.is_full_without(limits.per_connection, card_id) {
            return Err(refusals::too_many_joined(limits.per_connection));
        }
        let mut raised = limits.walls_raised.load(Ordering::SeqCst);
        let mut admitted =
            admission::let_in(home, connection.asker(), workspace_id, card_id).await?;
        let mut admissions = 1;
        loop {
            let room = self.room(&admitted.app, card_id);
            let mut turn = room.turn.lock().await;
            if turn.closed {
                continue;
            }
            // An organization began to take only its provider's sessions since the
            // connection was admitted, and its rooms may have put out whom it shuts out
            // already: the connection is admitted again.
            let now = limits.walls_raised.load(Ordering::SeqCst);
            if now != raised {
                self.shut_if_empty(&room, &mut turn).await;
                drop(turn);
                if admissions == ADMITTED_AT_MOST {
                    return Err(refusals::unavailable());
                }
                raised = now;
                admitted =
                    admission::let_in(home, connection.asker(), workspace_id, card_id).await?;
                admissions += 1;
                continue;
            }
            if let Some(refused) = self.refusal(&turn, connection) {
                self.shut_if_empty(&room, &mut turn).await;
                return Err(refused);
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
                connection.queue.forget_frames(before.doc);
            }
            if !turn.members.is_empty() && turn.epoch != epoch {
                // Made again since the members were sent it, and not yet told.
                turn.send_anew(&loaded);
            }
            turn.epoch = epoch;
            turn.members.push(Member {
                connection: Arc::clone(connection),
                doc,
                workspace_id: workspace_id.to_owned(),
                mode: admitted.mode,
            });
            return Ok(());
        }
    }

    /// Why a connection that was admitted is not let into a room as it is, if it is not:
    /// the server stops, or one more would be more than the limits take.
    fn refusal(&self, turn: &Turn, connection: &Connection) -> Option<AppError> {
        let limits = &self.inner.rooms;
        let is_member = turn
            .members
            .iter()
            .any(|member| member.connection.id == connection.id);
        if self.rooms_ended() {
            Some(refusals::unavailable())
        } else if is_member {
            // In the room anew: one more neither of the connection's rooms nor of the
            // room's connections.
            None
        } else if connection.joined().rooms.len() >= limits.per_connection {
            Some(refusals::too_many_joined(limits.per_connection))
        } else if turn.of_user(&connection.user_id) >= limits.per_user {
            Some(refusals::too_many_of_user(limits.per_user))
        } else if turn.members.len() >= limits.per_room {
            Some(refusals::full(limits.per_room))
        } else {
            None
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

    /// Puts a connection out of a room it knows as `doc`, and tells it why.
    pub(super) async fn put_out(
        &self,
        room: &Arc<Room>,
        connection: &Connection,
        doc: u32,
        reason: &str,
    ) {
        let mut turn = room.turn.lock().await;
        if let Some(index) = turn.index_of(connection, doc) {
            turn.put_out(index, reason);
        }
        self.shut_if_empty(room, &mut turn).await;
    }

    /// How many connections are in the room of a card of `app`'s organization, for
    /// the tests of who is put out of it.
    #[cfg(feature = "test-util")]
    #[doc(hidden)]
    pub async fn room_members(&self, app: &App, card_id: &str) -> usize {
        let key = place::key_of(app.store().tenant(), card_id);
        let room = {
            let rooms = self.inner.rooms.by_key.lock();
            rooms
                .expect("the rooms are never held across a panic")
                .get(&key)
                .map(Arc::clone)
        };
        match room {
            Some(room) => room.turn.lock().await.members.len(),
            None => 0,
        }
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
            Err(reason) => turn.answer(connection, doc, messages::nack(doc, epoch, &reason)),
            Ok(taken) => {
                // Told whatever waits for the sender: the update is logged, and is
                // sent on to the others also if the sender is put out for its queue.
                turn.answer(connection, doc, messages::ack(doc, epoch, taken.seq));
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
