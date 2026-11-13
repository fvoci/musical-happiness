//! Cached room grants and atomic registration/dispatch under the store revision.

use std::collections::HashMap;
use std::sync::Arc;

use super::{Answer, Asked, Connection, Member, Room, Turn, Who};
use crate::collab::frame::{self, Sync};
use crate::collab::messages::{self, reason_of};
use crate::collab::queue::Queued;
use crate::collab::{Collab, Loaded, admission, refusals};
use crate::error::{AppError, Reason};

impl Collab {
    pub(super) async fn refresh_visibility(
        &self,
        room: &Room,
        turn: &mut Turn,
    ) -> Option<fvoci_store::VisibilityVersion> {
        loop {
            let revision = room.app.store().visibility_version().await;
            if !revision.cacheable() {
                while !turn.members.is_empty() {
                    turn.put_out(0, Reason::DocumentUnavailable.as_str());
                }
                return None;
            }
            if turn.visibility == Some(revision) {
                return Some(revision);
            }
            let asked = turn.asked();
            let mut users: Vec<_> = asked.iter().map(|asked| asked.who.clone()).collect();
            users.sort_unstable();
            users.dedup();
            let answers = self.answers(room, users).await;
            if room.app.store().if_visibility_current(revision, || {
                turn.apply_answers(asked, &answers);
                turn.visibility = Some(revision);
            }) {
                return Some(revision);
            }
        }
    }

    pub(super) async fn send_visible(
        &self,
        room: &Room,
        turn: &mut Turn,
        mut send: impl FnMut(&mut Turn),
    ) {
        loop {
            let Some(revision) = self.refresh_visibility(room, turn).await else {
                return;
            };
            #[cfg(feature = "test-util")]
            self.paused_before_room_dispatch().await;
            if room
                .app
                .store()
                .if_visibility_current(revision, || send(turn))
            {
                return;
            }
        }
    }
}

impl Turn {
    pub(super) fn asked(&self) -> Vec<Asked> {
        self.members
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
    }

    pub(super) fn apply_answers(&mut self, asked: Vec<Asked>, answers: &HashMap<Who, Answer>) {
        for Asked {
            connection,
            doc,
            who,
        } in asked
        {
            let is_asked =
                |member: &Member| member.connection.id == connection && member.doc == doc;
            let Some(index) = self.members.iter().position(is_asked) else {
                continue;
            };
            match answers.get(&who) {
                Some(Ok(Some(mode))) if *mode == self.members[index].mode => {}
                Some(Ok(Some(mode))) => {
                    self.members[index].mode = *mode;
                    self.send(is_asked, |_| {
                        [Queued::text(doc, messages::mode(doc, *mode))]
                    });
                }
                Some(Ok(None)) => self.put_out(index, &reason_of(&AppError::not_found("card"))),
                Some(Err(reason)) => self.put_out(index, reason),
                None => self.put_out(index, &reason_of(&refusals::unavailable())),
            }
        }
    }

    pub(super) fn register(
        &mut self,
        connection: &Arc<Connection>,
        doc: u32,
        workspace_id: &str,
        admitted: &admission::Admitted,
        loaded: &Loaded,
        room: &Arc<Room>,
    ) -> Result<(), AppError> {
        let epoch = loaded.epoch;
        let whole = frame::message(Sync::Missing, &loaded.state);
        let joined = messages::joined(workspace_id, &room.card_id, doc, epoch, admitted.mode);
        let queued = connection
            .queue
            .push([Queued::text(doc, joined), Queued::sync(doc, epoch, &whole)]);
        // Count the document before adding the room member: a connection that
        // already ended cannot strand a member no reader will remove.
        if queued.is_err() || !connection.join(doc, room) {
            connection.queue.forget(doc);
            return Err(refusals::too_slow());
        }
        if let Some(index) = self
            .members
            .iter()
            .position(|member| member.connection.id == connection.id)
        {
            let before = self.members.swap_remove(index);
            connection.forget(before.doc);
            connection.queue.forget_frames(before.doc);
        }
        if !self.members.is_empty() && self.epoch != epoch {
            self.send_anew(loaded);
        }
        self.epoch = epoch;
        self.members.push(Member {
            connection: Arc::clone(connection),
            doc,
            workspace_id: workspace_id.to_owned(),
            mode: admitted.mode,
        });
        Ok(())
    }
}

#[cfg(feature = "test-util")]
impl Collab {
    /// The number of per-user room permission lookups made by dispatch and idle checks.
    pub fn room_permission_checks(&self) -> u64 {
        self.inner
            .room_permission_checks
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Pauses the next room payload after its permission check and before enqueue.
    ///
    /// # Panics
    ///
    /// Panics if the test pause lock was poisoned.
    pub fn pause_next_room_dispatch(&self) -> (Arc<tokio::sync::Notify>, Arc<tokio::sync::Notify>) {
        let pause = (Arc::default(), Arc::default());
        *self
            .inner
            .room_dispatch_pause
            .lock()
            .expect("room dispatch pause lock") = Some(pause.clone());
        pause
    }

    async fn paused_before_room_dispatch(&self) {
        let pause = self
            .inner
            .room_dispatch_pause
            .lock()
            .expect("room dispatch pause lock")
            .take();
        if let Some((reached, resume)) = pause {
            reached.notify_one();
            resume.notified().await;
        }
    }
}
