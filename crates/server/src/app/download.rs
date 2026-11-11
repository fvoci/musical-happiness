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
        // Without a runtime the archive's drop removes its file as it can.
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(release(archive, slot));
        }
    }
}

/// Removes the archive's file, and then gives the place back.
async fn release(archive: ExportedArchive, slot: Option<Slot>) {
    archive.remove().await;
    drop(slot);
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

#[cfg(test)]
mod tests {
    use futures_util::StreamExt as _;

    use super::*;
    use crate::app::doomed::Doomed;
    use crate::app::slots::Slots;

    /// An archive of four pieces in a file of its own; returns it and where it is.
    async fn archive(name: &str) -> (ExportedArchive, std::path::PathBuf) {
        let bytes = 4 * PIECE_BYTES;
        let path = std::env::temp_dir().join(format!("download-{}-{name}", std::process::id()));
        std::fs::write(&path, vec![7; bytes]).expect("the archive is written");
        let file = tokio::fs::File::open(&path).await.expect("opened");
        let size = u64::try_from(bytes).expect("a size");
        (
            ExportedArchive::new(file, size, Doomed::file(path.clone())),
            path,
        )
    }

    /// Waits, on the paused clock, until `done`.
    async fn until(what: &str, mut done: impl FnMut() -> bool) {
        for _ in 0..1000 {
            if done() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("{what}");
    }

    /// A client that takes a piece and then pauses, for less than the stall time each
    /// time but longer than it in all, gets the whole archive; the place is held until
    /// the end, then the file goes and the place is given back. On the paused clock, so
    /// that how busy the machine is does not matter.
    #[tokio::test(start_paused = true)]
    async fn a_download_taken_slowly_is_not_given_up() {
        let slots = Slots::new(1);
        let (archive, path) = archive("slow").await;
        let size = archive.size;
        let slot = slots.take("user").expect("a place");
        let breaker = Breaker::default();
        let stall = Duration::from_secs(1);
        let pieces = download(archive, slot, stall, Some(breaker.clone()));
        tokio::pin!(pieces);

        let started = Instant::now();
        let mut received = 0;
        while let Some(piece) = pieces.next().await {
            received += piece.expect("not given up").len();
            assert!(
                slots.take("other").is_err(),
                "the place is held while it is sent"
            );
            tokio::time::sleep(stall * 6 / 10).await;
        }
        assert_eq!(
            u64::try_from(received).ok(),
            Some(size),
            "the whole archive"
        );
        assert!(
            started.elapsed() > stall * 2,
            "it took longer than the stall time"
        );
        assert!(!breaker.is_cut(), "the connection stays");
        until("the place is given back", || slots.take("user").is_ok()).await;
        assert!(!path.exists(), "the file went before the place");
    }

    /// The file goes before the place: polled step by step, the release never has the
    /// place free while the file is there.
    #[tokio::test]
    async fn the_file_goes_before_the_place() {
        let slots = Slots::new(1);
        let (archive, path) = archive("released").await;
        let slot = slots.take("user").expect("a place");
        let releasing = release(archive, Some(slot));
        tokio::pin!(releasing);
        loop {
            let done = futures_util::poll!(releasing.as_mut()).is_ready();
            let free = slots.take("user").is_ok();
            assert!(
                !free || !path.exists(),
                "the place is free while the file is there"
            );
            if done {
                assert!(free && !path.exists(), "both are given back");
                return;
            }
            tokio::task::yield_now().await;
        }
    }

    /// A download that takes nothing for the stall time is given up: the file goes, then
    /// the place, the connection is cut, and the stream, polled again, ends with an error.
    /// On the paused clock the moment is exact: the watch looks every quarter of the stall
    /// time, sees the piece at its first look, and gives up at its look a stall time
    /// after that.
    #[tokio::test(start_paused = true)]
    async fn a_download_that_takes_nothing_is_given_up() {
        let slots = Slots::new(1);
        let (archive, path) = archive("stalled").await;
        let slot = slots.take("user").expect("a place");
        let breaker = Breaker::default();
        let stall = Duration::from_secs(1);
        let started = Instant::now();
        let pieces = download(archive, slot, stall, Some(breaker.clone()));
        tokio::pin!(pieces);
        pieces.next().await.expect("a piece").expect("read");

        tokio::time::sleep_until(started + stall * 5 / 4 - Duration::from_millis(10)).await;
        assert!(slots.take("other").is_err(), "held until the watch's look");
        assert!(!breaker.is_cut());
        tokio::time::sleep_until(started + stall * 5 / 4 + Duration::from_millis(10)).await;
        assert!(breaker.is_cut(), "cut at the watch's look");
        until("the place is given back", || slots.take("user").is_ok()).await;
        assert!(!path.exists(), "the file went before the place");
        assert!(breaker.is_cut(), "the connection is cut");
        let error = pieces.next().await.expect("an end").expect_err("given up");
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert!(pieces.next().await.is_none(), "and nothing after");
    }
}
