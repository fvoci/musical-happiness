//! The binary frames of a body edited together, as the websocket carries them in both
//! directions (`contracts/websocket.md`):
//!
//! ```text
//! varuint doc      which of the connection's rooms, as `DOC_JOINED` named it
//! varuint kind     0: a sync message (the only kind there is)
//! varuint epoch    which making of the document the message is about
//! varuint type     0: sync step 1, 1: sync step 2, 2: update
//! varuint length   of what follows, which is all that follows
//! bytes            a state vector (step 1) or an update (step 2, update), version 1
//! ```
//!
//! From `type` on it is a sync message of y-protocols, byte for byte. A varuint is
//! lib0's: seven bits a byte, the lowest first, the high bit set on all but the last.
//!
//! Nothing here allocates by a length a frame claims: what a frame carries is a slice
//! of the frame.

use bytes::{BufMut as _, Bytes, BytesMut};

/// The kind of a frame that carries a sync message.
const KIND_SYNC: u64 = 0;

/// The most bytes a frame has before what its sync message carries: five varuints,
/// of which the document and the epoch may be long. What the websocket takes of a
/// frame is the largest update and this.
pub(super) const HEAD_ROOM: usize = 32;

/// What a sync message is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Sync {
    /// Step 1: the state vector of the sender, who asks for what it is missing.
    StateVector,
    /// Step 2: what the receiver was missing; from the server, the whole document.
    Missing,
    /// A change.
    Update,
}

impl Sync {
    const fn number(self) -> u64 {
        match self {
            Self::StateVector => 0,
            Self::Missing => 1,
            Self::Update => 2,
        }
    }

    const fn of(number: u64) -> Option<Self> {
        match number {
            0 => Some(Self::StateVector),
            1 => Some(Self::Missing),
            2 => Some(Self::Update),
            _ => None,
        }
    }
}

/// A frame a client sent, after its room: the sync message of a making of the document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Received {
    pub(super) epoch: i64,
    pub(super) sync: Sync,
    /// A slice of the frame.
    pub(super) payload: Bytes,
}

/// Reads a varuint from the front of `bytes`: its value and how many bytes it took.
/// `None` for one that does not end, or does not fit 64 bits.
fn varuint(bytes: &[u8]) -> Option<(u64, usize)> {
    let mut value: u64 = 0;
    for (index, byte) in bytes.iter().enumerate().take(10) {
        let bits = u64::from(byte & 0x7f);
        let shift = u32::try_from(index).ok()?.checked_mul(7)?;
        if shift == 63 && bits > 1 {
            return None;
        }
        value |= bits << shift;
        if byte & 0x80 == 0 {
            return Some((value, index + 1));
        }
    }
    None
}

/// Takes a varuint from the front of `frame`.
fn take_varuint(frame: &mut Bytes) -> Option<u64> {
    let (value, taken) = varuint(frame)?;
    let _ = frame.split_to(taken);
    Some(value)
}

/// The room a frame names, and the rest of the frame. `None` for a frame that names
/// none.
pub(super) fn room_of(mut frame: Bytes) -> Option<(u32, Bytes)> {
    let doc = u32::try_from(take_varuint(&mut frame)?).ok()?;
    Some((doc, frame))
}

/// What a frame carries after its room. `None` for what is no sync message of this
/// shape to its last byte: another kind, a type that is none, a length that is not
/// that of what follows.
pub(super) fn received(mut rest: Bytes) -> Option<Received> {
    if take_varuint(&mut rest)? != KIND_SYNC {
        return None;
    }
    let epoch = i64::try_from(take_varuint(&mut rest)?).ok()?;
    let sync = Sync::of(take_varuint(&mut rest)?)?;
    let length = usize::try_from(take_varuint(&mut rest)?).ok()?;
    (rest.len() == length).then_some(Received {
        epoch,
        sync,
        payload: rest,
    })
}

fn put_varuint(out: &mut BytesMut, mut value: u64) {
    while value >= 0x80 {
        // The low seven bits, with the mark of a byte that is not the last.
        out.put_u8(u8::try_from(value & 0x7f).unwrap_or(0x7f) | 0x80);
        value >>= 7;
    }
    out.put_u8(u8::try_from(value).unwrap_or(0x7f));
}

/// A sync message that carries `payload`: what every receiver of it is sent after the
/// head of its own frame ([`framed`]).
pub(super) fn message(sync: Sync, payload: &[u8]) -> Bytes {
    let mut out = BytesMut::with_capacity(payload.len() + 11);
    put_varuint(&mut out, sync.number());
    put_varuint(&mut out, u64::try_from(payload.len()).unwrap_or(u64::MAX));
    out.put_slice(payload);
    out.freeze()
}

/// The frame of a sync message for one connection: its room there, and the epoch.
pub(super) fn framed(doc: u32, epoch: i64, message: &Bytes) -> Bytes {
    let mut out = BytesMut::with_capacity(message.len() + HEAD_ROOM);
    put_varuint(&mut out, u64::from(doc));
    put_varuint(&mut out, KIND_SYNC);
    debug_assert!(
        epoch >= 0,
        "an epoch counts the makings of a document: {epoch}"
    );
    put_varuint(&mut out, u64::try_from(epoch).unwrap_or(0));
    out.put_slice(message);
    out.freeze()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_frame_is_read_back_as_it_was_written() {
        for (doc, epoch, sync, payload) in [
            (1, 1, Sync::Update, &b"abc"[..]),
            (0, 0, Sync::StateVector, &[][..]),
            (u32::MAX, i64::MAX, Sync::Missing, &[0x80; 300][..]),
            (128, 16_384, Sync::Update, &[7][..]),
        ] {
            let frame = framed(doc, epoch, &message(sync, payload));
            let (room, rest) = room_of(frame).expect("a room");
            assert_eq!(room, doc);
            let read = received(rest).expect("a sync message");
            assert_eq!(
                (read.epoch, read.sync, &read.payload[..]),
                (epoch, sync, payload)
            );
        }
    }

    #[test]
    fn the_bytes_of_a_frame_are_those_of_the_contract() {
        // doc 3, sync, epoch 300, an update of two bytes.
        let frame = framed(3, 300, &message(Sync::Update, &[0xaa, 0xbb]));
        assert_eq!(&frame[..], [3, 0, 0xac, 0x02, 2, 2, 0xaa, 0xbb]);
        // From the type on, the sync message of y-protocols.
        assert_eq!(&message(Sync::StateVector, &[0])[..], [0, 1, 0]);
    }

    #[test]
    fn what_is_no_frame_is_none() {
        let no_frames: [&[u8]; 9] = [
            &[],
            // A room and nothing else.
            &[1],
            // Another kind.
            &[1, 1, 1, 2, 0],
            // A type that is none.
            &[1, 0, 1, 3, 0],
            // A length past the frame, and one short of it.
            &[1, 0, 1, 2, 5, 1, 2],
            &[1, 0, 1, 2, 1, 1, 2],
            // A length nothing could hold.
            &[
                1, 0, 1, 2, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x01,
            ],
            // A varuint that does not end, and one past 64 bits.
            &[
                1, 0, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80,
            ],
            &[
                1, 0, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x02, 2, 0,
            ],
        ];
        for bytes in no_frames {
            let read = room_of(Bytes::copy_from_slice(bytes)).and_then(|(_, rest)| received(rest));
            assert_eq!(read, None, "{bytes:?}");
        }
        // A room past 32 bits names none.
        assert_eq!(
            room_of(Bytes::from_static(&[0xff, 0xff, 0xff, 0xff, 0x1f, 0])),
            None
        );
    }
}
