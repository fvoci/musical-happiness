//! The carets of a room: where each member who writes is in the document, told to
//! the room's others and to nobody else, and kept nowhere but here.
//!
//! A caret is its member's: the connection that sent it, in the room it sent it to.
//! Whose it is, the number the others know it under and which telling of it this is
//! are the server's to say; of what the client sent only the place is sent on
//! ([`caret::said`]). A member who reads has none. The others are told it is gone
//! when its member withdraws it, reads from now on, or is no member any more
//! ([`Turn::take_out`]); the document made again leaves none, as every member then
//! holds another document.
//!
//! A member an immediate check holds is told nothing of the carets, and once the
//! check keeps them, where every caret is now and which went meanwhile.

use std::sync::atomic::Ordering;
use std::time::Duration;

use bytes::Bytes;
use tokio::time::Instant;

use super::super::Collab;
use super::super::caret::{self, Said};
use super::super::queue::Queued;
use super::{Asked, Connection, Member, Room, Turn};
use crate::permissions::DocMode;

/// How long after a caret was last heard of it is still told to who comes in: its
/// client says it again every fifteen seconds, and who holds it lets it go after
/// thirty without a word.
const TOLD_FOR: Duration = Duration::from_secs(30);

/// How many carets that went a held member is told of at most, once let go: the
/// rest they let go by themselves ([`TOLD_FOR`]).
const MOST_MISSED: usize = 64;

/// The caret of a member.
#[derive(Debug)]
pub(super) struct Caret {
    /// What the others know it under: the server's number, of no other caret of any
    /// room.
    number: u64,
    /// Which telling of it the last was: a later one takes an earlier one's place.
    clock: u64,
    /// Where it is, if it is shown.
    shown: Option<Shown>,
}

#[derive(Debug)]
struct Shown {
    /// The awareness state the others are told ([`caret::state_of`]).
    state: String,
    heard: Instant,
}

/// The carets that went while a check held a member: each with the telling that said
/// so.
pub(super) type CaretsMissed = Vec<(u64, u64)>;

impl Turn {
    /// Takes a member out of the room. The others are told that its caret is gone.
    pub(super) fn take_out(&mut self, index: usize) -> Member {
        let mut member = self.members.swap_remove(index);
        if let Some((number, clock)) = member.caret.as_mut().and_then(Caret::withdraw) {
            self.tell_caret(None, number, clock, None);
        }
        member
    }

    /// Withdraws the caret of a member who stays: it reads from now on, or says it
    /// has none.
    pub(super) fn withdraw_caret(&mut self, index: usize) {
        let member = &mut self.members[index];
        let but = member.connection.id;
        if let Some((number, clock)) = member.caret.as_mut().and_then(Caret::withdraw) {
            self.tell_caret(Some(but), number, clock, None);
        }
    }

    /// Shows the caret of a member where they said it is, under `number` if it had
    /// none yet.
    fn show_caret(&mut self, index: usize, cursor: &caret::Cursor, number: impl FnOnce() -> u64) {
        let member = &mut self.members[index];
        let state = caret::state_of(&member.connection.user_id, cursor);
        let caret = member.caret.get_or_insert_with(|| Caret {
            number: number(),
            clock: 0,
            shown: None,
        });
        caret.clock += 1;
        caret.shown = Some(Shown {
            state: state.clone(),
            heard: Instant::now(),
        });
        let (but, number, clock) = (member.connection.id, caret.number, caret.clock);
        self.tell_caret(Some(but), number, clock, Some(&state));
    }

    /// Tells every member, but the connection `but` and whom a check holds, a caret:
    /// where it is, or that it is gone. A connection with no room for it is not told:
    /// it reads nothing, and a caret is said again.
    fn tell_caret(&mut self, but: Option<u64>, number: u64, clock: u64, state: Option<&str>) {
        let update = caret::told(number, clock, state);
        let epoch = self.epoch;
        for member in &mut self.members {
            if Some(member.connection.id) == but {
                continue;
            }
            if member.is_held() {
                let missed = member.carets_missed.get_or_insert_default();
                if state.is_none() && missed.len() < MOST_MISSED {
                    missed.push((number, clock));
                }
                continue;
            }
            let told = Queued::caret(member.doc, epoch, number, &update);
            member.connection.queue.push_caret(told);
        }
    }

    /// Forgets every caret: the document was made again, and a caret is a place in
    /// the one before. Nobody is told: each member lets go of that document.
    pub(super) fn forget_carets(&mut self) {
        for member in &mut self.members {
            if let Some(caret) = &mut member.caret {
                caret.shown = None;
            }
            member.carets_missed = None;
        }
    }

    /// Tells the member at `index` where the others' carets are, but those nothing
    /// was heard of for [`TOLD_FOR`].
    pub(super) fn send_carets(&self, index: usize) {
        let to = &self.members[index];
        for other in &self.members {
            let Some(caret) = &other.caret else { continue };
            let Some(shown) = &caret.shown else { continue };
            if other.connection.id == to.connection.id || shown.heard.elapsed() >= TOLD_FOR {
                continue;
            }
            let update = caret::told(caret.number, caret.clock, Some(&shown.state));
            let told = Queued::caret(to.doc, self.epoch, caret.number, &update);
            to.connection.queue.push_caret(told);
        }
    }

    /// Tells the member `asked` names, whom a check let go of and kept, what they
    /// were not told of the carets meanwhile: where each is now, and which went.
    pub(super) fn send_carets_missed(&mut self, asked: &Asked) {
        let Some(index) = self.members.iter().position(|member| asked.is(member)) else {
            return;
        };
        let Some(missed) = self.members[index].carets_missed.take() else {
            return;
        };
        self.send_carets(index);
        let to = &self.members[index];
        for (number, clock) in missed {
            let shown_again = self.members.iter().any(|member| {
                member
                    .caret
                    .as_ref()
                    .is_some_and(|caret| caret.number == number && caret.shown.is_some())
            });
            if !shown_again {
                let update = caret::told(number, clock, None);
                let told = Queued::caret(to.doc, self.epoch, number, &update);
                to.connection.queue.push_caret(told);
            }
        }
    }
}

impl Caret {
    /// Shows it no more; the number and the telling that says so, if it was shown.
    fn withdraw(&mut self) -> Option<(u64, u64)> {
        self.shown.take()?;
        self.clock += 1;
        Some((self.number, self.clock))
    }
}

impl Collab {
    /// Takes what a member said of its caret, in an awareness update, and tells the
    /// room's others. Nothing is told for a member who reads, for another making of
    /// the document than the room holds, or for what names no caret
    /// ([`caret::said`]); nobody is told so, and nothing of it is kept or logged. Like
    /// an update, it waits while an immediate check holds the member.
    pub(in crate::collab) async fn caret(
        &self,
        room: &Room,
        connection: &Connection,
        doc: u32,
        epoch: i64,
        update: Bytes,
    ) {
        let Some(said) = caret::said(&update) else {
            tracing::debug!(doc, "websocket: a caret frame that names no caret");
            return;
        };
        let Some(mut turn) = self.turn_to_send(room, connection, doc).await else {
            return;
        };
        let Some(index) = turn.index_of(connection, doc) else {
            return;
        };
        let member = &turn.members[index];
        // Held still only as the server stops, which takes nothing more.
        if epoch != turn.epoch || member.mode == DocMode::Read || member.is_held() {
            return;
        }
        match said {
            Said::Shown(cursor) => {
                let numbers = &self.inner.rooms.next_caret;
                turn.show_caret(index, &cursor, || {
                    numbers.fetch_add(1, Ordering::Relaxed) + 1
                });
            }
            Said::Withdrawn => turn.withdraw_caret(index),
        }
    }
}
