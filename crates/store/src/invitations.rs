//! Invitations: the links that let somebody into a workspace, as a member of it or as a
//! guest of one of its boards.
//!
//! A link is found by the digest of its token; the token itself is never here. Every
//! statement names the organization itself: the handle's
//! ([`Tenant::Org`](crate::Tenant::Org)). Which organization a token leads into is asked
//! in `directory.rs`.
//!
//! A link lets somebody in while it is live: not revoked, not ended, and not used as
//! often as it may be. Whether who made it may still invite is the server's to ask,
//! of what the store answers about that user as about any other.

use fvoci_model::{BoardMember, BoardRole, Invitation, OrgRole, User, WorkspaceRole, new_id};

use crate::Store;
use crate::accounts::{self, Which};
use crate::boards::{find_member, save_member};
use crate::db::{Dialect, Id, OptId, Param, Row, Scope, Sql};
use crate::error::StoreError;
use crate::users::InstanceRole;

/// The columns of an invitation, in the order [`invitation_from_row`] reads them.
const COLUMNS: &str = "id, workspace_id, role, board_id, board_role, created_by, create_at, \
                       expires_at, max_uses, uses";

/// A link to store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NewLink<'a> {
    /// The workspace the link leads into.
    pub workspace_id: &'a str,
    /// For a guest: the board they are invited to, and their role on it. `None` for a
    /// link that makes a member of the workspace.
    pub guest_of: Option<(&'a str, BoardRole)>,
    /// The digest of the link's token.
    pub token_hash: &'a str,
    /// The user who makes the link.
    pub created_by: &'a str,
    /// When the link is made, in milliseconds since the Unix epoch.
    pub now: i64,
    /// When the link ends, in milliseconds since the Unix epoch.
    pub expires_at: i64,
    /// How often the link may be used; `None` for as often as it is until it ends.
    pub max_uses: Option<i64>,
}

/// Who accepts a link.
#[derive(Debug)]
pub enum Acceptor<'a> {
    /// The account with this identifier.
    Account(&'a str),
    /// A new account, stored with the acceptance and updated to what was stored.
    NewAccount(&'a mut User),
}

/// Where an accepted link let somebody in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Accepted {
    /// The account that was let in.
    pub user_id: String,
    /// The workspace.
    pub workspace_id: String,
    /// For a guest's link: the board.
    pub board_id: Option<String>,
    /// Whether the acceptance changed anything, and so counts as a use of the link:
    /// not if the account had already all the link gives.
    pub used: bool,
}

impl Store {
    /// Stores a link into a workspace in use of the handle's organization, and returns
    /// it as stored. A guest's link names a board of that workspace that is no template.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NotFound`] if the organization has no such workspace in
    /// use, or the workspace no such board; [`StoreError::NoOrganization`] if the
    /// handle names none; and [`StoreError::Database`] if the statement fails.
    pub async fn create_invitation(&self, link: &NewLink<'_>) -> Result<Invitation, StoreError> {
        self.with_conn(async |scope| {
            let (role, board_id, board_role) = match link.guest_of {
                Some((board_id, board_role)) => {
                    (WorkspaceRole::Guest, board_id, Some(board_role.as_str()))
                }
                None => (WorkspaceRole::Member, "", None),
            };
            let mut sql = scope.sql(
                "INSERT INTO invitations (id, org_id, workspace_id, role, board_id, board_role, \
                 token_hash, created_by, create_at, expires_at, max_uses) SELECT ",
            );
            sql.bind(Id(&new_id()))
                .push(", t.org_id, t.id")
                .push_bind(", ", role.as_str())
                .push_bind(", ", OptId(board_id))
                .push_bind(", ", Param::OptText(board_role.map(str::to_owned)))
                .push_bind(", ", link.token_hash)
                .push_bind(", ", Id(link.created_by))
                .push_bind(", ", link.now)
                .push_bind(", ", link.expires_at)
                .push_bind(", ", Param::OptInt(link.max_uses))
                .push_bind(" FROM workspaces AS t WHERE t.id = ", Id(link.workspace_id))
                .push_bind(" AND t.org_id = ", scope.org()?)
                .push_bind(" AND t.delete_at = ", 0_i64);
            if link.guest_of.is_some() {
                sql.push_bind(
                    " AND EXISTS (SELECT 1 FROM boards AS b WHERE b.id = ",
                    Id(board_id),
                )
                .push_bind(" AND b.workspace_id = t.id AND b.is_template = ", false)
                .push(")");
            }
            sql.push(&format!(" RETURNING {COLUMNS}"));
            match scope.fetch_optional(sql).await? {
                Some(row) => invitation_from_row(&row),
                None => Err(StoreError::not_found("workspace or board")),
            }
        })
        .await
    }

    /// The live link of the handle's organization whose token has the digest
    /// `token_hash`, if there is one at `now`.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NoOrganization`] if the handle names none, and
    /// [`StoreError::Database`] if the query fails.
    pub async fn live_invitation(
        &self,
        token_hash: &str,
        now: i64,
    ) -> Result<Option<Invitation>, StoreError> {
        self.with_conn(async |scope| {
            let sql = select_live_by_token(scope, token_hash, now)?;
            scope
                .fetch_optional(sql)
                .await?
                .as_ref()
                .map(invitation_from_row)
                .transpose()
        })
        .await
    }

    /// The live links into a workspace of the handle's organization at `now`, the
    /// newest first: all of them, or those `created_by` made.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NoOrganization`] if the handle names none, and
    /// [`StoreError::Database`] if the query fails.
    pub async fn live_invitations(
        &self,
        workspace_id: &str,
        created_by: Option<&str>,
        now: i64,
    ) -> Result<Vec<Invitation>, StoreError> {
        self.with_conn(async |scope| {
            let mut sql = select_live(scope, now)?;
            sql.push_bind(" AND i.workspace_id = ", Id(workspace_id));
            if let Some(user_id) = created_by {
                sql.push_bind(" AND i.created_by = ", Id(user_id));
            }
            sql.push(" ORDER BY i.create_at DESC, i.id DESC");
            scope
                .fetch_all(sql)
                .await?
                .iter()
                .map(invitation_from_row)
                .collect()
        })
        .await
    }

    /// Revokes a live link into a workspace of the handle's organization, any of them
    /// or one `created_by` made; says whether there was such a link. A revoked link
    /// lets nobody in from then on.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NoOrganization`] if the handle names none, and
    /// [`StoreError::Database`] if the statement fails.
    pub async fn revoke_invitation(
        &self,
        workspace_id: &str,
        invitation_id: &str,
        created_by: Option<&str>,
        now: i64,
    ) -> Result<bool, StoreError> {
        self.with_conn(async |scope| {
            let mut sql = scope.sql("UPDATE invitations AS i SET revoked_at = ");
            sql.bind(now.max(1))
                .push_bind(" WHERE i.id = ", Id(invitation_id))
                .push_bind(" AND i.workspace_id = ", Id(workspace_id))
                .push_bind(" AND i.org_id = ", scope.org()?);
            push_live(&mut sql, now);
            if let Some(user_id) = created_by {
                sql.push_bind(" AND i.created_by = ", Id(user_id));
            }
            Ok(scope.execute(sql).await? > 0)
        })
        .await
    }

    /// Lets `acceptor` in by the live link of the handle's organization whose token
    /// has the digest `token_hash`, in one transaction, and counts the use: `None` if
    /// there is no such link at `now`, and nothing is stored then. Acceptances at once
    /// of one link wait for each other, so that it is used no more often than it may be.
    ///
    /// - A member's link makes the account a member of the organization and of the
    ///   workspace: who was a guest of the workspace becomes a member, and who works
    ///   there stays what they are.
    /// - A guest's link makes the account a member of its board, in its role: who was
    ///   no member of the workspace becomes a guest of it, and holds no other role on
    ///   its boards; who was one stays what they are in it, and keeps a higher role
    ///   they have on the board.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Conflict`] if a new account's name or email address is
    /// taken; [`StoreError::NoOrganization`] if the handle names none; and
    /// [`StoreError::Database`] if a statement fails, as it does for an account that
    /// does not exist. Nothing is stored then.
    pub async fn accept_invitation(
        &self,
        token_hash: &str,
        acceptor: Acceptor<'_>,
        now: i64,
    ) -> Result<Option<Accepted>, StoreError> {
        self.transaction(async |scope| {
            let mut sql = select_live_by_token(scope, token_hash, now)?;
            if scope.dialect() == Dialect::Postgres {
                sql.push(" FOR UPDATE OF i");
            }
            let Some(row) = scope.fetch_optional(sql).await? else {
                return Ok(None);
            };
            let link = invitation_from_row(&row)?;
            let user_id = match acceptor {
                Acceptor::Account(user_id) => user_id.to_owned(),
                Acceptor::NewAccount(user) => {
                    accounts::insert_account(scope, user, InstanceRole::User).await?;
                    user.id.clone()
                }
            };
            let used = match (&link.board_id, link.board_role) {
                (Some(board_id), Some(board_role)) => {
                    let invited = BoardMember {
                        board_id: board_id.clone(),
                        user_id: user_id.clone(),
                        ..BoardMember::default()
                    }
                    .with_role(Some(board_role));
                    let_in_as_guest(scope, &link.workspace_id, invited).await?
                }
                _ => let_in_as_member(scope, &link.workspace_id, &user_id).await?,
            };
            if used {
                let mut sql = scope.sql("UPDATE invitations SET uses = uses + 1 WHERE id = ");
                sql.bind(Id(&link.id))
                    .push_bind(" AND org_id = ", scope.org()?);
                scope.execute(sql).await?;
            }
            Ok(Some(Accepted {
                user_id,
                workspace_id: link.workspace_id,
                board_id: link.board_id,
                used,
            }))
        })
        .await
    }
}

/// Makes the user a member of the scope's organization and of the workspace; says
/// whether anything changed. A guest of the workspace becomes a member of it.
async fn let_in_as_member(
    scope: &mut Scope<'_>,
    workspace_id: &str,
    user_id: &str,
) -> Result<bool, StoreError> {
    let joined_org = accounts::join_organization(scope, user_id, OrgRole::Member).await?;
    let joined = accounts::join_workspace(
        scope,
        Which::Workspace(workspace_id),
        user_id,
        WorkspaceRole::Member,
    )
    .await?;
    let mut sql = scope.sql("UPDATE workspace_members SET role = ");
    sql.bind(WorkspaceRole::Member.as_str())
        .push_bind(" WHERE workspace_id = ", Id(workspace_id))
        .push_bind(" AND user_id = ", Id(user_id))
        .push_bind(" AND role = ", WorkspaceRole::Guest.as_str());
    let raised = scope.execute(sql).await? > 0;
    Ok(joined_org || joined || raised)
}

/// Makes the user of `invited` a member of its board, in its role or the higher one
/// they have there, and a guest of the workspace if they are no member of it; says
/// whether anything changed.
async fn let_in_as_guest(
    scope: &mut Scope<'_>,
    workspace_id: &str,
    invited: BoardMember,
) -> Result<bool, StoreError> {
    let user_id = &invited.user_id;
    // Who comes into the workspace as a guest is on the board of the link and on no
    // other: a role left on a board of it from an earlier time there opens nothing
    // while they are outside, and is not to open anything again by this.
    let mut sql = scope.sql("SELECT 1 FROM workspace_members WHERE workspace_id = ");
    sql.bind(Id(workspace_id))
        .push_bind(" AND user_id = ", Id(user_id));
    if scope.fetch_optional(sql).await?.is_none() {
        let mut sql = scope.sql("DELETE FROM board_members WHERE user_id = ");
        sql.bind(Id(user_id))
            .push_bind(
                " AND board_id IN (SELECT id FROM boards WHERE workspace_id = ",
                Id(workspace_id),
            )
            .push(")");
        scope.execute(sql).await?;
    }
    let joined = accounts::join_workspace(
        scope,
        Which::Workspace(workspace_id),
        user_id,
        WorkspaceRole::Guest,
    )
    .await?;
    let held = find_member(scope, &invited.board_id, user_id)
        .await?
        .and_then(|member| member.role());
    if held >= invited.role() {
        return Ok(joined);
    }
    save_member(scope, &invited).await?;
    Ok(true)
}

/// Starts the query for the live links of the scope's organization at `now`;
/// conditions follow with `AND`.
fn select_live(scope: &Scope<'_>, now: i64) -> Result<Sql, StoreError> {
    let columns = COLUMNS
        .split(", ")
        .map(|column| format!("i.{}", column.trim()))
        .collect::<Vec<_>>()
        .join(", ");
    let mut sql = scope.sql(&format!(
        "SELECT {columns} FROM invitations AS i WHERE i.org_id = "
    ));
    sql.bind(scope.org()?);
    push_live(&mut sql, now);
    Ok(sql)
}

/// The query for the live link of the scope's organization with the token's digest.
fn select_live_by_token(scope: &Scope<'_>, token_hash: &str, now: i64) -> Result<Sql, StoreError> {
    let mut sql = select_live(scope, now)?;
    sql.push_bind(" AND i.token_hash = ", token_hash);
    Ok(sql)
}

/// Pushes the conditions that the link `i` is live at `now`: not revoked, not ended,
/// and not used as often as it may be.
pub(crate) fn push_live(sql: &mut Sql, now: i64) {
    sql.push_bind(" AND i.revoked_at = ", 0_i64)
        .push_bind(" AND i.expires_at > ", now)
        .push(" AND (i.max_uses IS NULL OR i.uses < i.max_uses)");
}

fn invitation_from_row(row: &Row) -> Result<Invitation, StoreError> {
    let corrupt = |error: fvoci_model::UnknownRole| StoreError::Corrupt {
        column: "role",
        reason: error.to_string(),
    };
    let board_id = row.id(3)?;
    let board_role = row.text(4)?;
    let max_uses = row.int(8)?;
    Ok(Invitation {
        id: row.id(0)?,
        workspace_id: row.id(1)?,
        role: row.text(2)?.parse().map_err(corrupt)?,
        board_id: (!board_id.is_empty()).then_some(board_id),
        board_role: if board_role.is_empty() {
            None
        } else {
            Some(board_role.parse().map_err(corrupt)?)
        },
        created_by: row.id(5)?,
        create_at: row.int(6)?,
        expires_at: row.int(7)?,
        // No link may be used zero times: a column without a number is one without
        // such an end.
        max_uses: (max_uses > 0).then_some(max_uses),
        uses: row.int(9)?,
        token: None,
    })
}
