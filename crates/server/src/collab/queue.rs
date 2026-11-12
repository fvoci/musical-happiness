//! What waits to be sent to one connection about the rooms it is in: held to a number
//! of messages and of bytes, so that a connection that does not read holds no room
//! and little memory.

use std::collections::VecDeque;
use std::sync::Mutex;

use bytes::Bytes;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

use super::frame;

/// A message of a room for one connection.
#[derive(Debug, Clone)]
pub(super) struct Queued {
    /// The room, as the connection knows it; 0 for a message of no room.
    pub(super) doc: u32,
    pub(super) what: What,
}

/// What a message is.
#[derive(Debug, Clone)]
pub(super) enum What {
    /// A JSON message.
    Text(String),
    /// A sync message of the document of `epoch`, shared by its receivers
    /// ([`frame::message`]).
    Sync { epoch: i64, message: Bytes },
}

impl Queued {
    /// The bytes it counts as while it waits.
    fn bytes(&self) -> usize {
        match &self.what {
            What::Text(text) => text.len(),
            What::Sync { message, .. } => message.len(),
        }
    }

    pub(super) fn text(doc: u32, text: String) -> Self {
        Self {
            doc,
            what: What::Text(text),
        }
    }

    pub(super) fn sync(doc: u32, epoch: i64, message: &Bytes) -> Self {
        Self {
            doc,
            what: What::Sync {
                epoch,
                message: message.clone(),
            },
        }
    }
}

/// A message as the websocket sends it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outgoing {
    /// A text frame: a JSON message.
    Text(String),
    /// A binary frame.
    Binary(Bytes),
}

/// Why the queue did not take what was pushed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum NotQueued {
    /// It holds as much as it may.
    Full,
    /// The connection ended.
    Closed,
}

/// How much may wait for one connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Capacity {
    /// How many messages.
    pub(super) messages: usize,
    /// How many bytes, of the messages together. A message past it alone is taken
    /// into an empty queue: the whole of a document is sent whatever its size.
    pub(super) bytes: usize,
}

/// The messages that wait for one connection.
#[derive(Debug)]
pub(super) struct Queue {
    waiting: Mutex<Waiting>,
    /// Woken when there is something to send, or nothing more will come.
    wake: Notify,
    capacity: Capacity,
    /// Cancelled when notices alone are past what may wait: the connection reads
    /// nothing, and is closed.
    overrun: CancellationToken,
}

#[derive(Debug, Default)]
struct Waiting {
    messages: VecDeque<Queued>,
    /// The bytes of the messages together.
    bytes: usize,
    closed: bool,
}

impl Waiting {
    /// Drops the messages `drops` picks.
    fn drop_where(&mut self, drops: impl Fn(&Queued) -> bool) {
        self.messages.retain(|queued| !drops(queued));
        self.bytes = self.messages.iter().map(Queued::bytes).sum();
    }

    /// Queues `messages`, all or none.
    fn push(
        &mut self,
        capacity: Capacity,
        messages: impl IntoIterator<Item = Queued, IntoIter: ExactSizeIterator + Clone>,
    ) -> Result<(), NotQueued> {
        if self.closed {
            return Err(NotQueued::Closed);
        }
        let messages = messages.into_iter();
        let bytes: usize = messages.clone().map(|queued| queued.bytes()).sum();
        let over_bytes = !self.messages.is_empty() && self.bytes + bytes > capacity.bytes;
        if self.messages.len() + messages.len() > capacity.messages || over_bytes {
            return Err(NotQueued::Full);
        }
        self.bytes += bytes;
        self.messages.extend(messages);
        Ok(())
    }
}

impl Queue {
    pub(super) fn new(capacity: Capacity) -> Self {
        Self {
            waiting: Mutex::new(Waiting::default()),
            wake: Notify::new(),
            capacity,
            overrun: CancellationToken::new(),
        }
    }

    fn waiting(&self) -> std::sync::MutexGuard<'_, Waiting> {
        self.waiting
            .lock()
            .expect("the queue is never held across a panic")
    }

    /// Queues `messages`, all or none.
    ///
    /// # Errors
    ///
    /// Returns why they were not queued: they do not fit under what may wait, or the
    /// connection ended.
    pub(super) fn push(
        &self,
        messages: impl IntoIterator<Item = Queued, IntoIter: ExactSizeIterator + Clone>,
    ) -> Result<(), NotQueued> {
        self.waiting().push(self.capacity, messages)?;
        self.wake.notify_one();
        Ok(())
    }

    /// Queues `messages` in place of the sync messages that wait for the room: what a
    /// connection that is sent the document anew no longer needs of the one before.
    ///
    /// # Errors
    ///
    /// Returns why they were not queued, if they do not fit even so; the sync messages
    /// that waited are dropped all the same, and the caller puts the connection out of
    /// the room.
    pub(super) fn push_anew(
        &self,
        doc: u32,
        messages: impl IntoIterator<Item = Queued, IntoIter: ExactSizeIterator + Clone>,
    ) -> Result<(), NotQueued> {
        {
            let mut waiting = self.waiting();
            waiting
                .drop_where(|queued| queued.doc == doc && matches!(queued.what, What::Sync { .. }));
            waiting.push(self.capacity, messages)?;
        }
        self.wake.notify_one();
        Ok(())
    }

    /// Drops what waits for a room the connection is no longer in.
    pub(super) fn forget(&self, doc: u32) {
        self.waiting().drop_where(|queued| queued.doc == doc);
    }

    /// Queues a notice whatever waits: the answer to a connection that is refused or
    /// put out because its queue is full. With `doc`, what waits for that room is
    /// dropped first. Notices alone past twice what may wait end the connection.
    pub(super) fn notice(&self, doc: Option<u32>, text: String) {
        {
            let mut waiting = self.waiting();
            if waiting.closed {
                return;
            }
            if let Some(doc) = doc {
                waiting.drop_where(|queued| queued.doc == doc);
            }
            waiting.bytes += text.len();
            waiting.messages.push_back(Queued::text(0, text));
            if waiting.messages.len() > self.capacity.messages.saturating_mul(2) {
                self.overrun.cancel();
            }
        }
        self.wake.notify_one();
    }

    /// Nothing more is queued, and what waits is dropped: the connection ended.
    pub(super) fn close(&self) {
        {
            let mut waiting = self.waiting();
            waiting.closed = true;
            waiting.messages.clear();
            waiting.bytes = 0;
        }
        self.wake.notify_one();
    }

    /// Cancelled when the connection is to be closed for reading nothing.
    pub(super) fn overrun(&self) -> &CancellationToken {
        &self.overrun
    }

    /// The next message to send; `None` once the connection ended. Waiting for it may
    /// be given up at any time: nothing is lost.
    pub(super) async fn next(&self) -> Option<Outgoing> {
        loop {
            // Registered before the look, so that a push between the look and the
            // wait wakes it.
            let woken = self.wake.notified();
            {
                let mut waiting = self.waiting();
                if let Some(queued) = waiting.messages.pop_front() {
                    waiting.bytes = waiting.bytes.saturating_sub(queued.bytes());
                    return Some(match queued.what {
                        What::Text(text) => Outgoing::Text(text),
                        What::Sync { epoch, message } => {
                            Outgoing::Binary(frame::framed(queued.doc, epoch, &message))
                        }
                    });
                }
                if waiting.closed {
                    return None;
                }
            }
            woken.await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(doc: u32, text: &str) -> Queued {
        Queued::text(doc, text.to_owned())
    }

    fn sync(doc: u32) -> Queued {
        Queued::sync(doc, 1, &Bytes::from_static(&[2, 0]))
    }

    fn queue_of(messages: usize) -> Queue {
        Queue::new(Capacity {
            messages,
            bytes: 1024,
        })
    }

    /// What waits, in order, without ending the queue.
    async fn drained(queue: &Queue) -> Vec<Outgoing> {
        let waits = queue.waiting().messages.len();
        let mut sent = Vec::new();
        for _ in 0..waits {
            sent.extend(queue.next().await);
        }
        sent
    }

    #[tokio::test]
    async fn what_does_not_fit_is_not_queued_and_a_notice_takes_the_place_of_a_room() {
        let queue = queue_of(3);
        queue.push([text(1, "a"), sync(1)]).expect("room for two");
        queue.push([sync(2)]).expect("room for a third");
        assert_eq!(
            queue.push([sync(1)]),
            Err(NotQueued::Full),
            "no room for a fourth"
        );
        assert_eq!(
            queue.push_anew(2, [text(2, "anew"), sync(2)]),
            Err(NotQueued::Full),
            "two in place of one do not fit"
        );
        queue.notice(Some(1), "left".to_owned());
        assert!(!queue.overrun().is_cancelled());
        let sent = drained(&queue).await;
        assert_eq!(sent, [Outgoing::Text("left".to_owned())], "{sent:?}");
    }

    #[tokio::test]
    async fn the_bytes_that_wait_are_held_to_their_limit_too() {
        let queue = Queue::new(Capacity {
            messages: 100,
            bytes: 10,
        });
        let large = Bytes::from_static(&[0; 8]);
        // Into an empty queue whatever its size: the whole of a document.
        queue
            .push([Queued::sync(1, 1, &Bytes::from_static(&[0; 64]))])
            .expect("alone");
        assert_eq!(queue.push([sync(1)]), Err(NotQueued::Full));
        assert_eq!(drained(&queue).await.len(), 1);
        queue.push([Queued::sync(1, 1, &large)]).expect("8 of 10");
        queue.push([sync(1)]).expect("10 of 10");
        assert_eq!(queue.push([text(1, "x")]), Err(NotQueued::Full));
        // What is sent, dropped or replaced no longer counts.
        assert!(queue.next().await.is_some());
        queue.push([Queued::sync(2, 1, &large)]).expect("10 of 10");
        assert_eq!(queue.push([sync(2)]), Err(NotQueued::Full));
        queue.forget(2);
        queue.push([Queued::sync(1, 1, &large)]).expect("10 of 10");
        queue
            .push_anew(1, [Queued::sync(1, 2, &large), sync(1)])
            .expect("in place of both");
        assert_eq!(drained(&queue).await.len(), 2);
    }

    #[tokio::test]
    async fn the_document_anew_takes_the_place_of_what_waited_of_it() {
        let queue = queue_of(4);
        queue
            .push([text(1, "ack"), sync(1), sync(2), sync(1)])
            .expect("room");
        queue
            .push_anew(1, [text(1, "reset"), sync(1)])
            .expect("in place of two");
        let sent = drained(&queue).await;
        let texts: Vec<&str> = sent
            .iter()
            .filter_map(|outgoing| match outgoing {
                Outgoing::Text(text) => Some(text.as_str()),
                Outgoing::Binary(_) => None,
            })
            .collect();
        assert_eq!(texts, ["ack", "reset"], "the messages in words stay");
        assert_eq!(sent.len(), 4, "{sent:?}");
        assert_eq!(
            sent[1],
            Outgoing::Binary(frame::framed(2, 1, &Bytes::from_static(&[2, 0]))),
            "the other room's waits on"
        );
    }

    #[tokio::test]
    async fn notices_alone_past_what_may_wait_end_the_connection() {
        let queue = queue_of(2);
        for _ in 0..4 {
            queue.notice(None, "refused".to_owned());
        }
        assert!(!queue.overrun().is_cancelled(), "twice what may wait");
        queue.notice(None, "refused".to_owned());
        assert!(queue.overrun().is_cancelled());
        queue.close();
        assert_eq!(queue.next().await, None, "a closed queue sends nothing");
        assert_eq!(
            queue.push([sync(1)]),
            Err(NotQueued::Closed),
            "and takes nothing"
        );
        queue.notice(None, "refused".to_owned());
        assert_eq!(queue.next().await, None);
    }

    #[tokio::test]
    async fn a_wait_is_woken_by_a_push() {
        let queue = std::sync::Arc::new(queue_of(2));
        let waiting = tokio::spawn({
            let queue = std::sync::Arc::clone(&queue);
            async move { queue.next().await }
        });
        tokio::task::yield_now().await;
        queue.push([text(1, "woken")]).expect("room");
        assert_eq!(
            waiting.await.expect("the wait ends"),
            Some(Outgoing::Text("woken".to_owned()))
        );
    }
}
