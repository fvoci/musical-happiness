//! The order in which a transaction holds the links of invitations and what they hang
//! from, on PostgreSQL. Two transactions that hold what they share in one order wait
//! for each other; in two orders, each can hold what the other waits for, and
//! PostgreSQL ends one of them (`40P01`).
//!
//! 1. The turn of several links ([`take_turn`]): first, by a transaction that holds
//!    more than one link, which it may then hold in an order of its own.
//! 2. The organization's row ([`org_admin::hold_organization_row`]); under the turn,
//!    the row of each organization whose link is held, before that link
//!    ([`hold_organization`]).
//! 3. The workspace.
//! 4. The board.
//! 5. The links, a link before the links made like it for the recipients of its mail.
//!
//! A step may be left out, never taken after a later one. Deleting a board or a
//! workspace holds the organization's row and then the board or the workspace, and the
//! schema's cascades then delete the links and the registrations made by them in an
//! order of the database's own: whoever holds more than one link, or a link and then
//! anything such a deletion holds, holds the organization's row first. A transaction
//! that holds a single link, and after it nothing a deletion holds, may hold it alone:
//! a deletion waits for it.
//!
//! SQLite's writers wait for each other already: nothing here holds anything there.

use crate::db::{Dialect, Id, Scope};
use crate::error::StoreError;
use crate::org_admin;

/// The key of the advisory lock of the turn of several links: `fvoclnks`.
const SEVERAL_LINKS_LOCK: i64 = 0x6676_6f63_6c6e_6b73;

/// That the transaction holds the turn of several links, until it ends.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Turn(());

/// Takes the turn of the transactions that hold more than one link, until this one
/// ends: a registration that replaces one made by another link holds both, and so do
/// the clean-up of registrations that ended, the revocation of a link with its
/// children and turning a workspace's guest links off.
pub(crate) async fn take_turn(scope: &mut Scope<'_>) -> Result<Turn, StoreError> {
    if scope.dialect() == Dialect::Postgres {
        let mut sql = scope.sql("SELECT pg_advisory_xact_lock(");
        sql.bind(SEVERAL_LINKS_LOCK).push(")");
        scope.fetch_optional(sql).await?;
    }
    Ok(Turn(()))
}

/// Takes the turn and holds the handle's organization's row: for a transaction that
/// writes several links of the organization, which a deletion of their board or
/// workspace takes along.
pub(crate) async fn hold_links_of_organization(scope: &mut Scope<'_>) -> Result<Turn, StoreError> {
    let turn = take_turn(scope).await?;
    org_admin::hold_organization_row(scope).await?;
    Ok(turn)
}

/// Holds the row of the organization `org_id` under the turn, before a link of it, as
/// [`org_admin::hold_organization_row`] holds the handle's: whichever organization the
/// handle names, if any. The row is reached past the wall, which the caller opens for
/// another organization's.
pub(crate) async fn hold_organization(
    scope: &mut Scope<'_>,
    _turn: Turn,
    org_id: &str,
) -> Result<(), StoreError> {
    if scope.dialect() == Dialect::Postgres {
        let mut sql = scope.sql("SELECT id FROM organizations WHERE id = ");
        sql.bind(Id(org_id)).push(" FOR NO KEY UPDATE");
        scope.fetch_optional(sql).await?;
    }
    Ok(())
}
