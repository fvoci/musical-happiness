//! What a connection that comes into a room is sent of the room's document: the
//! whole of it, or, to a connection that holds the document of this making already
//! (it was away for a while), what it lacks of it and the server's state vector, for
//! the connection to send what the server lacks.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use bytes::Bytes;

use super::super::frame::{self, Sync};
use super::super::{Collab, Loaded};
use super::Room;

/// The document a connection says it holds as it asks to come in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::collab) struct Held {
    /// Which making of the document.
    epoch: i64,
    /// Its state vector, version 1.
    state_vector: Vec<u8>,
}

impl Held {
    /// What a `JOIN_DOC` says the connection holds: `epoch`, and `stateVector` in
    /// base64. `None` for a join that names no state vector, or one that is no base64:
    /// the connection is sent the whole document, as one that holds none.
    pub(in crate::collab) fn of(epoch: i64, state_vector: &str) -> Option<Self> {
        if state_vector.is_empty() {
            return None;
        }
        let state_vector = STANDARD.decode(state_vector).ok()?;
        Some(Self {
            epoch,
            state_vector,
        })
    }
}

impl Collab {
    /// The sync messages a connection that comes into `room` is sent after
    /// `DOC_JOINED`, of the document as `loaded`: the whole of it (a step 2); or, for a
    /// connection that holds the document of this making, the server's state vector
    /// (a step 1) and then what the connection lacks (a step 2). Whatever keeps the
    /// difference from being read (a state vector that is none, a document that cannot
    /// be asked now) has the whole sent: the connection merges it all the same.
    pub(super) async fn first_of(
        &self,
        room: &Room,
        loaded: &Loaded,
        held: Option<&Held>,
    ) -> Vec<Bytes> {
        let whole = || vec![frame::message(Sync::Missing, &loaded.state)];
        let Some(held) = held.filter(|held| held.epoch == loaded.epoch) else {
            return whole();
        };
        let (app, card_id) = (&room.app, room.card_id.as_str());
        let gone = || Some(room.gone.clone());
        let lacked = self
            .diff_while(app, card_id, loaded.epoch, &held.state_vector, gone())
            .await;
        let ours = self.state_vector_while(app, card_id, gone()).await;
        match (ours, lacked) {
            (Ok(ours), Ok(lacked)) => vec![
                frame::message(Sync::StateVector, &ours),
                frame::message(Sync::Missing, &lacked),
            ],
            _ => whole(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_join_names_what_it_holds_or_nothing() {
        assert_eq!(Held::of(3, ""), None, "no state vector: no document held");
        assert_eq!(Held::of(3, "not base64!"), None);
        let held = Held::of(3, "AQID").expect("base64");
        assert_eq!((held.epoch, &held.state_vector[..]), (3, &[1, 2, 3][..]));
    }
}
