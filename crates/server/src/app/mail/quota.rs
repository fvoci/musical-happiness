//! How many invitation messages each workspace has queued within the last hour.

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::auth::limiter::RetryAfter;

/// The window the hourly limit counts messages in.
const WINDOW: Duration = Duration::from_hours(1);

/// Why messages were not taken; nothing was recorded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Refused {
    /// The workspace would pass its limit; enough earlier messages leave after this.
    Limit(RetryAfter),
    /// The messages fit within the hour, but there was no place for them.
    Unplaced,
}

/// Messages admitted per workspace, as (when, how many) oldest first, kept in memory.
#[derive(Debug)]
pub(super) struct HourlyQuota {
    per_hour: u32,
    admitted: Mutex<HashMap<String, VecDeque<(Instant, u32)>>>,
}

impl HourlyQuota {
    pub(super) fn new(per_hour: u32) -> Self {
        Self {
            per_hour,
            admitted: Mutex::new(HashMap::new()),
        }
    }

    /// Records `messages` for `workspace_id` if they fit within the last hour's limit
    /// and `place` finds room for them, returning what it found. Both are decided under
    /// one lock, so no other request is counted in between.
    ///
    /// # Errors
    /// Returns [`Refused::Limit`] with how long until enough earlier messages leave the
    /// window, or [`Refused::Unplaced`] when `place` found no room; nothing is recorded.
    pub(super) fn take<T>(
        &self,
        workspace_id: &str,
        messages: u32,
        now: Instant,
        place: impl FnOnce() -> Option<T>,
    ) -> Result<T, Refused> {
        let mut admitted = self.admitted.lock().expect("mail quota lock poisoned");
        // Forget workspaces with nothing in the window, so the map stays as small as
        // the workspaces that sent mail within the hour.
        admitted.retain(|_, entries| {
            while entries
                .front()
                .is_some_and(|(at, _)| now.saturating_duration_since(*at) >= WINDOW)
            {
                entries.pop_front();
            }
            !entries.is_empty()
        });
        let earlier = admitted.get(workspace_id);
        let mut used: u32 = earlier.map_or(0, |entries| entries.iter().map(|(_, n)| n).sum());
        if used.saturating_add(messages) > self.per_hour {
            // The configuration keeps a request's recipients within the hourly limit,
            // so the window always empties far enough.
            let mut wait = WINDOW;
            for (at, count) in earlier.into_iter().flatten() {
                used -= count;
                if used.saturating_add(messages) <= self.per_hour {
                    wait = (*at + WINDOW).saturating_duration_since(now);
                    break;
                }
            }
            return Err(Refused::Limit(RetryAfter(wait)));
        }
        let placed = place().ok_or(Refused::Unplaced)?;
        admitted
            .entry(workspace_id.to_owned())
            .or_default()
            .push_back((now, messages));
        Ok(placed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn take(quota: &HourlyQuota, workspace: &str, messages: u32, at: Instant) -> Option<Duration> {
        match quota.take(workspace, messages, at, || Some(())) {
            Ok(()) => None,
            Err(Refused::Limit(RetryAfter(wait))) => Some(wait),
            Err(Refused::Unplaced) => panic!("placing always succeeds here"),
        }
    }

    #[test]
    fn a_workspace_waits_until_its_oldest_messages_leave_the_hour() {
        let quota = HourlyQuota::new(5);
        let start = Instant::now();
        assert_eq!(take(&quota, "w1", 3, start), None);
        assert_eq!(take(&quota, "w1", 2, start + Duration::from_mins(10)), None);
        assert_eq!(
            take(&quota, "w1", 1, start + Duration::from_mins(15)),
            Some(Duration::from_mins(45)),
            "the first batch leaves first"
        );
        assert_eq!(
            take(&quota, "w2", 5, start + Duration::from_mins(15)),
            None,
            "another workspace counts on its own"
        );
        assert_eq!(
            take(&quota, "w1", 4, start + Duration::from_mins(15)),
            Some(Duration::from_mins(55)),
            "needs both batches gone"
        );
        assert_eq!(
            take(&quota, "w1", 3, start + WINDOW),
            None,
            "the first batch left the window"
        );
    }

    #[test]
    fn messages_without_a_place_take_nothing_of_the_hour() {
        let quota = HourlyQuota::new(5);
        let start = Instant::now();
        assert_eq!(
            quota.take("w1", 5, start, || None::<()>),
            Err(Refused::Unplaced)
        );
        assert_eq!(take(&quota, "w1", 5, start), None, "the hour is untouched");
        let mut asked = false;
        let refused = quota.take("w1", 1, start, || {
            asked = true;
            Some(())
        });
        assert_eq!(
            refused,
            Err(Refused::Limit(RetryAfter(WINDOW))),
            "the limit is told before a place is looked for"
        );
        assert!(!asked);
    }
}
