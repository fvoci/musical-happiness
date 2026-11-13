//! Where the tests stop the rooms, to look at what happens meanwhile: built with the
//! feature `test-util` only.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

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

    /// Never answers, if a test stalled the next immediate permission read.
    pub(super) async fn stalled_access(&self) {
        let waiting = self
            .inner
            .rooms
            .pauses
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
