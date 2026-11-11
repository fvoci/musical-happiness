//! Sending an export, and taking it back from a client that stopped reading.
//!
//! An export holds its place among the exports that run ([`Slot`]) and its temporary
//! file until the archive is sent or the client goes away: the limit on exports is a
//! limit on the disk they take at once. A client that stops reading would hold both as
//! long as its connection lasts; the body's stream is then not even polled again (the
//! socket takes nothing more), so no timer inside it would fire. A watch beside it sees
//! how far the sending got, and when nothing went out for the stall time it takes the
//! file and the place away (the file is closed and removed, then the place is free) and
//! breaks the connection ([`Breaker`]); a stream polled after that ends with an error.
//!
//! What went out is what hyper took from the stream, which it takes as the kernel's send
//! buffer and its own (a few MiB together) make room: a client must take about that much
//! within the stall time not to be given up.

use std::io;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Weak};
use std::time::Duration;

use bytes::{Bytes, BytesMut};
use futures_util::Stream;
use tokio::io::AsyncReadExt as _;
use tokio::sync::Mutex;
use tokio::time::Instant;

use super::Slot;
use super::export::ExportedArchive;
use crate::connection::Breaker;

/// How much of the archive one poll of the stream sends.
const PIECE_BYTES: usize = 64 * 1024;

/// What a download holds while it is sent. Dropped, it removes the archive's file and
/// then gives the place back, in that order: the place stands for the disk the file takes.
#[derive(Debug)]
struct Held {
    archive: Option<ExportedArchive>,
    slot: Option<Slot>,
}

impl Drop for Held {
    fn drop(&mut self) {
        let (Some(archive), slot) = (self.archive.take(), self.slot.take()) else {
            return;
        };
        let release = async move {
            archive.remove().await;
            drop(slot);
        };
        // Without a runtime the archive's drop removes its file as it can.
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(release);
        }
    }
}

/// A download, shared by its stream and its watch.
#[derive(Debug)]
struct Sending {
    /// `None` once the archive is sent, or taken away. Tokio's lock: it is held across
    /// the read of a piece, so that the watch never takes the file in the middle of one.
    held: Mutex<Option<Held>>,
    /// How many bytes went out so far.
    sent: AtomicU64,
}

/// The body of an export's download: the archive in pieces. A watch takes the archive and
/// `slot` away, and breaks the connection with `breaker`, when no piece went out for
/// `stall`.
pub(crate) fn download(
    archive: ExportedArchive,
    slot: Slot,
    stall: Duration,
    breaker: Option<Breaker>,
) -> impl Stream<Item = io::Result<Bytes>> + Send + 'static {
    let sending = Arc::new(Sending {
        held: Mutex::new(Some(Held {
            archive: Some(archive),
            slot: Some(slot),
        })),
        sent: AtomicU64::new(0),
    });
    tokio::spawn(watch(Arc::downgrade(&sending), stall, breaker));
    futures_util::stream::unfold(Some(sending), |sending| async move {
        let sending = sending?;
        let piece = next_piece(&sending).await;
        match piece {
            Ok(Some(bytes)) => Some((Ok(bytes), Some(sending))),
            Ok(None) => None,
            Err(error) => Some((Err(error), None)),
        }
    })
}

/// The next piece of the archive; `None` at its end, which gives back what the download
/// held.
async fn next_piece(sending: &Sending) -> io::Result<Option<Bytes>> {
    let mut held = sending.held.lock().await;
    let Some(archive) = held.as_mut().and_then(|held| held.archive.as_mut()) else {
        return Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "the client took nothing for too long; the export was given up",
        ));
    };
    let mut buffer = BytesMut::with_capacity(PIECE_BYTES);
    match archive.file.read_buf(&mut buffer).await {
        Ok(0) => {
            *held = None;
            Ok(None)
        }
        Ok(read) => {
            sending
                .sent
                .fetch_add(u64::try_from(read).unwrap_or(u64::MAX), Ordering::Relaxed);
            Ok(Some(buffer.freeze()))
        }
        Err(error) => {
            *held = None;
            Err(error)
        }
    }
}

/// Watches a download: when no byte went out for `stall`, takes what it holds away and
/// breaks the connection. Ends when the download is sent, taken away or dropped.
async fn watch(sending: Weak<Sending>, stall: Duration, breaker: Option<Breaker>) {
    let tick = (stall / 4).max(Duration::from_millis(50));
    let mut last_sent = 0;
    let mut last_progress = Instant::now();
    loop {
        tokio::time::sleep(tick).await;
        let Some(sending) = sending.upgrade() else {
            return;
        };
        let mut held = sending.held.lock().await;
        if held.is_none() {
            return;
        }
        // Read under the lock: a piece read while the watch waited for it counts.
        let sent = sending.sent.load(Ordering::Relaxed);
        if sent != last_sent {
            last_sent = sent;
            last_progress = Instant::now();
        }
        if last_progress.elapsed() >= stall {
            *held = None;
            if let Some(breaker) = &breaker {
                breaker.cut();
            }
            tracing::info!(
                sent,
                "an export's download took nothing for too long and was given up"
            );
            return;
        }
    }
}
