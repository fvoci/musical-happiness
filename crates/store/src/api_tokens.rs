//! Personal API tokens: an account's credential for an agent.
//!
//! A token is no organization's (`api_tokens`, as `sessions` is not): a request is read
//! for its token before it is known which organization it is for, so the lookup
//! ([`Store::live_api_token`]) reaches no organization's rows. Which workspaces it opens
//! is behind the wall of their organizations (`api_token_workspaces`), and is asked
//! once a workspace is entered, by a handle of the workspace's organization
//! ([`Store::api_token_reach`]).
//!
//! A token ends with what ends its account's sessions, in the same transaction: a
//! password changed, set or reset, the email taken back, an identity unlinked, a
//! provider removed or changed, an account put out of use or erased
//! ([`revoke_of_user`] and its siblings). It stops reaching a workspace its owner
//! leaves, is made a guest of, or that goes to the trash ([`forget_workspace_of_user`],
//! [`forget_workspace`]). The digest of the token is stored, never the token.

use fvoci_model::{ApiToken, ApiTokenScope, ApiTokenWorkspace};

use crate::Store;
use crate::db::{Id, OptId, Row, Scope};
use crate::error::StoreError;
use crate::oidc::StoredProvider;

/// The columns of a token as its list shows it, in the order [`listed_from_row`] reads.
const LISTED_COLUMNS: &str = "t.id, t.user_id, t.name, t.token_prefix, t.scope, \
     t.allow_personal_space, t.notify_self, t.last_used_at, t.last_used_address, \
     t.created_at, t.expires_at";

/// A workspace a new token is to reach, in its organization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GrantedWorkspace<'a> {
    /// The workspace.
    pub workspace_id: &'a str,
    /// Its organization, as the server found it when it let the maker in.
    pub org_id: &'a str,
}

/// The provider the session that makes a token was made through, at its revision then.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TokenOrigin<'a> {
    /// The provider.
    pub provider_id: &'a str,
    /// Its revision when the session was made.
    pub revision: i64,
}

/// A token to be stored ([`Store::issue_api_token`]).
#[derive(Debug, Clone, Copy)]
pub struct TokenToIssue<'a> {
    /// Its identifier.
    pub id: &'a str,
    /// Whose it is.
    pub user_id: &'a str,
    /// Its name, as checked.
    pub name: &'a str,
    /// The characters it is shown by.
    pub token_prefix: &'a str,
    /// The digest of the token.
    pub token_digest: &'a str,
    /// What it may do.
    pub scope: ApiTokenScope,
    /// The workspaces of organizations it reaches.
    pub workspaces: &'a [GrantedWorkspace<'a>],
    /// Whether it reaches the owner's own space.
    pub allow_personal_space: bool,
    /// Whether the owner is told of what it does.
    pub notify_self: bool,
    /// The provider of the session that makes it, if it was made through one.
    pub origin: Option<TokenOrigin<'a>>,
    /// When it is made.
    pub created_at: i64,
    /// When it ends.
    pub expires_at: i64,
}

/// What came of storing a token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Issuance {
    /// Stored, as its list shows it.
    Issued(ApiToken),
    /// Not stored: the maker no longer works in this workspace (left it, was made its
    /// guest, or it went to the trash) since they were let in.
    NoLongerWorksIn(String),
}

/// A token that opens what it names: in use, not past its end, of an account in use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveApiToken {
    /// Its identifier.
    pub id: String,
    /// Whose it is.
    pub user_id: String,
    /// Its name.
    pub name: String,
    /// What it may do.
    pub scope: ApiTokenScope,
    /// Whether it reaches the owner's own space.
    pub allow_personal_space: bool,
    /// Whether the owner is told of what it does.
    pub notify_self: bool,
    /// The provider of the session that made it, if any.
    pub origin_provider_id: Option<String>,
    /// When its use was last recorded.
    pub last_used_at: Option<i64>,
    /// When it ends.
    pub expires_at: i64,
}

/// What a workspace's organization says of a token that names the workspace
/// ([`Store::api_token_reach`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TokenReach {
    /// Whether the organization lets its members give its workspaces to tokens, which
    /// it is asked of only while it takes no session but those made through its
    /// provider.
    pub allow_api_tokens: bool,
}

impl Store {
    /// Stores a token and the workspaces it reaches, each with its organization, in
    /// one transaction that reaches them all: of each, only while `user_id` works in it
    /// (a member who is no guest of a workspace in use), which is asked again here, so
    /// that a maker taken out of a workspace since they were let in does not give it to
    /// a token. A handle of any reach stores it.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::InvalidId`] for an identifier that is none, and
    /// [`StoreError::Database`] if a statement fails, also for a digest another token
    /// is stored under.
    pub async fn issue_api_token(&self, token: &TokenToIssue<'_>) -> Result<Issuance, StoreError> {
        self.system()
            .transaction(async |scope| {
                let mut sql = scope.sql(
                    "INSERT INTO api_tokens (id, user_id, name, token_prefix, token_hash, \
                     scope, allow_personal_space, notify_self, origin_provider_id, \
                     origin_provider_revision, created_at, expires_at) VALUES (",
                );
                sql.bind(Id(token.id))
                    .push_bind(", ", Id(token.user_id))
                    .push_bind(", ", token.name)
                    .push_bind(", ", token.token_prefix)
                    .push_bind(", ", token.token_digest)
                    .push_bind(", ", token.scope.as_str())
                    .push_bind(", ", token.allow_personal_space)
                    .push_bind(", ", token.notify_self)
                    .push_bind(", ", OptId(token.origin.map_or("", |origin| origin.provider_id)))
                    .push_bind(
                        ", ",
                        crate::db::Param::OptInt(token.origin.map(|origin| origin.revision)),
                    )
                    .push_bind(", ", token.created_at)
                    .push_bind(", ", token.expires_at)
                    .push(")");
                scope.execute(sql).await?;
                for granted in token.workspaces {
                    let mut sql = scope.sql(
                        "INSERT INTO api_token_workspaces (org_id, token_id, workspace_id) \
                         SELECT w.org_id, ",
                    );
                    sql.bind(Id(token.id))
                        .push(", w.id FROM workspaces AS w JOIN workspace_members AS wm ON wm.workspace_id = w.id")
                        .push(&scope.same_organization(&[("wm", "w")]))
                        .push_bind(" WHERE w.id = ", Id(granted.workspace_id))
                        .push_bind(" AND w.org_id = ", Id(granted.org_id))
                        .push(" AND w.deleted_at IS NULL AND w.is_system = ")
                        .bind(false)
                        .push_bind(" AND wm.user_id = ", Id(token.user_id))
                        .push(" AND wm.role IN ('admin', 'member')");
                    if scope.execute(sql).await? == 0 {
                        // Out as an error, so that nothing of the token stays.
                        return Err(StoreError::NoLongerWorksIn(granted.workspace_id.to_owned()));
                    }
                }
                listed_of(scope, token.id).await?.ok_or(StoreError::NoRows)
            })
            .await
            .map(Issuance::Issued)
            .or_else(|error| match error {
                StoreError::NoLongerWorksIn(workspace_id) => {
                    Ok(Issuance::NoLongerWorksIn(workspace_id))
                }
                other => Err(other),
            })
    }

    /// The token stored under `token_digest`, if it opens anything at `now`: not taken
    /// back, not past its end, and of an account in use (one put out of use, or waiting
    /// to be deleted, opens nothing through its tokens). Read of no organization.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the query fails.
    pub async fn live_api_token(
        &self,
        token_digest: &str,
        now: i64,
    ) -> Result<Option<LiveApiToken>, StoreError> {
        self.with_conn(async |scope| {
            let mut sql = scope.sql(
                "SELECT t.id, t.user_id, t.name, t.scope, t.allow_personal_space, \
                 t.notify_self, t.origin_provider_id, t.last_used_at, t.expires_at \
                 FROM api_tokens AS t JOIN users AS u ON u.id = t.user_id \
                 WHERE t.token_hash = ",
            );
            sql.bind(token_digest)
                .push(" AND t.revoked_at IS NULL")
                .push_bind(" AND t.expires_at > ", now)
                .push(" AND u.deleted_at IS NULL");
            let Some(row) = scope.fetch_optional(sql).await? else {
                return Ok(None);
            };
            let origin_provider_id = row.id(6)?;
            Ok(Some(LiveApiToken {
                id: row.id(0)?,
                user_id: row.id(1)?,
                name: row.text(2)?,
                scope: scope_of(&row, 3)?,
                allow_personal_space: row.bool(4)?,
                notify_self: row.bool(5)?,
                origin_provider_id: Some(origin_provider_id).filter(|id| !id.is_empty()),
                last_used_at: row.opt_int(7)?,
                expires_at: row.int(8)?,
            }))
        })
        .await
    }

    /// Records that a token was used at `now`, from `address` if it is known. A use
    /// older than what is recorded changes nothing.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the statement fails.
    pub async fn record_api_token_use(
        &self,
        token_id: &str,
        now: i64,
        address: Option<&str>,
    ) -> Result<(), StoreError> {
        self.with_write_conn(async |scope| {
            let mut sql = scope.sql("UPDATE api_tokens SET last_used_at = ");
            sql.bind(now)
                .push_bind(
                    ", last_used_address = ",
                    crate::db::Param::OptText(address.map(str::to_owned)),
                )
                .push_bind(" WHERE id = ", Id(token_id))
                .push_bind(" AND (last_used_at IS NULL OR last_used_at < ", now)
                .push(")");
            scope.execute(sql).await?;
            Ok(())
        })
        .await
    }

    /// Whether the token reaches the workspace, asked of the workspace's organization,
    /// the one this handle names: `Some` with what the organization says of tokens if
    /// the token names the workspace, `None` if it does not.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NoOrganization`] for a handle of none, and
    /// [`StoreError::Database`] if the query fails.
    pub async fn api_token_reach(
        &self,
        token_id: &str,
        workspace_id: &str,
    ) -> Result<Option<TokenReach>, StoreError> {
        self.with_conn(async |scope| {
            let mut sql = scope.sql(
                "SELECT o.allow_api_tokens FROM api_token_workspaces AS tw \
                 JOIN organizations AS o ON o.id = tw.org_id WHERE tw.token_id = ",
            );
            sql.bind(Id(token_id))
                .push_bind(" AND tw.workspace_id = ", Id(workspace_id))
                .push_bind(" AND tw.org_id = ", scope.org()?);
            Ok(match scope.fetch_optional(sql).await? {
                Some(row) => Some(TokenReach {
                    allow_api_tokens: row.bool(0)?,
                }),
                None => None,
            })
        })
        .await
    }

    /// The tokens of an account that are not taken back, the newest first, each with
    /// the workspaces it reaches that are in use, in whichever organization. Those past
    /// their end are among them until they are taken back.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if a query fails.
    pub async fn api_tokens_of_user(&self, user_id: &str) -> Result<Vec<ApiToken>, StoreError> {
        self.system()
            .with_conn(async |scope| {
                let mut sql = scope.sql([
                    "SELECT ",
                    LISTED_COLUMNS,
                    " FROM api_tokens AS t WHERE t.user_id = ",
                ]);
                sql.bind(Id(user_id))
                    .push(" AND t.revoked_at IS NULL ORDER BY t.created_at DESC, t.id");
                let rows = scope.fetch_all(sql).await?;
                let mut tokens: Vec<ApiToken> =
                    rows.iter().map(listed_from_row).collect::<Result<_, _>>()?;
                let mut sql = scope.sql(
                    "SELECT tw.token_id, tw.workspace_id, tw.org_id, w.name \
                     FROM api_token_workspaces AS tw \
                     JOIN api_tokens AS t ON t.id = tw.token_id \
                     JOIN workspaces AS w ON w.id = tw.workspace_id",
                );
                sql.push(&scope.same_organization(&[("tw", "w")]))
                    .push_bind(" WHERE t.user_id = ", Id(user_id))
                    .push(
                        " AND t.revoked_at IS NULL AND w.deleted_at IS NULL ORDER BY w.name, w.id",
                    );
                attach_workspaces(&mut tokens, &scope.fetch_all(sql).await?)?;
                Ok(tokens)
            })
            .await
    }

    /// Takes back a token of `user_id` at `now`: from then on it opens nothing. Whether
    /// there was one to take back.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the statement fails.
    pub async fn revoke_api_token(
        &self,
        user_id: &str,
        token_id: &str,
        now: i64,
    ) -> Result<bool, StoreError> {
        self.with_write_conn(async |scope| {
            let mut sql = scope.sql("UPDATE api_tokens SET revoked_at = ");
            sql.bind(now)
                .push_bind(" WHERE id = ", Id(token_id))
                .push_bind(" AND user_id = ", Id(user_id))
                .push(" AND revoked_at IS NULL");
            Ok(scope.execute(sql).await? > 0)
        })
        .await
    }

    /// The tokens that reach a workspace of the handle's organization and open anything
    /// at `now`, the newest first, each with those of its workspaces that are of this
    /// organization: what its administrators see of their members' tokens.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NoOrganization`] for a handle of none, and
    /// [`StoreError::Database`] if a query fails.
    pub async fn api_tokens_of_organization(&self, now: i64) -> Result<Vec<ApiToken>, StoreError> {
        self.with_conn(async |scope| {
            let org = scope.org()?;
            let mut sql = scope.sql(["SELECT ", LISTED_COLUMNS, " FROM api_tokens AS t WHERE "]);
            sql.push("EXISTS (SELECT 1 FROM api_token_workspaces AS tw WHERE tw.token_id = t.id")
                .push_bind(" AND tw.org_id = ", org.clone())
                .push(") AND t.revoked_at IS NULL")
                .push_bind(" AND t.expires_at > ", now)
                .push(" ORDER BY t.created_at DESC, t.id");
            let rows = scope.fetch_all(sql).await?;
            let mut tokens: Vec<ApiToken> =
                rows.iter().map(listed_from_row).collect::<Result<_, _>>()?;
            let mut sql = scope.sql(
                "SELECT tw.token_id, tw.workspace_id, tw.org_id, w.name \
                 FROM api_token_workspaces AS tw JOIN workspaces AS w ON w.id = tw.workspace_id",
            );
            sql.push(&scope.same_organization(&[("tw", "w")]))
                .push_bind(" WHERE tw.org_id = ", org)
                .push(" AND w.deleted_at IS NULL ORDER BY w.name, w.id");
            attach_workspaces(&mut tokens, &scope.fetch_all(sql).await?)?;
            Ok(tokens)
        })
        .await
    }

    /// Takes back at `now` a token that reaches a workspace of the handle's
    /// organization, for an administrator of it: whoever's it is, and in every
    /// organization it reaches. Whether there was one to take back.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NoOrganization`] for a handle of none, and
    /// [`StoreError::Database`] if the statement fails.
    pub async fn revoke_api_token_of_organization(
        &self,
        token_id: &str,
        now: i64,
    ) -> Result<bool, StoreError> {
        self.with_write_conn(async |scope| {
            let mut sql = scope.sql("UPDATE api_tokens SET revoked_at = ");
            sql.bind(now)
                .push_bind(" WHERE id = ", Id(token_id))
                .push(
                    " AND revoked_at IS NULL AND EXISTS (SELECT 1 FROM api_token_workspaces AS tw \
                       WHERE tw.token_id = api_tokens.id",
                )
                .push_bind(" AND tw.org_id = ", scope.org()?)
                .push(")");
            Ok(scope.execute(sql).await? > 0)
        })
        .await
    }
}

/// Takes back every token of an account: with its sessions, in their transaction.
pub(crate) async fn revoke_of_user(scope: &mut Scope<'_>, user_id: &str) -> Result<(), StoreError> {
    let mut sql = scope.sql("UPDATE api_tokens SET revoked_at = ");
    sql.bind(scope.now_millis())
        .push_bind(" WHERE user_id = ", Id(user_id))
        .push(" AND revoked_at IS NULL");
    scope.execute(sql).await?;
    Ok(())
}

/// Takes back the tokens of an account that were made by a session made through a
/// provider: with the sessions of an identity it unlinks.
pub(crate) async fn revoke_of_user_through_providers(
    scope: &mut Scope<'_>,
    user_id: &str,
) -> Result<(), StoreError> {
    let mut sql = scope.sql("UPDATE api_tokens SET revoked_at = ");
    sql.bind(scope.now_millis())
        .push_bind(" WHERE user_id = ", Id(user_id))
        .push(" AND origin_provider_id IS NOT NULL AND revoked_at IS NULL");
    scope.execute(sql).await?;
    Ok(())
}

/// Takes back the tokens made through the provider `before` that its change to `after`
/// ends: every one where it is removed (`after` is `None`), and those of another
/// revision where it signs in otherwise. A token proves what the session that made it
/// proved, the revision it was made at; a change that replaced the row gave the
/// provider another identifier, so the tokens are found by the one before.
pub(crate) async fn follow_provider(
    scope: &mut Scope<'_>,
    before: Option<&StoredProvider>,
    after: Option<&StoredProvider>,
) -> Result<(), StoreError> {
    let Some(before) = before else {
        return Ok(());
    };
    match after {
        None => revoke_of_provider(scope, &before.id, None).await,
        Some(after) if after.revision != before.revision => {
            revoke_of_provider(scope, &before.id, Some(after.revision)).await
        }
        Some(_) => Ok(()),
    }
}

/// Takes back the tokens made by sessions made through `provider_id`, but those of
/// `keeping_revision` if one is given: with the provider removed, given another issuer,
/// or given a revision that signs in otherwise.
async fn revoke_of_provider(
    scope: &mut Scope<'_>,
    provider_id: &str,
    keeping_revision: Option<i64>,
) -> Result<(), StoreError> {
    let mut sql = scope.sql("UPDATE api_tokens SET revoked_at = ");
    sql.bind(scope.now_millis())
        .push_bind(" WHERE origin_provider_id = ", Id(provider_id))
        .push(" AND revoked_at IS NULL");
    if let Some(revision) = keeping_revision {
        sql.push_bind(" AND origin_provider_revision <> ", revision);
    }
    scope.execute(sql).await?;
    Ok(())
}

/// Takes back the tokens made through a provider of the scope's organization: with the
/// organization, as it is erased.
pub(crate) async fn revoke_of_organization_providers(
    scope: &mut Scope<'_>,
) -> Result<(), StoreError> {
    let mut sql = scope.sql("UPDATE api_tokens SET revoked_at = ");
    sql.bind(scope.now_millis())
        .push(" WHERE revoked_at IS NULL AND origin_provider_id IN (SELECT id FROM oidc_providers WHERE org_id = ")
        .bind(scope.org()?)
        .push(")");
    scope.execute(sql).await?;
    Ok(())
}

/// Forgets that the tokens of `user_id` reach a workspace of the scope's organization:
/// they leave it, or are made its guest.
pub(crate) async fn forget_workspace_of_user(
    scope: &mut Scope<'_>,
    workspace_id: &str,
    user_id: &str,
) -> Result<(), StoreError> {
    let mut sql = scope.sql("DELETE FROM api_token_workspaces WHERE workspace_id = ");
    sql.bind(Id(workspace_id))
        .push_bind(" AND org_id = ", scope.org()?)
        .push(" AND token_id IN (SELECT id FROM api_tokens WHERE user_id = ")
        .bind(Id(user_id))
        .push(")");
    scope.execute(sql).await?;
    Ok(())
}

/// Forgets that any token reaches a workspace of the scope's organization: it goes to
/// the trash, and coming back from it gives it to no token again.
pub(crate) async fn forget_workspace(
    scope: &mut Scope<'_>,
    workspace_id: &str,
) -> Result<(), StoreError> {
    let mut sql = scope.sql("DELETE FROM api_token_workspaces WHERE workspace_id = ");
    sql.bind(Id(workspace_id))
        .push_bind(" AND org_id = ", scope.org()?);
    scope.execute(sql).await?;
    Ok(())
}

/// Forgets that the tokens of `user_id` reach any workspace of the scope's
/// organization: they leave it.
pub(crate) async fn forget_organization_of_user(
    scope: &mut Scope<'_>,
    user_id: &str,
) -> Result<(), StoreError> {
    let mut sql = scope.sql("DELETE FROM api_token_workspaces WHERE org_id = ");
    sql.bind(scope.org()?)
        .push(" AND token_id IN (SELECT id FROM api_tokens WHERE user_id = ")
        .bind(Id(user_id))
        .push(")");
    scope.execute(sql).await?;
    Ok(())
}

/// A token as its list shows it, by its identifier, with the workspaces it reaches:
/// for the answer to its making, read in its transaction.
async fn listed_of(scope: &mut Scope<'_>, token_id: &str) -> Result<Option<ApiToken>, StoreError> {
    let mut sql = scope.sql([
        "SELECT ",
        LISTED_COLUMNS,
        " FROM api_tokens AS t WHERE t.id = ",
    ]);
    sql.bind(Id(token_id));
    let Some(row) = scope.fetch_optional(sql).await? else {
        return Ok(None);
    };
    let mut tokens = vec![listed_from_row(&row)?];
    let mut sql = scope.sql(
        "SELECT tw.token_id, tw.workspace_id, tw.org_id, w.name \
         FROM api_token_workspaces AS tw JOIN workspaces AS w ON w.id = tw.workspace_id",
    );
    sql.push(&scope.same_organization(&[("tw", "w")]))
        .push_bind(" WHERE tw.token_id = ", Id(token_id))
        .push(" ORDER BY w.name, w.id");
    attach_workspaces(&mut tokens, &scope.fetch_all(sql).await?)?;
    Ok(tokens.pop())
}

/// Reads a token of [`LISTED_COLUMNS`], with no workspace yet.
fn listed_from_row(row: &Row) -> Result<ApiToken, StoreError> {
    Ok(ApiToken {
        id: row.id(0)?,
        user_id: row.id(1)?,
        name: row.text(2)?,
        token_prefix: row.text(3)?,
        scope: scope_of(row, 4)?,
        workspaces: Vec::new(),
        allow_personal_space: row.bool(5)?,
        notify_self: row.bool(6)?,
        last_used_at: row.opt_int(7)?,
        last_used_address: row.opt_text(8)?,
        created_at: row.int(9)?,
        expires_at: row.int(10)?,
    })
}

/// Puts rows of `(token_id, workspace_id, org_id, name)` with their tokens.
fn attach_workspaces(tokens: &mut [ApiToken], rows: &[Row]) -> Result<(), StoreError> {
    for row in rows {
        let token_id = row.id(0)?;
        if let Some(token) = tokens.iter_mut().find(|token| token.id == token_id) {
            token.workspaces.push(ApiTokenWorkspace {
                workspace_id: row.id(1)?,
                org_id: row.id(2)?,
                name: row.text(3)?,
            });
        }
    }
    Ok(())
}

fn scope_of(row: &Row, index: usize) -> Result<ApiTokenScope, StoreError> {
    let text = row.text(index)?;
    text.parse().map_err(|_| StoreError::Corrupt {
        column: "api_tokens.scope",
        reason: format!("{text:?} is no scope"),
    })
}
