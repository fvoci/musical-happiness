//! A deterministic seam between validating a subscription and registering it.

use std::sync::Arc;

use tokio::sync::Notify;

use super::Hub;

/// Stops the next validated subscription before it is registered. Dropping the
/// handle releases it, so a failed test cannot leave the connection waiting.
#[derive(Debug, Clone, Default)]
pub struct SubscriptionPause {
    reached: Arc<Notify>,
    resumed: Arc<Notify>,
}

impl SubscriptionPause {
    /// Waits until the subscription has passed its authorization check.
    pub async fn reached(&self) {
        self.reached.notified().await;
    }

    /// Lets the subscription attempt register itself.
    pub fn resume(&self) {
        self.resumed.notify_one();
    }
}

impl Drop for SubscriptionPause {
    fn drop(&mut self) {
        self.resume();
    }
}

impl Hub {
    /// Pauses the next valid subscription immediately before registration.
    ///
    /// # Panics
    ///
    /// Panics when another pause is already installed.
    pub fn pause_next_subscription(&self) -> SubscriptionPause {
        let pause = SubscriptionPause::default();
        let mut next = self
            .inner
            .subscription_pause
            .lock()
            .expect("the test pause lock");
        assert!(
            next.is_none(),
            "only one subscription may be paused at a time"
        );
        *next = Some(pause.clone());
        pause
    }

    pub(super) async fn pause_subscription(&self) {
        let pause = self
            .inner
            .subscription_pause
            .lock()
            .expect("the test pause lock")
            .take();
        if let Some(pause) = pause {
            pause.reached.notify_one();
            pause.resumed.notified().await;
        }
    }
}
