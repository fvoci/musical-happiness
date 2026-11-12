//! What was done that is to be accounted for: the audit log of an organization.
//!
//! A record is written in the transaction of what it records, and never changed.
//! Reading the log is for the organization's administrators, by a route still to come.

use fvoci_model::new_id;

use crate::Store;
use crate::db::{Id, Scope, now_millis};
use crate::error::StoreError;

/// What a record of the audit log says was done.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuditAction {
    /// An administrator of a workspace made themselves a member of a board of it whose
    /// content they could not read before. The target is the board.
    SelfAddedToBoard,
}

impl AuditAction {
    /// The stored form.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SelfAddedToBoard => "self_added_to_board",
        }
    }

    fn from_stored(text: &str) -> Result<Self, StoreError> {
        match text {
            "self_added_to_board" => Ok(Self::SelfAddedToBoard),
            other => Err(StoreError::Corrupt {
                column: "action",
                reason: format!("no audit action is called {other:?}"),
            }),
        }
    }
}

/// One record of the audit log: who did what to what, in which workspace, and when. The
/// organization is the one of the store's handle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditEntry {
    /// The workspace it was done in.
    pub workspace_id: String,
    /// The user who did it.
    pub actor_id: String,
    /// What was done.
    pub action: AuditAction,
    /// What it was done to.
    pub target_id: String,
    /// When, in milliseconds since the epoch; set when the record is written.
    pub create_at: i64,
}

impl Store {
    /// The records of the audit log of a workspace of the handle's organization, the
    /// oldest first.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the query fails, and [`StoreError::Corrupt`]
    /// if a stored action is none.
    pub async fn get_audit_log(&self, workspace_id: &str) -> Result<Vec<AuditEntry>, StoreError> {
        self.with_conn(async |scope| {
            let mut sql = scope.sql(
                "SELECT workspace_id, actor_id, action, target_id, create_at FROM audit_log \
                 WHERE workspace_id = ",
            );
            sql.bind(Id(workspace_id)).push(" ORDER BY create_at, id");
            scope
                .fetch_all(sql)
                .await?
                .iter()
                .map(|row| {
                    Ok(AuditEntry {
                        workspace_id: row.id(0)?,
                        actor_id: row.id(1)?,
                        action: AuditAction::from_stored(&row.text(2)?)?,
                        target_id: row.id(3)?,
                        create_at: row.int(4)?,
                    })
                })
                .collect()
        })
        .await
    }
}

/// Writes a record, in the handle's organization, at this moment.
pub(crate) async fn record(scope: &mut Scope<'_>, entry: &AuditEntry) -> Result<(), StoreError> {
    let mut sql = scope.sql(
        "INSERT INTO audit_log (id, org_id, workspace_id, actor_id, action, target_id, \
         create_at) VALUES (",
    );
    sql.bind(Id(&new_id()))
        .push_bind(", ", scope.org()?)
        .push_bind(", ", Id(&entry.workspace_id))
        .push_bind(", ", Id(&entry.actor_id))
        .push_bind(", ", entry.action.as_str())
        .push_bind(", ", Id(&entry.target_id))
        .push_bind(", ", now_millis())
        .push(")");
    scope.execute(sql).await?;
    Ok(())
}
