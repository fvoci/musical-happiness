//! Row locks: how a statement holds what it reads, the rank each lock is taken at, and
//! compile-time witnesses of what a transaction holds.
//!
//! A statement holds rows only through [`Scope::row_lock`], [`Scope::hold`] and
//! [`Scope::first_held`], which record the lock at its rank as they take it: a debug
//! build asserts that a transaction takes its locks in the order of [`LockRank`]. A
//! statement that skips what others hold ([`Scope::skipping_held`]) waits for nobody
//! and is not recorded, nor are the two locks no rank fits
//! ([`Scope::unranked_row_lock`]). No other module of the crate writes a locking clause
//! (`cargo xtask check`, "layers").

use std::marker::PhantomData;

use super::{Dialect, FromRow, Scope, Sql, SqlFragment, is_sql_ident};
use crate::error::StoreError;

/// How a statement holds the rows it reads, on PostgreSQL. SQLite has no row locks: a
/// transaction that writes holds the whole database there (`BEGIN IMMEDIATE`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RowLock {
    /// `FOR UPDATE`: as a deletion, or a change of the row's key, holds it.
    Update,
    /// `FOR NO KEY UPDATE`: as a change of the row holds it; what refers to the row is
    /// still added meanwhile.
    NoKeyUpdate,
    /// `FOR SHARE`: against any change, alongside other readers that hold it so.
    Share,
    /// `FOR KEY SHARE`: against a deletion or a change of the key only, as a reference
    /// to the row holds it.
    KeyShare,
}

impl RowLock {
    /// This lock on the rows of `table` alone (`OF table`), a table or an alias of the
    /// statement's join.
    ///
    /// # Panics
    ///
    /// Panics if `table` is no plain name.
    pub(crate) fn of(self, table: &'static str) -> Locking {
        assert!(
            is_sql_ident(table),
            "invalid SQL table alias in a row lock: {table:?}"
        );
        Locking {
            lock: self,
            of: Some(table),
        }
    }
}

/// A [`RowLock`], on the rows of all the tables a statement reads or of one of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Locking {
    lock: RowLock,
    of: Option<&'static str>,
}

impl From<RowLock> for Locking {
    fn from(lock: RowLock) -> Self {
        Self { lock, of: None }
    }
}

impl Locking {
    /// The clause, for the end of a `SELECT` on PostgreSQL.
    fn clause(self) -> SqlFragment {
        let mut clause = SqlFragment::of(&[match self.lock {
            RowLock::Update => " FOR UPDATE",
            RowLock::NoKeyUpdate => " FOR NO KEY UPDATE",
            RowLock::Share => " FOR SHARE",
            RowLock::KeyShare => " FOR KEY SHARE",
        }]);
        if let Some(table) = self.of {
            clause.push(" OF ").push(table);
        }
        clause
    }
}

impl Scope<'_> {
    /// The clause by which the statement it ends holds the rows it reads under `lock`,
    /// which is recorded as taken at `(rank, key)`: on PostgreSQL. On SQLite, nothing.
    /// Pushed last, as the statement is about to run.
    ///
    /// The key of a lock on one row is the row's; of a lock on the rows of something
    /// (the files of a board), that something's.
    pub(crate) fn row_lock(
        &mut self,
        rank: LockRank,
        key: &str,
        lock: impl Into<Locking>,
    ) -> SqlFragment {
        self.record_lock(rank, key);
        match self.dialect() {
            Dialect::Postgres => lock.into().clause(),
            Dialect::Sqlite => SqlFragment::new(),
        }
    }

    /// The clause by which the statement it ends holds, under `lock`, the rows it reads
    /// that no other transaction holds, and leaves out those one does (`SKIP LOCKED`):
    /// on PostgreSQL. On SQLite, nothing. Such a statement waits for nobody, so the lock
    /// has no place in the order of [`LockRank`] and is not recorded.
    pub(crate) fn skipping_held(&self, lock: impl Into<Locking>) -> SqlFragment {
        match self.dialect() {
            Dialect::Postgres => {
                let mut clause = lock.into().clause();
                clause.push(" SKIP LOCKED");
                clause
            }
            Dialect::Sqlite => SqlFragment::new(),
        }
    }

    /// The clause by which the statement it ends holds the rows it reads under `lock`,
    /// recorded at no rank: on PostgreSQL. On SQLite, nothing. For the two locks whose
    /// place no rank of [`LockRank`] fits, as two orders of the store take them:
    ///
    /// - The marks of a board's files (`file_refs::hold_marks`), taken only under the
    ///   board, held live ([`LockRank::Container`]) or in its trash
    ///   ([`LockRank::DeletedContainers`]). A copy of a board marks the files of the
    ///   new board before it holds the membership of whoever makes it
    ///   (`members::hold_creator`); a board that comes back from the trash marks its
    ///   files after the members it reads (`members::has_acting_admin`). What a copy
    ///   marks are rows it made itself, and the files of a board in the trash are
    ///   written only under its trash. They can have a rank of their own once boards
    ///   are made and copied in one order.
    /// - The link a registration was made by (`signups::hold_link_of`), held to give
    ///   its use back: after the account that a sign-in through a provider signs in,
    ///   and, under the turn of the links, after an organization taken again, whose
    ///   row is held already. An acceptance holds a link before the trash and the
    ///   members of its workspace ([`LockRank::Invitation`]).
    pub(crate) fn unranked_row_lock(&self, lock: impl Into<Locking>) -> SqlFragment {
        match self.dialect() {
            Dialect::Postgres => lock.into().clause(),
            Dialect::Sqlite => SqlFragment::new(),
        }
    }

    /// Holds the rows `sql` selects under `lock`, recorded at `(rank, key)` as
    /// [`Scope::row_lock`] records it. Only PostgreSQL runs the statement; what it reads
    /// is not looked at.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the statement fails.
    pub(crate) async fn hold(
        &mut self,
        rank: LockRank,
        key: &str,
        mut sql: Sql,
        lock: impl Into<Locking>,
    ) -> Result<(), StoreError> {
        let clause = self.row_lock(rank, key, lock);
        if self.dialect() == Dialect::Postgres {
            sql.push(&clause);
            self.fetch_all(sql).await?;
        }
        Ok(())
    }

    /// The first row `sql` reads, held under `lock` and recorded at `rank` under the
    /// identifier its first column holds: for a row found through what it is joined
    /// to, whose key is not known before the statement reads it. Nothing is recorded
    /// if there is no row.
    ///
    /// # Errors
    ///
    /// Returns what [`Scope::first`] returns.
    pub(crate) async fn first_held<T: FromRow>(
        &mut self,
        rank: LockRank,
        mut sql: Sql,
        lock: impl Into<Locking>,
    ) -> Result<Option<T>, StoreError> {
        if self.dialect() == Dialect::Postgres {
            sql.push(&lock.into().clause());
        }
        let Some(row) = self.fetch_optional(sql).await? else {
            return Ok(None);
        };
        let key: String = FromRow::from_row(&row)?;
        self.record_lock(rank, &key);
        T::from_row(&row).map(Some)
    }
}

/// Global lock acquisition rank within a transaction (`debug_assert!` checked).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum LockRank {
    Instance,
    LinkTurn,
    Organization,
    /// The pending settings of an organization's provider, keyed by the organization:
    /// a trial holds them before the provider it writes them to.
    PendingProvider,
    /// The row of an organization's provider, held before the account that signs in,
    /// tries settings or makes a token through it.
    Provider,
    Workspace,
    /// A board, and with it the file records of it: those are held under the board at
    /// its place, keyed by it, so that a write that spans boards takes each with its
    /// files in the order of the boards.
    Container,
    /// A link into a workspace or a board, under the board it is of, before the trash
    /// and the membership of the workspace an acceptance reads.
    Invitation,
    /// An exam or a meeting of a course, under the course's board, before the
    /// membership and the account of whoever it is written for.
    CourseItem,
    DeletedContainers,
    Block,
    WorkspaceMembers,
    Users,
    /// A session, under its account.
    Session,
    /// An API token, under its account and its session.
    ApiToken,
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

/// The lock a pass of `rank` goes under, for the ranks a transaction takes again after
/// deeper locks: the rows of several organizations under the turn of several links, in
/// any order, since the turn lets one such transaction through at a time; and the
/// workspaces of an organization under its row, one pass a workspace in the order of
/// their keys, each with what it holds under it.
#[cfg(debug_assertions)]
const fn pass_under(rank: LockRank) -> Option<LockRank> {
    match rank {
        LockRank::Organization => Some(LockRank::LinkTurn),
        LockRank::Workspace => Some(LockRank::Organization),
        _ => None,
    }
}

/// Records the lock of `key` at `rank`, asserting that it comes after what `locks` holds:
/// a deeper rank than the last lock, or the same rank and a greater key (any key for
/// [`LockRank::WorkspaceMembers`]). A new pass ([`pass_under`]) is checked against the
/// previous pass under the same lock, and nothing recorded is forgotten: what each pass
/// takes is checked within it, and a lock that is not a new pass against the last.
/// Nothing is a new pass once [`LockRank::Users`] is held.
#[cfg(debug_assertions)]
pub(super) fn push_checked_lock(locks: &mut Vec<(LockRank, String)>, rank: LockRank, key: &str) {
    if locks.iter().any(|(r, k)| *r == rank && k == key) {
        return;
    }
    let holds_users = locks.iter().any(|(r, _)| *r >= LockRank::Users);
    if !holds_users
        && let Some(under) = pass_under(rank)
        && let Some(start) = locks.iter().rposition(|(r, _)| *r == under)
    {
        if rank == LockRank::Organization {
            locks.push((rank, key.to_owned()));
            return;
        }
        if let Some((_, previous)) = locks[start + 1..].iter().rev().find(|(r, _)| *r == rank) {
            debug_assert!(
                previous.as_str() < key,
                "out-of-order {rank:?} pass: after ({rank:?}, {previous:?}), acquiring ({rank:?}, {key:?}); all={locks:?}",
            );
            locks.push((rank, key.to_owned()));
            return;
        }
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

#[cfg(all(test, debug_assertions))]
mod tests {
    use super::LockRank::{
        Block, Container, DeletedContainers, Instance, LinkTurn, Organization, Users, Workspace,
        WorkspaceMembers,
    };
    use super::{LockRank, push_checked_lock};

    /// Records `sequence` in its order, as a transaction takes its locks.
    fn take(sequence: &[(LockRank, &str)]) -> Vec<(LockRank, String)> {
        let mut locks = Vec::new();
        for (rank, key) in sequence {
            push_checked_lock(&mut locks, *rank, key);
        }
        locks
    }

    #[test]
    fn each_workspace_of_an_organization_is_a_pass_with_its_own_order() {
        // As an account leaves its workspaces: each one's boards, trash and members.
        let locks = take(&[
            (Organization, "o"),
            (Workspace, "w1"),
            (Container, "c5"),
            (Container, "c7"),
            (DeletedContainers, "w1"),
            (WorkspaceMembers, "c7"),
            (WorkspaceMembers, "c5"),
            (Workspace, "w2"),
            (Container, "c1"),
            (Block, "b1"),
            (Workspace, "w3"),
        ]);
        assert_eq!(locks.len(), 11, "nothing is forgotten: {locks:?}");
    }

    #[test]
    fn the_turn_takes_organizations_in_any_order() {
        take(&[
            (Instance, "1"),
            (LinkTurn, "links"),
            (Organization, "o2"),
            (Workspace, "w9"),
            (Container, "c9"),
            (Organization, "o1"),
            (Workspace, "w1"),
        ]);
    }

    #[test]
    #[should_panic(expected = "out-of-order Workspace pass")]
    fn workspaces_are_passed_in_the_order_of_their_keys() {
        take(&[
            (Organization, "o"),
            (Workspace, "w2"),
            (Container, "c1"),
            (Workspace, "w1"),
        ]);
    }

    #[test]
    #[should_panic(expected = "lock rank violation")]
    fn a_board_is_not_taken_before_the_first_workspace() {
        take(&[(Organization, "o"), (Container, "c1"), (Workspace, "w1")]);
    }

    #[test]
    #[should_panic(expected = "lock rank violation")]
    fn the_boards_of_a_pass_are_taken_in_order() {
        take(&[
            (Organization, "o"),
            (Workspace, "w1"),
            (Container, "c2"),
            (Container, "c1"),
        ]);
    }

    #[test]
    #[should_panic(expected = "lock rank violation")]
    fn the_boards_of_a_later_pass_are_taken_in_order() {
        take(&[
            (Organization, "o"),
            (Workspace, "w1"),
            (Container, "c9"),
            (Workspace, "w2"),
            (Container, "c2"),
            (Container, "c1"),
        ]);
    }

    #[test]
    #[should_panic(expected = "lock rank violation")]
    fn no_workspace_is_passed_without_its_organization() {
        take(&[(Workspace, "w1"), (Container, "c1"), (Workspace, "w2")]);
    }

    #[test]
    #[should_panic(expected = "lock rank violation")]
    fn no_pass_begins_once_an_account_is_held() {
        take(&[
            (Organization, "o"),
            (Workspace, "w1"),
            (Users, "u"),
            (Workspace, "w2"),
        ]);
    }
}
