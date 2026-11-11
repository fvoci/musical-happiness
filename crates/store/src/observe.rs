//! What a [`Store`](crate::Store) tells an [`Observer`] about its calls: how long each
//! took, whether it read or wrote, how it ended, and how long it waited for a
//! connection. For metrics, which this crate does not keep: the server gives a store an
//! observer that counts into them ([`Store::observe`](crate::Store::observe)).
//!
//! A store without an observer pays one atomic load a call, and reads no clock.

use std::time::{Duration, Instant};

use sqlx::pool::PoolConnection;

use crate::db::Scope;
use crate::{DbType, StoreError};

/// Who is told about the calls of a store. Its methods are called on the task that made
/// the call, after the fact: they are to be quick and not to block.
pub trait Observer: Send + Sync + std::fmt::Debug {
    /// A call of the store ended.
    fn called(&self, call: Call);

    /// A call waited `waited` for a connection of the pool of a database of `db`.
    fn waited_for_connection(&self, db: DbType, waited: Duration);
}

/// A call of a store: one transaction, or one connection for a read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Call {
    /// The kind of database.
    pub db: DbType,
    /// Whether it wrote.
    pub access: Access,
    /// How it ended.
    pub outcome: Outcome,
    /// How long it took, from asking for a connection to the end of its transaction.
    pub took: Duration,
}

/// Whether a call wrote.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    /// Every statement only read.
    Read,
    /// It was a transaction that may write, or some statement wrote.
    Write,
}

impl Access {
    /// Whether a call on one connection wrote: whether a statement in `scope` did.
    pub(crate) fn of(scope: &Scope<'_>) -> Self {
        if scope.wrote() {
            Self::Write
        } else {
            Self::Read
        }
    }
}

/// How a call ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// It did what it was asked.
    Done,
    /// It answered that it could not: what was asked for is not there, is taken, or is
    /// not valid. The database did its part.
    Refused,
    /// The database failed it, or gave back what cannot be read: the call did not get
    /// an answer.
    Failed,
}

impl Outcome {
    /// How a call that returned `result` ended.
    pub(crate) fn of<T>(result: &Result<T, StoreError>) -> Self {
        match result {
            Ok(_) => Self::Done,
            Err(
                StoreError::Database(_)
                | StoreError::Corrupt { .. }
                | StoreError::Schema(_)
                | StoreError::Config(_)
                | StoreError::NoOrganization,
            ) => Self::Failed,
            Err(_) => Self::Refused,
        }
    }
}

/// A call being watched: started when it asked for a connection.
pub(crate) struct Watch<'a> {
    observer: &'a dyn Observer,
    db: DbType,
    started: Instant,
}

impl<'a> Watch<'a> {
    /// Starts watching a call on a database of `db`, if there is an observer.
    pub(crate) fn start(observer: Option<&'a dyn Observer>, db: DbType) -> Option<Self> {
        observer.map(|observer| Self {
            observer,
            db,
            started: Instant::now(),
        })
    }

    /// Takes a connection from `pool`, and tells the observer how long that took, if
    /// there is one: also if no connection came.
    pub(crate) async fn acquire<DB: sqlx::Database>(
        watch: Option<&Self>,
        pool: &sqlx::Pool<DB>,
    ) -> Result<PoolConnection<DB>, sqlx::Error> {
        let Some(watch) = watch else {
            return pool.acquire().await;
        };
        let asked = Instant::now();
        let conn = pool.acquire().await;
        watch
            .observer
            .waited_for_connection(watch.db, asked.elapsed());
        conn
    }

    /// Tells the observer how the call ended.
    pub(crate) fn finish<T>(self, access: Access, result: &Result<T, StoreError>) {
        self.observer.called(Call {
            db: self.db,
            access,
            outcome: Outcome::of(result),
            took: self.started.elapsed(),
        });
    }
}
