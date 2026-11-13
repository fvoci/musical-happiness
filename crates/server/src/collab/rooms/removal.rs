//! Detaching deleted rooms without waiting on their pending document operations.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use super::{Collab, Room};
use crate::app::App;
use crate::collab::place;

impl Collab {
    /// Called with the document deletion gate held, before allowing any new work.
    pub(in crate::collab) fn detach_deleted_rooms(
        &self,
        app: &App,
        card_ids: &[String],
    ) -> Vec<Arc<Room>> {
        self.inner
            .rooms
            .walls
            .raise(app.store().tenant(), super::AccessScope::cards(card_ids));
        let mut rooms = self
            .inner
            .rooms
            .by_key
            .lock()
            .expect("the rooms are not poisoned");
        let detached: Vec<Arc<Room>> = card_ids
            .iter()
            .filter_map(|id| rooms.remove(&place::key_of(app.store().tenant(), id)))
            .collect();
        for room in &detached {
            room.deleted.store(true, Ordering::SeqCst);
            room.gone.cancel();
        }
        detached
    }

    /// A normal last departure writes the body; a deletion already did so. Check
    /// after the document gate, since deletion can pass a close waiting for it.
    pub(in crate::collab) async fn close_room_document(
        &self,
        room: &Arc<Room>,
    ) -> Result<(), crate::error::AppError> {
        let closing = Arc::clone(room);
        self.on_slot(
            &room.app,
            &room.card_id,
            move |collab, app, card_id, mut slot| async move {
                let written = if closing.deleted.load(Ordering::SeqCst) {
                    Ok(())
                } else {
                    let written = collab.write_now(&app, &card_id, &mut slot).await;
                    collab.forget(&mut slot).await;
                    written
                };
                collab.retire(&mut slot);
                written
            },
        )
        .await
    }

    /// The documents were already forgotten: closing an old room must not close the
    /// new document of a card that somebody restored meanwhile.
    pub(in crate::collab) async fn finish_deleted_rooms(&self, rooms: Vec<Arc<Room>>) {
        for room in rooms {
            let mut turn = room.turn.lock().await;
            while !turn.members.is_empty() {
                turn.put_out(0, "not_found");
            }
            turn.closed = true;
        }
    }
}
