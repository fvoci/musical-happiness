//! What changed of who may be in the rooms, by organization, and what each change
//! reached: what an admission or a periodic answer read before a change that reaches
//! its room and user is asked again, and nothing else is.

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::time::Duration;

use fvoci_store::Tenant;
use tokio::time::Instant;

use super::{AccessScope, Room};

/// How long a change is remembered: much longer than an admission or a periodic
/// check reads. What was read before a change no longer remembered is asked again,
/// whatever the change reached.
const REMEMBERED_FOR: Duration = Duration::from_mins(10);

/// How many changes of one organization are remembered at most, the latest.
const REMEMBERED_AT_MOST: usize = 1024;

/// The changes of access facts, numbered in one sequence for the whole server: an
/// admission that does not know its organization yet can be measured against the
/// change of any.
#[derive(Debug, Default)]
pub(super) struct Walls {
    changes: Mutex<Changes>,
}

#[derive(Debug, Default)]
struct Changes {
    /// The number of the latest change.
    now: u64,
    /// The number of the latest change no longer remembered.
    forgotten: u64,
    /// The changes remembered, of each organization, oldest first.
    by_tenant: HashMap<Tenant, VecDeque<Change>>,
}

#[derive(Debug)]
struct Change {
    number: u64,
    at: Instant,
    /// What it reached.
    scope: AccessScope,
}

/// The changes of one organization after a number, as what they reached: what was
/// read before them is stale where they reach.
#[derive(Debug)]
pub(super) enum Since {
    Known(Vec<AccessScope>),
    /// Some are no longer remembered: they may have reached anything.
    Forgotten,
}

impl Since {
    /// Whether one of the changes reaches `user_id` in `room`.
    pub(super) fn reaches(&self, room: &Room, user_id: &str) -> bool {
        match self {
            Self::Known(scopes) => scopes
                .iter()
                .any(|scope| scope.contains(room) && scope.concerns(user_id)),
            Self::Forgotten => true,
        }
    }
}

impl Walls {
    /// The number of the latest change of any organization: what is read from now on
    /// is measured against it.
    pub(super) fn now(&self) -> u64 {
        self.changes().now
    }

    /// Records that `tenant`'s access facts changed where `scope` reaches; the number
    /// of the change.
    pub(super) fn raise(&self, tenant: Tenant, scope: AccessScope) -> u64 {
        let mut changes = self.changes();
        changes.now += 1;
        let (number, at) = (changes.now, Instant::now());
        let kept = changes.by_tenant.entry(tenant).or_default();
        kept.push_back(Change { number, at, scope });
        let mut forgotten = None;
        while let Some(oldest) = kept.front()
            && (kept.len() > REMEMBERED_AT_MOST || at.duration_since(oldest.at) > REMEMBERED_FOR)
        {
            forgotten = Some(oldest.number);
            kept.pop_front();
        }
        if let Some(forgotten) = forgotten {
            changes.forgotten = changes.forgotten.max(forgotten);
        }
        number
    }

    /// The changes of `tenant` after the change numbered `since`.
    pub(super) fn since(&self, tenant: Tenant, since: u64) -> Since {
        let changes = self.changes();
        if since < changes.forgotten {
            return Since::Forgotten;
        }
        let scopes = changes
            .by_tenant
            .get(&tenant)
            .map(|kept| {
                kept.iter()
                    .rev()
                    .take_while(|change| change.number > since)
                    .map(|change| change.scope.clone())
                    .collect()
            })
            .unwrap_or_default();
        Since::Known(scopes)
    }

    fn changes(&self) -> std::sync::MutexGuard<'_, Changes> {
        self.changes.lock().expect("the walls are not poisoned")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scopes(since: &Since) -> usize {
        match since {
            Since::Known(scopes) => scopes.len(),
            Since::Forgotten => usize::MAX,
        }
    }

    #[test]
    fn what_was_read_is_measured_against_the_changes_after_it() {
        let walls = Walls::default();
        let tenant = Tenant::default();
        let before = walls.now();
        walls.raise(tenant, AccessScope::board("one"));
        let between = walls.now();
        walls.raise(tenant, AccessScope::board("two").of_user("user"));
        assert_eq!(scopes(&walls.since(tenant, before)), 2);
        assert_eq!(scopes(&walls.since(tenant, between)), 1);
        assert_eq!(scopes(&walls.since(tenant, walls.now())), 0);
    }

    #[test]
    fn what_was_read_before_a_change_no_longer_remembered_is_read_again() {
        let walls = Walls::default();
        let tenant = Tenant::default();
        let before = walls.now();
        for _ in 0..=REMEMBERED_AT_MOST {
            walls.raise(tenant, AccessScope::board("busy"));
        }
        assert!(matches!(walls.since(tenant, before), Since::Forgotten));
        let after = walls.now();
        assert_eq!(scopes(&walls.since(tenant, after)), 0);
    }
}
