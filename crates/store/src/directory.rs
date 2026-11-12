//! What is asked before the organization of a request is known, and what makes an
//! organization: which workspaces a user has, whose a workspace or a shared board is,
//! what a user is in an organization, which sign-in through a provider comes back and
//! what a login page shows of an organization.
//! And whether an object of the file storage is held, which is asked of every
//! organization: an object's place is one across them.
//!
//! Every function here runs one statement, or one short transaction, that reaches the
//! rows of every organization ([`Tenant::System`](crate::Tenant::System)) and is
//! narrowed by what the caller holds: a user's identifier, a read token, the places of
//! objects. Nothing else of the server asks across organizations on behalf of a
//! request.

use std::collections::BTreeSet;

use fvoci_model::{LoginOrganization, OrgRole, UserWorkspace, Workspace, WorkspaceRole, new_id};
use subtle::ConstantTimeEq as _;

use crate::Store;
use crate::db::{Dialect, Id, Row, Scope, now_millis};
use crate::error::StoreError;
use crate::file_info;
use crate::loose_objects::LooseState;
use crate::oidc::{LoginFlow, flow_from_row};
use crate::uploads::{OpenUploads, UploadUsage};
use crate::workspaces;

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

impl WorkspaceAccess {
    /// Whether the user works in the workspace: a member of it who is no guest. A
    /// guest is on the boards they were invited to and reaches those by their board;
    /// what a member may do in the workspace (see its open boards, make a board, join
    /// one) a guest may not.
    #[must_use]
    pub fn works_there(&self) -> bool {
        self.workspace_role != WorkspaceRole::Guest
    }
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
        workspace: &Workspace,
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
    pub async fn only_organization(&self, workspace: &Workspace) -> Result<String, StoreError> {
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
                sql.bind(false)
                    .push_bind(" AND delete_at = ", 0_i64)
                    .push(" ORDER BY create_at, id LIMIT 1");
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
                     JOIN workspaces AS t ON t.id = wm.workspace_id \
                     JOIN organizations AS o ON o.id = t.org_id \
                     LEFT JOIN organization_members AS om \
                     ON om.org_id = t.org_id AND om.user_id = wm.user_id \
                     WHERE wm.user_id = ",
                );
                sql.bind(Id(user_id))
                    .push_bind(" AND wm.workspace_id = ", Id(workspace_id))
                    .push(IN_USE)
                    .push(&scope.same_organization(&[("wm", "t")]));
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
                     JOIN workspaces AS t ON t.id = wm.workspace_id \
                     JOIN organizations AS o ON o.id = t.org_id \
                     WHERE wm.user_id = ",
                );
                sql.bind(Id(user_id))
                    .push(IN_USE)
                    .push(&scope.same_organization(&[("wm", "t")]))
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

    /// Returns the upload sessions open at `now`, all users' together and of every
    /// organization, as [`Store::upload_usage`] counts them for the server: a number
    /// of the whole server, for its metrics.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the query fails.
    pub async fn open_uploads(&self, now: i64) -> Result<OpenUploads, StoreError> {
        self.system()
            .with_conn(async |scope| {
                let mut sql = scope.sql(
                    "SELECT CAST(COUNT(*) AS BIGINT), CAST(COALESCE(SUM(size), 0) AS BIGINT) \
                 FROM uploads WHERE closed_at = 0 AND expires_at > ",
                );
                sql.bind(now);
                let row = scope.fetch_optional(sql).await?.ok_or(StoreError::NoRows)?;
                Ok(OpenUploads {
                    sessions: row.int(0)?,
                    bytes: row.int(1)?,
                })
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

    /// Whether a row of any organization names a place of the file storage otherwise
    /// than the one way: a record, of a board or of one in the trash, a listed object,
    /// or an upload session. Asked once a process: the store takes no such row since
    /// it has the rule, and while one is there nothing is removed
    /// ([`Store::held_objects`]). One found is logged with its place.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the query fails.
    pub async fn other_spellings_stored(&self) -> Result<bool, StoreError> {
        let found = self
            .inner
            .other_spellings
            .get_or_try_init(async || {
                let found = self.system().with_conn(file_info::other_spellings).await?;
                if let Some(path) = found.first() {
                    tracing::error!(
                        count = found.len(),
                        path,
                        "rows name places of the file storage otherwise than the one way; \
                         no object is removed until they are written the one way"
                    );
                }
                Ok::<_, StoreError>(!found.is_empty())
            })
            .await?;
        Ok(*found)
    }

    /// The places among `paths` whose object something of any organization holds: a
    /// file's record, of a board or of a board in the trash, an object being written
    /// there, or an upload session that makes its file there, but for `except_upload`.
    /// Whatever a row says of its board or organization, an object it names is not
    /// removed under it: whether one may go is asked here, of every organization,
    /// before it is removed.
    ///
    /// A place is compared as it is spelled, which is the one way the storage takes
    /// ([`fvoci_model::is_storage_path`]). One among `paths` spelled otherwise is held:
    /// what another spelling would name is not known, and nothing is removed under it.
    /// And while a row stored before that rule names a place otherwise (asked once a
    /// process), every place is held.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if a query fails.
    pub async fn held_objects(
        &self,
        paths: &[&str],
        except_upload: Option<&str>,
    ) -> Result<BTreeSet<String>, StoreError> {
        let (paths, unknown): (Vec<&str>, Vec<&str>) = paths
            .iter()
            .copied()
            .partition(|path| fvoci_model::is_storage_path(path));
        let mut held: BTreeSet<String> = unknown.into_iter().map(str::to_owned).collect();
        if paths.is_empty() {
            return Ok(held);
        }
        if self.other_spellings_stored().await? {
            held.extend(paths.into_iter().map(str::to_owned));
            return Ok(held);
        }
        self.system()
            .with_conn(async |scope| {
                let (trashed_path, trashed_files) = match scope.dialect() {
                    Dialect::Sqlite => {
                        ("json_extract(f.value, '$.path')", "json_each(d.files) AS f")
                    }
                    Dialect::Postgres => (
                        "f.value ->> 'path'",
                        "json_array_elements(d.files) AS f(value)",
                    ),
                };
                let mut sql = scope.sql("SELECT path FROM file_info WHERE path IN (");
                sql.bind_list(paths.iter().copied())
                    .push(&format!(
                        ") UNION SELECT {trashed_path} FROM deleted_boards AS d, {trashed_files} \
                         WHERE {trashed_path} IN ("
                    ))
                    .bind_list(paths.iter().copied())
                    .push(") UNION SELECT path FROM loose_objects WHERE path IN (")
                    .bind_list(paths.iter().copied())
                    .push_bind(") AND state = ", LooseState::Writing.as_str())
                    .push(" UNION SELECT path FROM uploads WHERE path IN (")
                    .bind_list(paths.iter().copied())
                    .push(")");
                if let Some(upload_id) = except_upload {
                    sql.push_bind(" AND id <> ", upload_id);
                }
                for row in scope.fetch_all(sql).await? {
                    held.insert(row.text(0)?);
                }
                Ok(held)
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
                     JOIN workspaces AS t ON t.id = b.workspace_id \
                     JOIN organizations AS o ON o.id = t.org_id WHERE s.board_id = ",
                );
                sql.bind(Id(board_id))
                    .push_bind(" AND s.enabled = ", true)
                    .push(IN_USE)
                    .push(&scope.same_organization(&[("s", "b"), ("b", "t")]));
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

impl Store {
    /// Takes the sign-in under way whose `state` has the digest `state_hash`, if the
    /// browser that comes back with it has the digest `browser_hash` of the one that
    /// started it: it is removed, ended or not, so that it comes back once; whether it
    /// ended (`expires_at`) is the caller's to see. `None` if there is none of that
    /// browser; one of another browser stays. The sign-in names its organization: the
    /// request that comes back from the provider does not.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the statement fails.
    pub async fn take_oidc_login_flow(
        &self,
        state_hash: &str,
        browser_hash: &str,
    ) -> Result<Option<LoginFlow>, StoreError> {
        self.system()
            .with_conn(async |scope| {
                let mut sql = scope.sql("DELETE FROM oidc_login_flows WHERE state_hash = ");
                sql.bind(state_hash)
                    .push_bind(" AND browser_hash = ", browser_hash)
                    .push(
                        " RETURNING state_hash, browser_hash, nonce, pkce_verifier, org_id, \
                     provider_id, create_at, expires_at",
                    );
                let Some(row) = scope.fetch_optional(sql).await? else {
                    return Ok(None);
                };
                flow_from_row(&row).map(Some)
            })
            .await
    }

    /// The organization of the sign-in under way whose `state` has the digest
    /// `state_hash`, if there is one: for the login page a refused return goes back to.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the query fails.
    pub async fn oidc_login_flow_org(
        &self,
        state_hash: &str,
    ) -> Result<Option<String>, StoreError> {
        self.system()
            .with_conn(async |scope| {
                let mut sql = scope.sql("SELECT org_id FROM oidc_login_flows WHERE state_hash = ");
                sql.bind(state_hash);
                scope
                    .fetch_optional(sql)
                    .await?
                    .map(|row| row.id(0))
                    .transpose()
            })
            .await
    }

    /// Removes the sign-ins under way, of every organization, that ended before `now`,
    /// and says how many there were.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the statement fails.
    pub async fn delete_ended_oidc_login_flows(&self, now: i64) -> Result<u64, StoreError> {
        self.system()
            .with_conn(async |scope| {
                let mut sql = scope.sql("DELETE FROM oidc_login_flows WHERE expires_at <= ");
                sql.bind(now);
                scope.execute(sql).await
            })
            .await
    }

    /// What the user is in the organization; `None` for somebody who is no member, and
    /// for an organization that is not in use or is the system's, whether it exists or
    /// not.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the query fails, and [`StoreError::Corrupt`]
    /// if the stored role is none.
    pub async fn org_role_of_user(
        &self,
        user_id: &str,
        org_id: &str,
    ) -> Result<Option<OrgRole>, StoreError> {
        self.system()
            .with_conn(async |scope| {
                let mut sql = scope.sql(
                    "SELECT om.role FROM organization_members AS om \
                     JOIN organizations AS o ON o.id = om.org_id WHERE om.user_id = ",
                );
                sql.bind(Id(user_id))
                    .push_bind(" AND om.org_id = ", Id(org_id))
                    .push(" AND o.delete_at = 0 AND o.is_system = false");
                match scope.fetch_optional(sql).await? {
                    Some(row) => role(&row, 0).map(Some),
                    None => Ok(None),
                }
            })
            .await
    }

    /// What a login page may show of an organization: the one with the identifier, or
    /// with none the only organization in use if there is one only. `None` if there is
    /// no such organization, it is not in use, or it is the system's. Its name, and
    /// whether signing in through its provider is on.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the query fails.
    pub async fn login_organization(
        &self,
        org_id: Option<&str>,
    ) -> Result<Option<LoginOrganization>, StoreError> {
        self.system()
            .with_conn(async |scope| {
                let mut sql = scope.sql(
                    "SELECT o.id, o.name, EXISTS (SELECT 1 FROM oidc_providers AS p \
                     WHERE p.org_id = o.id AND p.enabled = ",
                );
                sql.bind(true).push(
                    ") FROM organizations AS o WHERE o.delete_at = 0 AND o.is_system = false",
                );
                if let Some(org_id) = org_id {
                    sql.push_bind(" AND o.id = ", Id(org_id));
                }
                // Two rows tell that the organization is not the only one.
                sql.push(" ORDER BY o.create_at, o.id LIMIT 2");
                let rows = scope.fetch_all(sql).await?;
                let [row] = rows.as_slice() else {
                    return Ok(None);
                };
                Ok(Some(LoginOrganization {
                    id: row.id(0)?,
                    name: row.text(1)?,
                    oidc: row.bool(2)?,
                }))
            })
            .await
    }
}

async fn create_organization(
    scope: &mut Scope<'_>,
    name: &str,
    workspace: &Workspace,
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
    workspaces::insert_workspace(scope, workspace, Id(&id)).await?;
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
