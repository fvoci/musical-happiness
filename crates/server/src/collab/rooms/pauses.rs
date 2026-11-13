//! Where the tests stop the rooms, to look at what happens meanwhile: built with the
//! feature `test-util` only.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::Notify;

use super::super::Collab;

/// Where a room's step waits for a test.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum Pause {
    /// A periodic check read its answers and has not applied them.
    AfterRecheck,
    /// A connection coming into a room was admitted, and the room is not held yet.
    AfterAdmission,
}

/// A pause: told when the step gets there, and waited for to go on.
type Waiting = (Arc<Notify>, Arc<Notify>);

/// The pauses set, each for the next step that gets there.
#[derive(Debug, Default)]
pub(super) struct Pauses {
    /// The next immediate permission read is left unanswered: told when it is asked.
    access_stall: Mutex<Option<Arc<Notify>>>,
    /// How long every immediate permission read takes besides the store's own time.
    access_delay: Mutex<Option<Duration>>,
    /// Told when the step gets there, and waited for to go on.
    at: Mutex<HashMap<Pause, Waiting>>,
}

impl Collab {
    /// Leaves the next immediate permission read unanswered, to exercise its deadline.
    #[doc(hidden)]
    #[must_use]
    pub fn stall_next_access_check(&self) -> Arc<Notify> {
        let waiting = Arc::new(Notify::new());
        *self
            .inner
            .rooms
            .pauses
            .access_stall
            .lock()
            .expect("the pauses are not poisoned") = Some(Arc::clone(&waiting));
        waiting
    }

    /// Has every immediate permission read take `delay` more, as a slow store would.
    #[doc(hidden)]
    pub fn slow_access_reads(&self, delay: Duration) {
        *self
            .inner
            .rooms
            .pauses
            .access_delay
            .lock()
            .expect("the pauses are not poisoned") = Some(delay);
    }

    /// Pauses the next periodic check after its reads, before applying its answers:
    /// the first is told when it gets there, and it goes on when the second is.
    #[doc(hidden)]
    #[must_use]
    pub fn pause_next_recheck(&self) -> (Arc<Notify>, Arc<Notify>) {
        self.pause_next(Pause::AfterRecheck)
    }

    /// Pauses the next connection coming into a room once it is admitted, before the
    /// room is held: the first is told when it gets there, and it goes on when the
    /// second is.
    #[doc(hidden)]
    #[must_use]
    pub fn pause_next_admission(&self) -> (Arc<Notify>, Arc<Notify>) {
        self.pause_next(Pause::AfterAdmission)
    }

    /// Takes away the pauses set and not got to, so that nothing waits at them.
    #[doc(hidden)]
    pub fn unpause(&self) {
        self.inner
            .rooms
            .pauses
            .at
            .lock()
            .expect("the pauses are not poisoned")
            .clear();
    }

    fn pause_next(&self, at: Pause) -> Waiting {
        let pause = (Arc::new(Notify::new()), Arc::new(Notify::new()));
        self.inner
            .rooms
            .pauses
            .at
            .lock()
            .expect("the pauses are not poisoned")
            .insert(at, pause.clone());
        pause
    }

    /// Waits at `at` if a test paused the next step there.
    pub(super) async fn paused(&self, at: Pause) {
        let pause = self
            .inner
            .rooms
            .pauses
            .at
            .lock()
            .expect("the pauses are not poisoned")
            .remove(&at);
        if let Some((waiting, go)) = pause {
            waiting.notify_one();
            go.notified().await;
        }
    }

    /// Before an immediate permission read: never answers if a test stalled the next
    /// one, and waits as long as a test slowed every one.
    pub(super) async fn before_access_read(&self) {
        let pauses = &self.inner.rooms.pauses;
        let delay = *pauses
            .access_delay
            .lock()
            .expect("the pauses are not poisoned");
        if let Some(delay) = delay {
            tokio::time::sleep(delay).await;
        }
        let waiting = pauses
            .access_stall
            .lock()
            .expect("the pauses are not poisoned")
            .take();
        if let Some(waiting) = waiting {
            waiting.notify_one();
            std::future::pending::<()>().await;
        }
    }
}
