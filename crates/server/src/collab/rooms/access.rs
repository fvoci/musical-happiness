//! Immediate access changes and the periodic fallback for the rooms.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use tokio::time::Instant;

use super::super::messages::reason_of;
use super::super::queue::Queued;
use super::super::{Collab, admission, messages, refusals};
use super::{ASKED_AT_ONCE, ASKED_ONCE_MORE_AFTER, Answer, Asked, Member, Room, Who};
use crate::app::App;
use crate::error::AppError;

/// The facts changed in this part of the application's organization.
#[derive(Debug, Clone)]
pub(crate) enum AccessScope {
    /// One board, including all its cards.
    Board(String),
    /// Every board of a workspace.
    Workspace(String),
    /// Every workspace of the organization.
    Organization,
}

impl AccessScope {
    fn contains(&self, room: &Room) -> bool {
        match self {
            Self::Board(id) => room.board_id == *id,
            Self::Workspace(id) => room.workspace_id == *id,
            Self::Organization => true,
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

    async fn access_changed(&self, app: &App, scope: AccessScope) {
        let rooms = &self.inner.rooms;
        let _turn = rooms.access_turn.lock().await;
        // Admissions that began before this change must read the new facts.
        rooms.walls_raised.fetch_add(1, Ordering::SeqCst);
        let tenant = app.store().tenant();
        let affected: Vec<Arc<Room>> = rooms
            .by_key
            .lock()
            .expect("the rooms are never held across a panic")
            .values()
            .filter(|room| room.app.store().tenant() == tenant && scope.contains(room))
            .cloned()
            .collect();
        let deadline = Instant::now() + rooms.recheck;
        let mut answers: HashMap<(String, Who), Answer> = HashMap::new();
        for room in affected {
            let mut turn = room.turn.lock().await;
            if turn.closed {
                continue;
            }
            for index in (0..turn.members.len()).rev() {
                let member = &turn.members[index];
                let who = Who {
                    user_id: member.connection.user_id.clone(),
                    made_through: member.connection.made_through.clone(),
                    workspace_id: member.workspace_id.clone(),
                };
                let key = (room.board_id.clone(), who.clone());
                let answer = if let Some(answer) = answers.get(&key) {
                    answer.clone()
                } else {
                    let answer = self.access_answer(&room, &who, deadline).await;
                    answers.insert(key, answer.clone());
                    answer
                };
                let doc = turn.members[index].doc;
                let connection = turn.members[index].connection.id;
                match answer {
                    Ok(Some(mode)) if mode == turn.members[index].mode => {}
                    Ok(Some(mode)) => {
                        turn.members[index].mode = mode;
                        turn.send(
                            |member| member.connection.id == connection && member.doc == doc,
                            |_| [Queued::text(doc, messages::mode(doc, mode))],
                        );
                    }
                    Ok(None) => turn.put_out(index, &reason_of(&AppError::not_found("card"))),
                    Err(reason) => turn.put_out(index, &reason),
                }
            }
            self.shut_if_empty(&room, &mut turn).await;
        }
        // A periodic read made during this pass must not undo its result later.
        rooms.walls_raised.fetch_add(1, Ordering::SeqCst);
    }

    /// One deadline for the entire pass, so a stalled store cannot keep rooms locked.
    async fn access_answer(&self, room: &Room, who: &Who, deadline: Instant) -> Answer {
        let checking = async {
            #[cfg(feature = "test-util")]
            self.stalled_access().await;
            admission::board_mode(&room.app, who.asker(), &who.workspace_id, &room.board_id).await
        };
        match tokio::time::timeout_at(deadline, checking).await {
            Ok(answer) => answer.map_err(|error| {
                error.log();
                reason_of(&error)
            }),
            Err(_) => Err(reason_of(&refusals::unavailable())),
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
        let revision = self.inner.rooms.walls_raised.load(Ordering::SeqCst);
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
        #[cfg(feature = "test-util")]
        self.paused_after_recheck().await;
        let mut turn = room.turn.lock().await;
        if turn.closed
            || self.rooms_ended()
            || revision != self.inner.rooms.walls_raised.load(Ordering::SeqCst)
        {
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
