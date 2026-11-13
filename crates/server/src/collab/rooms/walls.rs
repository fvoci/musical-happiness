//! When the facts of who may be in the rooms last changed, by organization: what an
//! admission or a periodic answer read before is asked again, and only for a change
//! of its own organization.

use std::collections::HashMap;
use std::hash::Hash;
use std::sync::{Arc, Mutex};

use fvoci_store::Tenant;

/// The changes of access facts, numbered in one sequence for the whole server, and the
/// last number of each organization. One sequence, so that an admission that does not
/// know its organization yet can be measured against the change of any. By tenant;
/// any key that tells organizations apart does.
#[derive(Debug)]
pub(super) struct Walls<T = Tenant> {
    raised: Mutex<Raised<T>>,
    /// Per organization, the immediate checks one after another: an answer never
    /// overtakes a later change of the same organization.
    turns: Mutex<HashMap<T, Arc<tokio::sync::Mutex<()>>>>,
}

#[derive(Debug)]
struct Raised<T> {
    /// The number of the latest change.
    now: u64,
    /// The number of each organization's latest change.
    by_tenant: HashMap<T, u64>,
}

impl<T> Default for Walls<T> {
    fn default() -> Self {
        Self {
            raised: Mutex::new(Raised {
                now: 0,
                by_tenant: HashMap::new(),
            }),
            turns: Mutex::new(HashMap::new()),
        }
    }
}

impl<T: Copy + Eq + Hash> Walls<T> {
    /// The number of the latest change of any organization: what is read from now on
    /// is measured against it.
    pub(super) fn now(&self) -> u64 {
        self.raised.lock().expect("the walls are not poisoned").now
    }

    /// Records that `tenant`'s access facts changed.
    pub(super) fn raise(&self, tenant: T) {
        let mut raised = self.raised.lock().expect("the walls are not poisoned");
        raised.now += 1;
        let now = raised.now;
        raised.by_tenant.insert(tenant, now);
    }

    /// Whether `tenant`'s access facts changed after the change numbered `since`.
    pub(super) fn raised_since(&self, tenant: T, since: u64) -> bool {
        self.raised
            .lock()
            .expect("the walls are not poisoned")
            .by_tenant
            .get(&tenant)
            .is_some_and(|at| *at > since)
    }

    /// The turn of `tenant`'s immediate checks.
    pub(super) fn turn(&self, tenant: T) -> Arc<tokio::sync::Mutex<()>> {
        let mut turns = self.turns.lock().expect("the walls are not poisoned");
        Arc::clone(turns.entry(tenant).or_default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_change_is_seen_by_its_organization_alone() {
        let walls: Walls<u8> = Walls::default();
        let (one, other) = (1, 2);
        let before = walls.now();
        walls.raise(one);
        assert!(walls.raised_since(one, before));
        assert!(!walls.raised_since(other, before));
        let after = walls.now();
        assert!(!walls.raised_since(one, after));
        walls.raise(other);
        assert!(walls.raised_since(other, after));
        assert!(!walls.raised_since(one, after));
    }
}
