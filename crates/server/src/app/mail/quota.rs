//! How many invitation messages each workspace has queued within the last hour.

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// The window the hourly limit counts messages in.
const WINDOW: Duration = Duration::from_hours(1);

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

    /// Records `messages` for `workspace_id` if they fit within the last hour's limit.
    ///
    /// # Errors
    /// Returns how long until enough earlier messages leave the window; nothing is recorded.
    pub(super) fn take(
        &self,
        workspace_id: &str,
        messages: u32,
        now: Instant,
    ) -> Result<(), Duration> {
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
        let entries = admitted.entry(workspace_id.to_owned()).or_default();
        let mut used: u32 = entries.iter().map(|(_, count)| count).sum();
        if used.saturating_add(messages) <= self.per_hour {
            entries.push_back((now, messages));
            return Ok(());
        }
        // The configuration keeps a request's recipients within the hourly limit, so
        // the window always empties far enough.
        for (at, count) in entries.iter() {
            used -= count;
            if used.saturating_add(messages) <= self.per_hour {
                return Err((*at + WINDOW).saturating_duration_since(now));
            }
        }
        Err(WINDOW)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_workspace_waits_until_its_oldest_messages_leave_the_hour() {
        let quota = HourlyQuota::new(5);
        let start = Instant::now();
        quota.take("w1", 3, start).expect("first");
        quota
            .take("w1", 2, start + Duration::from_mins(10))
            .expect("second");
        let wait = quota
            .take("w1", 1, start + Duration::from_mins(15))
            .expect_err("over the limit");
        assert_eq!(
            wait,
            Duration::from_mins(45),
            "the first batch leaves first"
        );
        quota
            .take("w2", 5, start + Duration::from_mins(15))
            .expect("another workspace counts on its own");
        let wait = quota
            .take("w1", 4, start + Duration::from_mins(15))
            .expect_err("needs both batches gone");
        assert_eq!(wait, Duration::from_mins(55));
        quota
            .take("w1", 3, start + WINDOW)
            .expect("the first batch left the window");
    }
}
