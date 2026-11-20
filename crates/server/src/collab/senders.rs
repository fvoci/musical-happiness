//! Who sent an update that ended a worker, by taking longer than it may or by breaking
//! it: their updates are refused for a little while, ten seconds the first time (and the
//! connection that sent it is closed), twice as long each further time, and so are
//! those of everyone in their organization from its third time. An update is no
//! more than a worker's time limit to anybody else (the worker is started again at
//! once for it, [`super::worker::Blame::Sender`]); this keeps its sender from sending
//! it again and again.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use fvoci_store::Tenant;
use tokio::time::Instant;

/// How long the updates of a sender are refused after the first that ended a worker:
/// short, as one may end it by chance; one who sends it again is held longer.
const FIRST_HOLD: Duration = Duration::from_secs(10);

/// The longest the updates of a sender or of an organization are refused for.
const LONGEST_HOLD: Duration = Duration::from_mins(60);

/// How long the deaths of a sender are remembered after the last: the deaths within it
/// of the one before are counted together. Longer than the longest hold, so that a
/// sender held that long is held again at the next.
const REMEMBERED: Duration = Duration::from_hours(24);

/// How many deaths of an organization's updates are not held against everyone in it:
/// those of its senders are held against them alone. One that sends from many users
/// gets no more than that before the organization is held.
const ORGANIZATION_SPARED: u32 = 2;

/// Whose deaths are counted.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Whose {
    /// A user of an organization.
    Sender(Tenant, String),
    /// Every user of an organization.
    Organization(Tenant),
}

/// The deaths of a sender or an organization.
#[derive(Debug, Clone, Copy)]
struct Deaths {
    count: u32,
    last: Instant,
    /// Until when its updates are refused.
    held_until: Option<Instant>,
}

/// The senders and organizations whose updates ended a worker.
#[derive(Debug, Default)]
pub(super) struct Senders {
    deaths: Mutex<HashMap<Whose, Deaths>>,
}

impl Senders {
    /// Until when the updates of a user of an organization are refused, at `now`; `None`
    /// if they are not. A user without a session (an empty `user_id`) is held with the
    /// organization alone.
    pub(super) fn held_until(
        &self,
        tenant: Tenant,
        user_id: &str,
        now: Instant,
    ) -> Option<Instant> {
        let deaths = self.deaths.lock().expect("the deaths are not poisoned");
        whose(tenant, user_id)
            .filter_map(|whose| deaths.get(&whose)?.held_until)
            .filter(|until| *until > now)
            .max()
    }

    /// Puts down that an update of a user of an organization ended a worker at `now`.
    pub(super) fn ended_worker(&self, tenant: Tenant, user_id: &str, now: Instant) {
        let mut deaths = self.deaths.lock().expect("the deaths are not poisoned");
        deaths.retain(|_, deaths| now.duration_since(deaths.last) <= REMEMBERED);
        for whose in whose(tenant, user_id) {
            let spared = match whose {
                Whose::Sender(..) => 0,
                Whose::Organization(_) => ORGANIZATION_SPARED,
            };
            let entry = deaths.entry(whose).or_insert(Deaths {
                count: 0,
                last: now,
                held_until: None,
            });
            entry.count = entry.count.saturating_add(1);
            entry.last = now;
            if let Some(hold) = hold(entry.count, spared) {
                entry.held_until = Some(now + hold);
            }
        }
    }
}

/// Whose deaths an update of a user of an organization counts in.
fn whose(tenant: Tenant, user_id: &str) -> impl Iterator<Item = Whose> {
    let sender = (!user_id.is_empty()).then(|| Whose::Sender(tenant, user_id.to_owned()));
    sender.into_iter().chain([Whose::Organization(tenant)])
}

/// How long updates are refused after `count` deaths, the first `spared` of which are
/// not held: [`FIRST_HOLD`], doubling up to [`LONGEST_HOLD`].
fn hold(count: u32, spared: u32) -> Option<Duration> {
    let past = count.checked_sub(spared)?.checked_sub(1)?;
    Some(
        FIRST_HOLD
            .saturating_mul(1 << past.min(16))
            .min(LONGEST_HOLD),
    )
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::*;

    const ONE: Tenant = Tenant::Org(Uuid::from_u128(1));

    #[test]
    fn a_sender_is_held_for_longer_each_time_up_to_the_longest() {
        let senders = Senders::default();
        let start = Instant::now();
        assert_eq!(senders.held_until(ONE, "ann", start), None);
        let mut now = start;
        for expected in [10, 20, 40, 80, 160, 320, 640, 1280, 2560, 3600, 3600] {
            senders.ended_worker(ONE, "ann", now);
            let until = senders.held_until(ONE, "ann", now).expect("held");
            assert_eq!(until - now, Duration::from_secs(expected), "after {now:?}");
            // Sent again as soon as it is let.
            now = until;
            assert_eq!(senders.held_until(ONE, "ann", now), None, "let again");
        }
    }

    #[test]
    fn a_sender_is_forgotten_a_day_after_the_last_time() {
        let senders = Senders::default();
        let start = Instant::now();
        senders.ended_worker(ONE, "ann", start);
        senders.ended_worker(ONE, "ann", start);
        let later = start + REMEMBERED + Duration::from_secs(1);
        senders.ended_worker(ONE, "ann", later);
        assert_eq!(
            senders.held_until(ONE, "ann", later),
            Some(later + FIRST_HOLD),
            "counted from the first again"
        );
    }

    #[test]
    fn the_organization_is_held_from_its_third_time() {
        let senders = Senders::default();
        let now = Instant::now();
        senders.ended_worker(ONE, "ann", now);
        senders.ended_worker(ONE, "bob", now);
        assert_eq!(senders.held_until(ONE, "ann", now), Some(now + FIRST_HOLD));
        assert_eq!(
            senders.held_until(ONE, "cat", now),
            None,
            "the others are not"
        );
        senders.ended_worker(ONE, "dan", now);
        assert_eq!(
            senders.held_until(ONE, "cat", now),
            Some(now + FIRST_HOLD),
            "everyone is"
        );
        assert_eq!(senders.held_until(ONE, "", now), Some(now + FIRST_HOLD));
        let other = Tenant::Org(Uuid::from_u128(2));
        assert_eq!(
            senders.held_until(other, "cat", now),
            None,
            "another organization"
        );
    }

    #[test]
    fn a_user_without_a_session_counts_for_the_organization_alone() {
        let senders = Senders::default();
        let now = Instant::now();
        for _ in 0..ORGANIZATION_SPARED {
            senders.ended_worker(ONE, "", now);
        }
        assert_eq!(senders.held_until(ONE, "", now), None);
        senders.ended_worker(ONE, "", now);
        assert_eq!(senders.held_until(ONE, "ann", now), Some(now + FIRST_HOLD));
    }
}
