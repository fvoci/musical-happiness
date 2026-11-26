//! Transaction-scoped lock ranks and compile-time lock witnesses.

use std::marker::PhantomData;

/// Global lock acquisition rank within a transaction (`debug_assert!` checked).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum LockRank {
    Instance,
    LinkTurn,
    Organization,
    Workspace,
    Container,
    DeletedContainers,
    Block,
    WorkspaceMembers,
    Users,
}

/// Compile-time witness that [`crate::users::lock_instance`] holds the instance row in
/// this transaction scope.
#[derive(Debug, Clone, Copy)]
pub(crate) struct InstanceHeld<'c>(PhantomData<fn(&'c ()) -> &'c ()>);

impl InstanceHeld<'_> {
    pub(crate) const fn new() -> Self {
        Self(PhantomData)
    }
}

/// Compile-time witness that an organization row is held in this transaction scope.
#[derive(Debug, Clone, Copy)]
pub(crate) struct OrganizationHeld<'c>(PhantomData<fn(&'c ()) -> &'c ()>);

impl OrganizationHeld<'_> {
    pub(crate) const fn new() -> Self {
        Self(PhantomData)
    }
}

/// Compile-time witness that [`crate::trash::hold_deleted_containers`] holds a
/// workspace's trashed containers in this transaction scope.
#[derive(Debug, Clone, Copy)]
pub(crate) struct DeletedContainersHeld<'c>(PhantomData<fn(&'c ()) -> &'c ()>);

impl DeletedContainersHeld<'_> {
    pub(crate) const fn new() -> Self {
        Self(PhantomData)
    }
}

#[cfg(debug_assertions)]
pub(super) fn push_checked_lock(locks: &mut Vec<(LockRank, String)>, rank: LockRank, key: &str) {
    if locks.iter().any(|(r, k)| *r == rank && k == key) {
        return;
    }
    let holds_users = locks.iter().any(|(r, _)| *r >= LockRank::Users);
    if !holds_users
        && rank == LockRank::Organization
        && locks.iter().any(|(r, _)| *r == LockRank::LinkTurn)
    {
        locks.retain(|(r, _)| *r < LockRank::Organization);
        locks.push((rank, key.to_owned()));
        return;
    }
    if !holds_users
        && rank == LockRank::Workspace
        && locks.iter().any(|(r, _)| *r == LockRank::Organization)
    {
        if let Some((_, prev_ws)) = locks.iter().rev().find(|(r, _)| *r == LockRank::Workspace) {
            debug_assert!(
                prev_ws.as_str() < key,
                "out-of-order workspace lock: holding ({:?}, {prev_ws:?}), acquiring ({rank:?}, {key:?}); all={locks:?}",
                LockRank::Workspace,
            );
        }
        locks.retain(|(r, _)| *r <= LockRank::Workspace);
        locks.push((rank, key.to_owned()));
        return;
    }
    if let Some((last_rank, last_key)) = locks.last() {
        debug_assert!(
            rank > *last_rank
                || (rank == *last_rank
                    && (rank == LockRank::WorkspaceMembers || last_key.as_str() < key)),
            "lock rank violation: holding ({last_rank:?}, {last_key:?}), acquiring ({rank:?}, {key:?}); all={locks:?}",
        );
    }
    locks.push((rank, key.to_owned()));
}
