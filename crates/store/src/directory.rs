//! What is asked before the organization of a request is known, and what makes an
//! organization: which workspaces a user has, whose a workspace or a shared board is.
//!
//! Every function here runs one statement, or one short transaction, that reaches the
//! rows of every organization ([`Tenant::System`](crate::Tenant::System)) and is
//! narrowed by what the caller holds: a user's identifier, a read token. Nothing else
//! of the server asks across organizations on behalf of a request.

use fvoci_model::{OrgRole, Team, UserWorkspace, WorkspaceRole, new_id};
use subtle::ConstantTimeEq as _;

use crate::Store;
use crate::db::{Dialect, Id, Row, Scope, now_millis};
use crate::error::StoreError;
use crate::teams;
use crate::uploads::UploadUsage;

/// The organization that holds what the server ships with and is nobody's: the
/// built-in templates. The schema makes it, under this identifier.
pub const SYSTEM_ORG_ID: &str = "00000000-0000-7000-8000-00000000a000";

/// The workspace of the system organization, which the built-in templates are in.
pub const SYSTEM_WORKSPACE_ID: &str = "00000000-0000-7000-8000-00000000a001";

/// The condition every lookup here ends with: the workspace `t` and its organization
/// `o` are in use, and are not the system's, which is nobody's to work in.
const IN_USE: &str = " AND t.delete_at = 0 AND o.delete_at = 0 AND o.is_system = false \
                      AND t.is_system = false";

/// What a user is in a workspace, and whose the workspace is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceAccess {
    /// The organization the workspace belongs to.
    pub org_id: String,
    /// What the user is in that organization; `None` for somebody who is in the
    /// workspace without belonging to the organization.
    pub org_role: Option<OrgRole>,
    /// What the user is in the workspace.
    pub workspace_role: WorkspaceRole,
}

impl Store {
    /// Makes an organization with the given name and its first workspace, which is its
    /// default one, and returns the organization's identifier. Nobody is a member of
    /// either yet.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::InvalidId`] if the workspace's identifier is not one, and
    /// [`StoreError::Database`] if a statement fails, as it does on an SQLite database
    /// that holds an organization already.
    pub async fn create_organization(
        &self,
        name: &str,
        workspace: &Team,
    ) -> Result<String, StoreError> {
        self.system()
            .transaction(async |scope| create_organization(scope, name, workspace).await)
            .await
    }

    /// The organization that the routes which name no workspace work in: the oldest
    /// one, made now with `workspace` as its first if there is none.
    ///
    /// TEMPORARY. It stands in for the workspace a request will name, and goes with
    /// the last route that names none.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if a statement fails.
    pub async fn only_organization(&self, workspace: &Team) -> Result<String, StoreError> {
        self.system()
            .transaction(async |scope| {
                // One at a time: of two servers that start on an empty database, the
                // second waits here and then finds the organization of the first. On
                // SQLite a transaction that writes holds the whole database.
                let mut sql = scope.sql("SELECT id FROM instance WHERE id = 1");
                if scope.dialect() == Dialect::Postgres {
                    sql.push(" FOR UPDATE");
                }
                scope.fetch_optional(sql).await?;

                let mut sql = scope.sql("SELECT id FROM organizations WHERE is_system = ");
                sql.bind(false).push(" ORDER BY create_at, id LIMIT 1");
                match scope.fetch_optional(sql).await? {
                    Some(row) => row.id(0),
                    None => create_organization(scope, "", workspace).await,
                }
            })
            .await
    }

    /// What the user is in the workspace, and whose the workspace is; `None` if the
    /// user is no member of it, whether it exists or not. The user is the one of a
    /// session: whether the account is still in use is the session's to say.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the query fails, and [`StoreError::Corrupt`]
    /// if a stored role is none.
    pub async fn resolve_workspace(
        &self,
        user_id: &str,
        workspace_id: &str,
    ) -> Result<Option<WorkspaceAccess>, StoreError> {
        self.system()
            .with_conn(async |scope| {
                let mut sql = scope.sql(
                    "SELECT t.org_id, wm.role, om.role FROM workspace_members AS wm \
                     JOIN teams AS t ON t.id = wm.workspace_id \
                     JOIN organizations AS o ON o.id = t.org_id \
                     LEFT JOIN organization_members AS om \
                     ON om.org_id = t.org_id AND om.user_id = wm.user_id \
                     WHERE wm.user_id = ",
                );
                sql.bind(Id(user_id))
                    .push_bind(" AND wm.workspace_id = ", Id(workspace_id))
                    .push(IN_USE);
                let Some(row) = scope.fetch_optional(sql).await? else {
                    return Ok(None);
                };
                let org_role = row.text(2)?;
                Ok(Some(WorkspaceAccess {
                    org_id: row.id(0)?,
                    workspace_role: role(&row, 1)?,
                    org_role: if org_role.is_empty() {
                        None
                    } else {
                        Some(role(&row, 2)?)
                    },
                }))
            })
            .await
    }

    /// The workspaces the user is a member of, in every organization: by organization,
    /// the oldest first, and in each the oldest workspace first.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the query fails, and [`StoreError::Corrupt`]
    /// if a stored role is none.
    pub async fn workspaces_of_user(
        &self,
        user_id: &str,
    ) -> Result<Vec<UserWorkspace>, StoreError> {
        self.system()
            .with_conn(async |scope| {
                let mut sql = scope.sql(
                    "SELECT t.id, t.org_id, t.name, wm.role FROM workspace_members AS wm \
                     JOIN teams AS t ON t.id = wm.workspace_id \
                     JOIN organizations AS o ON o.id = t.org_id \
                     WHERE wm.user_id = ",
                );
                sql.bind(Id(user_id))
                    .push(IN_USE)
                    .push(" ORDER BY o.create_at, o.id, t.create_at, t.id");
                scope
                    .fetch_all(sql)
                    .await?
                    .iter()
                    .map(|row| {
                        Ok(UserWorkspace {
                            id: row.id(0)?,
                            org_id: row.id(1)?,
                            name: row.text(2)?,
                            role: role(row, 3)?,
                        })
                    })
                    .collect()
            })
            .await
    }

    /// Returns what the open upload sessions hold back at `now`: those that are not
    /// closed and have not ended. A session that was closed and is not thrown away yet,
    /// as after a crash, does not count.
    ///
    /// The limits these figures are for are the user's and the server's, not an
    /// organization's: the sessions of every organization are counted, the user's own
    /// by the user's identifier. Nothing of another's session is told but its size in
    /// the server's sum.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the query fails.
    pub async fn upload_usage(&self, user_id: &str, now: i64) -> Result<UploadUsage, StoreError> {
        self.system()
            .with_conn(async |scope| {
                let mut sql = scope.sql("SELECT CAST(COALESCE(SUM(CASE WHEN user_id = ");
                sql.bind(Id(user_id))
                .push(" THEN 1 ELSE 0 END), 0) AS BIGINT), CAST(COALESCE(SUM(CASE WHEN user_id = ")
                .bind(Id(user_id))
                .push(
                    " THEN size ELSE 0 END), 0) AS BIGINT), CAST(COALESCE(SUM(size), 0) AS BIGINT) \
                     FROM uploads WHERE closed_at = 0 AND expires_at > ",
                )
                .bind(now);
                let row = scope.fetch_optional(sql).await?.ok_or(StoreError::NoRows)?;
                Ok(UploadUsage {
                    user_sessions: row.int(0)?,
                    user_bytes: row.int(1)?,
                    server_bytes: row.int(2)?,
                })
            })
            .await
    }

    /// The organization of a board that is shared, for whoever holds its read token;
    /// `None` if the board is not shared, is not there, or the token is not its own.
    /// Nothing of the system organization is shared, and nothing of a workspace or an
    /// organization that was deleted.
    ///
    /// The tokens are compared without showing through timing where they differ.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the query fails.
    pub async fn org_of_shared_board(
        &self,
        board_id: &str,
        read_token: &str,
    ) -> Result<Option<String>, StoreError> {
        self.system()
            .with_conn(async |scope| {
                let mut sql = scope.sql(
                    "SELECT t.org_id, s.token FROM sharing AS s \
                     JOIN boards AS b ON b.id = s.board_id \
                     JOIN teams AS t ON t.id = b.team_id \
                     JOIN organizations AS o ON o.id = t.org_id WHERE s.board_id = ",
                );
                sql.bind(Id(board_id))
                    .push_bind(" AND s.enabled = ", true)
                    .push(IN_USE);
                let Some(row) = scope.fetch_optional(sql).await? else {
                    return Ok(None);
                };
                let token = row.text(1)?;
                // A board that is not shared has no token; nothing matches that, not
                // even nothing.
                let held: bool = token.as_bytes().ct_eq(read_token.as_bytes()).into();
                if token.is_empty() || !held {
                    return Ok(None);
                }
                row.id(0).map(Some)
            })
            .await
    }
}

async fn create_organization(
    scope: &mut Scope<'_>,
    name: &str,
    workspace: &Team,
) -> Result<String, StoreError> {
    let id = new_id();
    let now = now_millis();
    let mut sql =
        scope.sql("INSERT INTO organizations (id, name, settings, create_at, update_at) VALUES (");
    sql.bind(Id(&id))
        .push_bind(", ", name)
        .push(", '{}'")
        .push_bind(", ", now)
        .push_bind(", ", now)
        .push(")");
    scope.execute(sql).await?;
    teams::insert_team(scope, workspace, Id(&id)).await?;
    let mut sql = scope.sql("UPDATE organizations SET default_workspace_id = ");
    sql.bind(Id(&workspace.id))
        .push_bind(" WHERE id = ", Id(&id));
    scope.execute(sql).await?;
    Ok(id)
}

/// Reads a role from a column.
fn role<R: std::str::FromStr<Err = fvoci_model::UnknownRole>>(
    row: &Row,
    index: usize,
) -> Result<R, StoreError> {
    row.text(index)?
        .parse()
        .map_err(|error: fvoci_model::UnknownRole| StoreError::Corrupt {
            column: "role",
            reason: error.to_string(),
        })
}
