//! What changed of who may be in the rooms, by organization, and what each change
//! reached: what an admission or a periodic answer read before a change that reaches
//! its room and user is asked again, and nothing else is.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
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

/// Every how many changes those of every organization older than
/// [`REMEMBERED_FOR`] are forgotten, and organizations with none left with them.
const SWEPT_EVERY: u64 = 64;

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
    /// The number of the latest change of an organization that is no longer
    /// remembered at all (none of its changes was left).
    forgotten: u64,
    by_tenant: HashMap<Tenant, Kept>,
}

/// The changes of one organization remembered, oldest first.
#[derive(Debug, Default)]
struct Kept {
    /// The number of its latest change no longer remembered.
    forgotten: u64,
    changes: VecDeque<Change>,
}

#[derive(Debug)]
struct Change {
    number: u64,
    at: Instant,
    /// What it reached; shared with whoever asks what changed since.
    scope: Arc<AccessScope>,
}

impl Kept {
    /// Forgets the changes made before `before`, and more than the last
    /// [`REMEMBERED_AT_MOST`].
    fn forget(&mut self, before: Instant) {
        while let Some(oldest) = self.changes.front()
            && (self.changes.len() > REMEMBERED_AT_MOST || oldest.at < before)
        {
            self.forgotten = oldest.number;
            self.changes.pop_front();
        }
    }
}

/// The changes of one organization after a number, as what they reached: what was
/// read before them is stale where they reach.
#[derive(Debug)]
pub(super) enum Since {
    Known(Vec<Arc<AccessScope>>),
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
        let before = at.checked_sub(REMEMBERED_FOR).unwrap_or(at);
        let kept = changes.by_tenant.entry(tenant).or_default();
        kept.changes.push_back(Change {
            number,
            at,
            scope: Arc::new(scope),
        });
        kept.forget(before);
        if number.is_multiple_of(SWEPT_EVERY) {
            changes.sweep(before);
        }
        number
    }

    /// The changes of `tenant` after the change numbered `since`.
    pub(super) fn since(&self, tenant: Tenant, since: u64) -> Since {
        let changes = self.changes();
        let Some(kept) = changes.by_tenant.get(&tenant) else {
            return if since < changes.forgotten {
                Since::Forgotten
            } else {
                Since::Known(Vec::new())
            };
        };
        if since < kept.forgotten {
            return Since::Forgotten;
        }
        let scopes = kept
            .changes
            .iter()
            .rev()
            .take_while(|change| change.number > since)
            .map(|change| Arc::clone(&change.scope))
            .collect();
        Since::Known(scopes)
    }

    fn changes(&self) -> std::sync::MutexGuard<'_, Changes> {
        self.changes.lock().expect("the walls are not poisoned")
    }
}

impl Changes {
    /// Forgets every organization's changes made before `before`, and the
    /// organizations with none left.
    fn sweep(&mut self, before: Instant) {
        let mut forgotten = self.forgotten;
        self.by_tenant.retain(|_, kept| {
            kept.forget(before);
            let left = !kept.changes.is_empty();
            if !left {
                forgotten = forgotten.max(kept.forgotten);
            }
            left
        });
        self.forgotten = forgotten;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// How many changes of the organization came after `since`; `None` if some are
    /// forgotten.
    fn after(walls: &Walls, since: u64) -> Option<usize> {
        match walls.since(Tenant::default(), since) {
            Since::Known(scopes) => Some(scopes.len()),
            Since::Forgotten => None,
        }
    }

    fn raise(walls: &Walls) {
        walls.raise(Tenant::default(), AccessScope::board("board"));
    }

    #[test]
    fn what_was_read_is_measured_against_the_changes_after_it() {
        let walls = Walls::default();
        let before = walls.now();
        raise(&walls);
        let between = walls.now();
        raise(&walls);
        assert_eq!(after(&walls, before), Some(2));
        assert_eq!(after(&walls, between), Some(1));
        assert_eq!(after(&walls, walls.now()), Some(0));
    }

    #[test]
    fn what_was_read_before_a_change_no_longer_remembered_is_read_again() {
        let walls = Walls::default();
        let before = walls.now();
        for _ in 0..=REMEMBERED_AT_MOST {
            raise(&walls);
        }
        assert_eq!(after(&walls, before), None);
        assert_eq!(after(&walls, walls.now()), Some(0));
    }

    #[tokio::test(start_paused = true)]
    async fn changes_are_forgotten_with_time() {
        let walls = Walls::default();
        let before = walls.now();
        raise(&walls);
        tokio::time::advance(REMEMBERED_FOR + Duration::from_secs(1)).await;
        raise(&walls);
        assert_eq!(after(&walls, before), None, "a forgotten change counts");
        assert_eq!(after(&walls, walls.now()), Some(0));
        assert_eq!(
            walls.changes().by_tenant[&Tenant::default()].changes.len(),
            1
        );
    }

    #[tokio::test(start_paused = true)]
    async fn an_organization_whose_changes_are_all_forgotten_is_swept_away() {
        let walls = Walls::default();
        let before = walls.now();
        raise(&walls);
        tokio::time::advance(REMEMBERED_FOR + Duration::from_secs(1)).await;
        walls.changes().sweep(Instant::now() - REMEMBERED_FOR);
        assert!(walls.changes().by_tenant.is_empty());
        assert_eq!(
            after(&walls, before),
            None,
            "what was read before it is read again"
        );
        assert_eq!(after(&walls, walls.now()), Some(0));
    }
}
