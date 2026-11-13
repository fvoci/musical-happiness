//! Immediate access changes and the periodic fallback for the rooms.

use std::collections::HashMap;
use std::future::Future;
use std::hash::Hash;
use std::sync::Arc;
use std::time::Duration;

use fvoci_store::Tenant;
use tokio::sync::MutexGuard;

use super::super::messages::reason_of;
use super::super::queue::Queued;
use super::super::{Collab, admission, messages, refusals};
use super::{
    ASKED_AT_ONCE, ASKED_ONCE_MORE_AFTER, Answer, Asked, Connection, Member, Room, Turn, Who,
};
use crate::app::App;
use crate::error::AppError;
use crate::permissions::DocMode;

/// The facts changed in this part of the application's organization, for everybody
/// or for one user.
#[derive(Debug, Clone)]
pub(crate) struct AccessScope {
    place: Place,
    /// The one user whose facts changed; everybody's when `None`.
    user_id: Option<String>,
}

#[derive(Debug, Clone)]
enum Place {
    Board(String),
    Workspace(String),
    Organization,
}

impl AccessScope {
    /// One board, including all its cards.
    pub(crate) fn board(board_id: &str) -> Self {
        Self::of(Place::Board(board_id.to_owned()))
    }

    /// Every board of a workspace.
    pub(crate) fn workspace(workspace_id: &str) -> Self {
        Self::of(Place::Workspace(workspace_id.to_owned()))
    }

    /// Every workspace of the organization.
    pub(crate) fn organization() -> Self {
        Self::of(Place::Organization)
    }

    /// The same place, where only `user_id`'s facts changed: the others in its rooms
    /// are not asked about.
    pub(crate) fn of_user(self, user_id: &str) -> Self {
        Self {
            user_id: Some(user_id.to_owned()),
            ..self
        }
    }

    fn of(place: Place) -> Self {
        Self {
            place,
            user_id: None,
        }
    }

    fn contains(&self, room: &Room) -> bool {
        match &self.place {
            Place::Board(id) => room.board_id == *id,
            Place::Workspace(id) => room.workspace_id == *id,
            Place::Organization => true,
        }
    }

    fn concerns(&self, user_id: &str) -> bool {
        self.user_id
            .as_deref()
            .is_none_or(|changed| changed == user_id)
    }
}

/// The members an immediate check holds, by room: what each sends waits until the
/// check answered of them. Dropped before it did (the check ended early), it puts
/// them out as members the store did not answer of, in a task of its own: what they
/// send is never left waiting, nor taken unasked.
#[derive(Debug)]
struct Holds {
    collab: Collab,
    /// The number of the change the check is for: of two checks of one member, the
    /// later change's answers.
    change: u64,
    rooms: Vec<(Arc<Room>, Vec<Asked>)>,
}

impl Holds {
    /// Lets go of every member held, each with what `answer_of` says of them (`None`:
    /// not answered of), and closes a room that nobody is left in.
    async fn let_go(mut self, answer_of: impl Fn(&Room, &Who) -> Option<Answer>) {
        // Taken from the holds once a room is done: a room not done stays held, for
        // the drop.
        while let Some((room, _)) = self.rooms.last() {
            let room = Arc::clone(room);
            let mut turn = room.turn.lock().await;
            let (_, members) = self.rooms.pop().expect("the room just looked at");
            for member in members {
                if let Some(index) = turn.let_go(&member, self.change) {
                    turn.settle(index, answer_of(&room, &member.who).as_ref());
                }
            }
            room.checked.notify_waiters();
            self.collab.shut_if_empty(&room, &mut turn).await;
        }
    }
}

impl Drop for Holds {
    fn drop(&mut self) {
        if self.rooms.is_empty() {
            return;
        }
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            tracing::error!("an immediate check ended outside the runtime; its members stay held");
            return;
        };
        tracing::error!("an immediate check ended before it answered; whom it held is put out");
        let held = Self {
            collab: self.collab.clone(),
            change: self.change,
            rooms: std::mem::take(&mut self.rooms),
        };
        runtime.spawn(held.let_go(|_, _| None));
    }
}

impl Asked {
    /// A member as it is asked about, by its connection and the room's number on it.
    fn of(member: &Member) -> Self {
        Self {
            connection: member.connection.id,
            doc: member.doc,
            who: Who {
                user_id: member.connection.user_id.clone(),
                made_through: member.connection.made_through.clone(),
                workspace_id: member.workspace_id.clone(),
            },
        }
    }

    fn is(&self, member: &Member) -> bool {
        member.connection.id == self.connection && member.doc == self.doc
    }
}

impl Turn {
    /// Has an answer of the store take effect for a member: told how they are in the
    /// room if that changed (`DOC_MODE`), or put out if they are not let in, if the
    /// store could not be asked (`None`: not answered in time).
    fn settle(&mut self, index: usize, answer: Option<&Answer>) {
        let member = &mut self.members[index];
        match answer {
            Some(Ok(Some(mode))) if *mode == member.mode => {}
            Some(Ok(Some(mode))) => {
                member.mode = *mode;
                let (doc, connection, mode) = (member.doc, member.connection.id, *mode);
                self.send(
                    |member| member.connection.id == connection && member.doc == doc,
                    |_| [Queued::text(doc, messages::mode(doc, mode))],
                );
            }
            Some(Ok(None)) => self.put_out(index, &reason_of(&AppError::not_found("card"))),
            Some(Err(reason)) => self.put_out(index, reason),
            None => self.put_out(index, &reason_of(&refusals::unavailable())),
        }
    }

    /// Holds the members `scope` concerns for the check of `change`, unless a later
    /// change's check holds them already; who is held, to be asked about.
    fn hold(&mut self, scope: &AccessScope, change: u64) -> Vec<Asked> {
        self.members
            .iter_mut()
            .filter(|member| scope.concerns(&member.connection.user_id))
            .map(|member| {
                if member.checking.is_none_or(|held| held < change) {
                    member.checking = Some(change);
                }
                Asked::of(member)
            })
            .collect()
    }

    /// Lets go of the member `asked` names if the check of `change` holds them, and
    /// says where they are; `None` if they left, or a later change's check holds
    /// them or answered of them already.
    fn let_go(&mut self, asked: &Asked, change: u64) -> Option<usize> {
        let index = self.members.iter().position(|member| asked.is(member))?;
        let member = &mut self.members[index];
        if member.checking == Some(change) {
            member.checking = None;
            Some(index)
        } else {
            None
        }
    }
}

impl Collab {
    /// Rechecks affected rooms before a permission-changing operation answers.
    /// The task survives a caller going away. The store is asked once per board,
    /// user and session kind, regardless of the number of cards or connections.
    pub(crate) async fn recheck_access(&self, app: &App, scope: AccessScope) {
        let (collab, scoped) = (self.clone(), app.clone());
        let task = app.spawn_tracked(async move { collab.access_changed(&scoped, scope).await });
        if let Err(error) = task.await {
            tracing::error!(%error, "the immediate room access check ended");
        }
    }

    /// Holds whom the change concerns in the rooms it reaches, asks the store about
    /// them while no room is held, and has each answer take effect, unless a later
    /// change concerned the same member meanwhile: that change's check answers of
    /// them instead. Each read has a deadline of its own: a slow one keeps its own
    /// member waiting, and puts them out at its deadline, and nobody else.
    async fn access_changed(&self, app: &App, scope: AccessScope) {
        let rooms = &self.inner.rooms;
        let tenant = app.store().tenant();
        // Admissions that began before this change must read the new facts.
        let change = rooms.walls.raise(tenant);
        let holds = self.hold(tenant, &scope, change).await;
        // Each board and user once, however many rooms and connections they have.
        let mut asked: HashMap<(String, Who), App> = HashMap::new();
        for (room, members) in &holds.rooms {
            for member in members {
                asked
                    .entry((room.board_id.clone(), member.who.clone()))
                    .or_insert_with(|| room.app.clone());
            }
        }
        let questions: Vec<(String, Who)> = asked.keys().cloned().collect();
        let answers = answered(questions, rooms.recheck, |(board_id, who)| {
            let app = asked[&(board_id.clone(), who.clone())].clone();
            async move {
                #[cfg(feature = "test-util")]
                self.stalled_access().await;
                admission::board_mode(&app, who.asker(), &who.workspace_id, &board_id).await
            }
        })
        .await;
        holds
            .let_go(|room, who| answers.get(&(room.board_id.clone(), who.clone())).cloned())
            .await;
    }

    /// Holds, for the check of `change`, the members `scope` concerns in the rooms of
    /// `tenant` it reaches.
    async fn hold(&self, tenant: Tenant, scope: &AccessScope, change: u64) -> Holds {
        let reached: Vec<Arc<Room>> = self
            .inner
            .rooms
            .by_key
            .lock()
            .expect("the rooms are never held across a panic")
            .values()
            .filter(|room| room.app.store().tenant() == tenant && scope.contains(room))
            .cloned()
            .collect();
        let mut holds = Holds {
            collab: self.clone(),
            change,
            rooms: Vec::new(),
        };
        for room in reached {
            let mut turn = room.turn.lock().await;
            if turn.closed {
                continue;
            }
            let members = turn.hold(scope, change);
            drop(turn);
            if !members.is_empty() {
                holds.rooms.push((room, members));
            }
        }
        holds
    }

    /// The room's turn for what the member `doc` of `connection` sent, once no
    /// immediate check holds them; `None` if they are no member of the room (any
    /// more), or the server stops.
    pub(super) async fn turn_to_send<'room>(
        &self,
        room: &'room Room,
        connection: &Connection,
        doc: u32,
    ) -> Option<MutexGuard<'room, Turn>> {
        loop {
            // Before the turn is looked at: an answer given after that is heard.
            let checked = room.checked.notified();
            let turn = room.turn.lock().await;
            let index = turn.index_of(connection, doc)?;
            if turn.members[index].checking.is_none() {
                return Some(turn);
            }
            drop(turn);
            tokio::select! {
                () = checked => {}
                () = self.inner.rooms.stopping.cancelled() => return None,
            }
        }
    }

    /// Leaves the next immediate permission read unanswered, to exercise its deadline.
    #[cfg(feature = "test-util")]
    #[doc(hidden)]
    #[must_use]
    pub fn stall_next_access_check(&self) -> Arc<tokio::sync::Notify> {
        let waiting = Arc::new(tokio::sync::Notify::new());
        *self
            .inner
            .rooms
            .access_stall
            .lock()
            .expect("the access stall is not poisoned") = Some(Arc::clone(&waiting));
        waiting
    }

    #[cfg(feature = "test-util")]
    async fn stalled_access(&self) {
        let waiting = self
            .inner
            .rooms
            .access_stall
            .lock()
            .expect("the access stall is not poisoned")
            .take();
        if let Some(waiting) = waiting {
            waiting.notify_one();
            std::future::pending::<()>().await;
        }
    }

    /// Asks again whether each member of a room may be in it, and how: who may no
    /// longer read the card's board is put out, and who is to be in the room otherwise
    /// than they are is told so (`DOC_MODE`). Nobody stays unasked: a member the store
    /// could not be asked about, or was not asked about within the time of one
    /// asking, is asked about once more two seconds later, and put out if the store
    /// does not answer then either. The store is asked while the room is not held; what
    /// a member sent meanwhile was taken as they were.
    pub(super) async fn recheck(&self, room: &Arc<Room>) {
        let (tenant, revision) = (room.app.store().tenant(), self.inner.rooms.walls.now());
        let asked: Vec<Asked> = room
            .turn
            .lock()
            .await
            .members
            .iter()
            .map(Asked::of)
            .collect();
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
        #[cfg(feature = "test-util")]
        self.paused_after_recheck().await;
        let mut turn = room.turn.lock().await;
        if turn.closed
            || self.rooms_ended()
            || self.inner.rooms.walls.raised_since(tenant, revision)
        {
            return;
        }
        for asked in asked {
            if let Some(index) = turn.members.iter().position(|member| asked.is(member)) {
                turn.settle(index, answers.get(&asked.who));
            }
        }
        self.shut_if_empty(room, &mut turn).await;
    }

    /// What the store answers of each of `users` in a room: how they are in it, `None`
    /// for who is not let in, or the reason the store could not be asked. Each within
    /// the time between two askings: who is not answered of by then is answered as
    /// one the store could not be asked about.
    async fn answers(&self, room: &Room, users: Vec<Who>) -> HashMap<Who, Answer> {
        let (app, card_id) = (&room.app, room.card_id.as_str());
        answered(users, self.inner.rooms.recheck, |who| async move {
            let admitted = admission::admit(app, who.asker(), &who.workspace_id, card_id).await;
            admitted.map(|admitted| admitted.map(|admitted| admitted.mode))
        })
        .await
    }

    /// Pauses the next periodic check after its reads, before applying its answers.
    #[cfg(feature = "test-util")]
    #[doc(hidden)]
    #[must_use]
    pub fn pause_next_recheck(&self) -> (Arc<tokio::sync::Notify>, Arc<tokio::sync::Notify>) {
        let pause = (
            Arc::new(tokio::sync::Notify::new()),
            Arc::new(tokio::sync::Notify::new()),
        );
        *self
            .inner
            .rooms
            .recheck_pause
            .lock()
            .expect("the recheck pause is not poisoned") = Some(pause.clone());
        pause
    }

    #[cfg(feature = "test-util")]
    async fn paused_after_recheck(&self) {
        let pause = self
            .inner
            .rooms
            .recheck_pause
            .lock()
            .expect("the recheck pause is not poisoned")
            .take();
        if let Some((waiting, go)) = pause {
            waiting.notify_one();
            go.notified().await;
        }
    }
}

/// Asks the store each of `questions` with `ask`, some at a time ([`ASKED_AT_ONCE`]),
/// each within `deadline` of its asking: a slow one costs the others no time. One not
/// answered in time is answered `document_unavailable`; a reason the store could not
/// be asked is logged, and answered as the reason.
async fn answered<K, A, F>(
    questions: impl IntoIterator<Item = K>,
    deadline: Duration,
    mut ask: A,
) -> HashMap<K, Answer>
where
    K: Clone + Eq + Hash,
    A: FnMut(K) -> F,
    F: Future<Output = Result<Option<DocMode>, AppError>>,
{
    use futures_util::StreamExt as _;

    futures_util::stream::iter(questions)
        .map(|key: K| {
            let asking = ask(key.clone());
            async move {
                let answer = tokio::time::timeout(deadline, asking)
                    .await
                    .unwrap_or_else(|_| {
                        tracing::warn!(
                            ?deadline,
                            "the store did not answer of a member of a room in time"
                        );
                        Err(refusals::unavailable())
                    });
                let answer = answer.map_err(|error| {
                    error.log();
                    reason_of(&error)
                });
                (key, answer)
            }
        })
        .buffer_unordered(ASKED_AT_ONCE)
        .collect()
        .await
}
