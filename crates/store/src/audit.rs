//! What was done that is to be accounted for: the audit log of an organization.
//!
//! A record is written in the transaction of what it records, and never changed: the
//! schema refuses a change, and a removal but with its workspace when that goes for
//! good. A record is a workspace's (read by its members) or the organization's (read by
//! its administrators).

use fvoci_model::{AuditAction, AuditActorKind, AuditPerson, AuditRecord, new_id};

use crate::db::{Id, OptId, Row, Scope, now_millis};
use crate::error::StoreError;
use crate::{Paging, Store};

/// The trusted origin of a change, chosen by its entry point rather than request data.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Actor<'a> {
    Person(&'a str),
    ServerOperator,
}

impl Actor<'_> {
    pub(crate) fn entry(
        self,
        workspace_id: &str,
        action: AuditAction,
        target_id: &str,
    ) -> AuditEntry {
        match self {
            Self::Person(id) => AuditEntry::new(workspace_id, id, action, target_id),
            Self::ServerOperator => AuditEntry {
                workspace_id: workspace_id.to_owned(),
                ..AuditEntry::by_operator(action, target_id)
            },
        }
    }
}

/// One record of the audit log as it is written: who did what to what (and whom), in
/// which workspace, and when. The organization is the one of the store's handle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditEntry {
    /// The workspace it was done in; empty for what was done to the organization.
    pub workspace_id: String,
    /// The user who did it, absent for an operator of the server.
    pub actor_id: Option<String>,
    /// Whether an account or the administration socket made the change.
    pub actor_kind: AuditActorKind,
    /// What was done.
    pub action: AuditAction,
    /// What it was done to: a board, a workspace or the organization.
    pub target_id: String,
    /// Whom it was done to, a user; empty for an action that names nobody.
    pub subject_id: String,
    /// When, in milliseconds since the epoch; set when the record is written.
    pub create_at: i64,
}

impl AuditEntry {
    /// A record of `actor_id` doing `action` to `target_id` in the workspace, to nobody.
    #[must_use]
    pub fn new(workspace_id: &str, actor_id: &str, action: AuditAction, target_id: &str) -> Self {
        Self {
            workspace_id: workspace_id.to_owned(),
            actor_id: Some(actor_id.to_owned()),
            actor_kind: AuditActorKind::Person,
            action,
            target_id: target_id.to_owned(),
            subject_id: String::new(),
            create_at: 0,
        }
    }

    /// An operator's action on the organization, made by no account.
    pub(crate) fn by_operator(action: AuditAction, target_id: &str) -> Self {
        Self {
            actor_id: None,
            actor_kind: AuditActorKind::ServerOperator,
            ..Self::new("", "", action, target_id)
        }
    }

    /// The record with whom it was done to.
    #[must_use]
    pub fn with_subject(mut self, subject_id: &str) -> Self {
        subject_id.clone_into(&mut self.subject_id);
        self
    }
}

/// Whose records are read: a workspace's, or those of the organization itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuditPlace<'a> {
    /// The records of the workspace.
    Workspace(&'a str),
    /// The records of the organization of the handle, of no workspace.
    Organization,
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
                "SELECT workspace_id, actor_id, action, target_id, subject_id, create_at, actor_kind \
                 FROM audit_log WHERE workspace_id = ",
            );
            sql.bind(Id(workspace_id)).push(" ORDER BY create_at, id");
            scope
                .fetch_all(sql)
                .await?
                .iter()
                .map(|row| {
                    Ok(AuditEntry {
                        workspace_id: row.id(0)?,
                        actor_id: Some(row.id(1)?).filter(|id| !id.is_empty()),
                        actor_kind: actor_kind(row, 6)?,
                        action: action(row, 2)?,
                        target_id: row.id(3)?,
                        subject_id: row.id(4)?,
                        create_at: row.int(5)?,
                    })
                })
                .collect()
        })
        .await
    }

    /// One page of the records of a workspace, or of the organization, the newest first
    /// (and by identifier), with the names of who did it and whom it was done to as
    /// their accounts have them now. The target's name is the workspace's for what was
    /// done to a workspace, and none otherwise: what else may be told of a board is the
    /// caller's to say.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the query fails, and [`StoreError::Corrupt`]
    /// if a stored action is none.
    pub async fn audit_records(
        &self,
        place: AuditPlace<'_>,
        paging: Paging,
    ) -> Result<Vec<AuditRecord>, StoreError> {
        self.with_conn(async |scope| {
            let mut sql = scope.sql(
                "SELECT a.id, a.action, a.actor_id, actor.username, actor.display_name, \
                 a.subject_id, subject.username, subject.display_name, a.target_id, w.name, \
                 a.create_at, a.actor_kind FROM audit_log AS a \
                 LEFT JOIN users AS actor ON actor.id = a.actor_id \
                 LEFT JOIN users AS subject ON subject.id = a.subject_id \
                 LEFT JOIN workspaces AS w ON w.id = a.target_id AND w.org_id = a.org_id \
                 WHERE a.org_id = ",
            );
            sql.bind(scope.org()?);
            match place {
                AuditPlace::Workspace(workspace_id) => {
                    sql.push_bind(" AND a.workspace_id = ", Id(workspace_id));
                }
                AuditPlace::Organization => {
                    sql.push(" AND a.workspace_id IS NULL");
                }
            }
            sql.push(" ORDER BY a.create_at DESC, a.id DESC");
            if paging.per_page > 0 {
                sql.push_bind(" LIMIT ", i64::from(paging.per_page))
                    .push_bind(
                        " OFFSET ",
                        i64::from(paging.page) * i64::from(paging.per_page),
                    );
            }
            scope.fetch_all(sql).await?.iter().map(record).collect()
        })
        .await
    }
}

fn action(row: &Row, index: usize) -> Result<AuditAction, StoreError> {
    row.text(index)?.parse().map_err(
        |error: fvoci_model::UnknownAuditAction| StoreError::Corrupt {
            column: "action",
            reason: error.to_string(),
        },
    )
}

fn actor_kind(row: &Row, index: usize) -> Result<AuditActorKind, StoreError> {
    match row.text(index)?.as_str() {
        "person" => Ok(AuditActorKind::Person),
        "server_operator" => Ok(AuditActorKind::ServerOperator),
        other => Err(StoreError::Corrupt {
            column: "actor_kind",
            reason: format!("unknown actor kind {other:?}"),
        }),
    }
}

fn record(row: &Row) -> Result<AuditRecord, StoreError> {
    let subject_id = row.id(5)?;
    let target_name = row.text(9)?;
    let action = action(row, 1)?;
    Ok(AuditRecord {
        id: row.id(0)?,
        action,
        actor: if row.id(2)?.is_empty() {
            None
        } else {
            Some(AuditPerson {
                id: row.id(2)?,
                username: row.text(3)?,
                display_name: row.text(4)?,
            })
        },
        actor_kind: actor_kind(row, 11)?,
        subject: if subject_id.is_empty() {
            None
        } else {
            Some(AuditPerson {
                id: subject_id,
                username: row.text(6)?,
                display_name: row.text(7)?,
            })
        },
        target_id: row.id(8)?,
        // A board's identifier is no workspace's: the join finds none for it.
        target_name: (!target_name.is_empty()).then_some(target_name),
        create_at: row.int(10)?,
    })
}

/// Writes a record, in the handle's organization, at this moment.
pub(crate) async fn record_entry(
    scope: &mut Scope<'_>,
    entry: &AuditEntry,
) -> Result<(), StoreError> {
    let mut sql = scope.sql(
        "INSERT INTO audit_log (id, org_id, workspace_id, actor_id, action, target_id, \
         subject_id, create_at, actor_kind) VALUES (",
    );
    sql.bind(Id(&new_id()))
        .push_bind(", ", scope.org()?)
        .push_bind(", ", OptId(&entry.workspace_id))
        .push_bind(", ", OptId(entry.actor_id.as_deref().unwrap_or_default()))
        .push_bind(", ", entry.action.as_str())
        .push_bind(", ", Id(&entry.target_id))
        .push_bind(", ", OptId(&entry.subject_id))
        .push_bind(", ", now_millis())
        .push_bind(", ", entry.actor_kind.as_str())
        .push(")");
    scope.execute(sql).await?;
    Ok(())
}
