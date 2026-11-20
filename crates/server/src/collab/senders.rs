//! Who sent an update that ended a worker, by taking longer than it may or by breaking
//! it: their updates are refused for a little while, ten seconds the first time (and the
//! connection that sent it is closed), twice as long each further time, and so are
//! those of everyone in their organization from its third such sender within a day. An
//! update is no more than a worker's time limit to anybody else (the worker is started
//! again at once for it, [`super::worker::Blame::Sender`]); this keeps its sender from
//! sending it again and again. Updates a sender sent together are refused as they reach
//! the worker after the first that ended it ([`super::place::AskedFor::Sender`]); those
//! under way on other workers at that time may end them too.

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

/// How long a sender is remembered after their last death: the deaths within it of the
/// one before are counted together, and the organization counts the senders it
/// remembers. Longer than the longest hold, so that a sender held that long is held
/// again at the next.
const REMEMBERED: Duration = Duration::from_hours(24);

/// How many of the users an organization remembers ([`REMEMBERED`]) are held back
/// alone: when one more ends a worker, everyone in it is, and longer for each further
/// one. One that sends from many users gets no more than that before the organization
/// is held; one user is held alone however often. A user without a session counts each
/// time, as one cannot be told from another.
const ORGANIZATION_SPARED: u32 = 2;

/// The deaths of a sender.
#[derive(Debug, Clone, Copy)]
struct Deaths {
    count: u32,
    last: Instant,
    /// Until when their updates are refused.
    held_until: Option<Instant>,
}

/// What an organization remembers of the updates of its users that ended a worker.
#[derive(Debug, Default)]
struct Organization {
    /// Its users with a session, by their identifier.
    senders: HashMap<String, Deaths>,
    /// When a user without a session ended one.
    unnamed: Vec<Instant>,
    /// Until when the updates of all its users are refused.
    held_until: Option<Instant>,
}

impl Organization {
    /// Forgets the senders not remembered at `now`. `false` if nothing is left that it
    /// would need: no sender, and no hold.
    fn forget(&mut self, now: Instant) -> bool {
        let remembered = |last: Instant| now.duration_since(last) <= REMEMBERED;
        self.senders.retain(|_, deaths| remembered(deaths.last));
        self.unnamed.retain(|at| remembered(*at));
        !self.senders.is_empty()
            || !self.unnamed.is_empty()
            || self.held_until.is_some_and(|until| until > now)
    }

    /// Counts a death of a user at `now`, and holds everyone back if the user is one
    /// more that it remembers past [`ORGANIZATION_SPARED`].
    fn ended_worker(&mut self, user_id: &str, now: Instant) {
        let new_user = if user_id.is_empty() {
            self.unnamed.push(now);
            true
        } else {
            let new_user = !self.senders.contains_key(user_id);
            let deaths = self.senders.entry(user_id.to_owned()).or_insert(Deaths {
                count: 0,
                last: now,
                held_until: None,
            });
            deaths.count = deaths.count.saturating_add(1);
            deaths.last = now;
            if let Some(hold) = hold(deaths.count, 0) {
                deaths.held_until = Some(now + hold);
            }
            new_user
        };
        if new_user {
            let users = self.senders.len().saturating_add(self.unnamed.len());
            let users = u32::try_from(users).unwrap_or(u32::MAX);
            if let Some(hold) = hold(users, ORGANIZATION_SPARED) {
                self.held_until = Some(now + hold);
            }
        }
    }
}

/// The organizations whose users' updates ended a worker.
#[derive(Debug, Default)]
pub(super) struct Senders {
    organizations: Mutex<HashMap<Tenant, Organization>>,
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
        let organizations = self
            .organizations
            .lock()
            .expect("the organizations are not poisoned");
        let organization = organizations.get(&tenant)?;
        let sender = organization
            .senders
            .get(user_id)
            .and_then(|deaths| deaths.held_until);
        [sender, organization.held_until]
            .into_iter()
            .flatten()
            .filter(|until| *until > now)
            .max()
    }

    /// Puts down that an update of a user of an organization ended a worker at `now`,
    /// after forgetting the senders, and the organizations, no longer remembered.
    pub(super) fn ended_worker(&self, tenant: Tenant, user_id: &str, now: Instant) {
        let mut organizations = self
            .organizations
            .lock()
            .expect("the organizations are not poisoned");
        organizations.retain(|_, organization| organization.forget(now));
        organizations
            .entry(tenant)
            .or_default()
            .ended_worker(user_id, now);
    }
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
    use super::*;

    /// The holds are kept by tenant whatever it is: two that are no organization stand
    /// for two organizations here. Naming an organization is for the few files that
    /// make a store handle of one (the store's
    /// `the_server_names_an_organization_for_a_handle_in_few_places`).
    const ONE: Tenant = Tenant::System;

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
    fn the_organization_is_held_from_its_third_sender() {
        let senders = Senders::default();
        let now = Instant::now();
        for _ in 0..5 {
            senders.ended_worker(ONE, "ann", now);
        }
        assert!(senders.held_until(ONE, "ann", now).is_some());
        assert_eq!(
            senders.held_until(ONE, "cat", now),
            None,
            "one user is held alone however often"
        );
        senders.ended_worker(ONE, "bob", now);
        assert_eq!(senders.held_until(ONE, "bob", now), Some(now + FIRST_HOLD));
        assert_eq!(
            senders.held_until(ONE, "ann", now),
            Some(now + FIRST_HOLD * 16),
            "her fifth"
        );
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
        let other = Tenant::Unset;
        assert_eq!(
            senders.held_until(other, "cat", now),
            None,
            "another organization"
        );
    }

    /// Two users whose deaths are more than a day apart are never three in the
    /// organization's day: the first is forgotten before they end a worker again.
    #[test]
    fn two_users_do_not_hold_the_organization_across_a_day() {
        const HOUR: Duration = Duration::from_hours(1);
        let senders = Senders::default();
        let start = Instant::now();
        senders.ended_worker(ONE, "ann", start);
        senders.ended_worker(ONE, "bob", start + HOUR * 20);
        let later = start + HOUR * 25;
        senders.ended_worker(ONE, "ann", later);
        assert_eq!(senders.held_until(ONE, "cat", later), None);
        assert_eq!(
            senders.held_until(ONE, "ann", later),
            Some(later + FIRST_HOLD),
            "her first again"
        );
    }

    /// Users a day apart each, however many, are one at a time to the organization.
    #[test]
    fn users_a_day_apart_never_hold_the_organization() {
        let senders = Senders::default();
        let start = Instant::now();
        for (day, user) in (0..12).zip(["ann", "bob", "cat", "dan"].into_iter().cycle()) {
            let now = start + Duration::from_hours(23) * day;
            senders.ended_worker(ONE, user, now);
            assert_eq!(senders.held_until(ONE, "eve", now), None, "day {day}");
        }
    }

    #[test]
    fn a_sender_again_does_not_hold_the_organization_again() {
        let senders = Senders::default();
        let start = Instant::now();
        for user in ["ann", "bob", "cat"] {
            senders.ended_worker(ONE, user, start);
        }
        assert_eq!(
            senders.held_until(ONE, "dan", start),
            Some(start + FIRST_HOLD)
        );
        let later = start + FIRST_HOLD * 2;
        senders.ended_worker(ONE, "ann", later);
        assert_eq!(senders.held_until(ONE, "dan", later), None);
        assert_eq!(
            senders.held_until(ONE, "ann", later),
            Some(later + FIRST_HOLD * 2),
            "her second"
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
