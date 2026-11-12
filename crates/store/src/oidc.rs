//! Signing in through an organization's `OpenID Connect` provider: the provider, the
//! sign-ins under way, and who a provider vouches for.
//!
//! The provider and a sign-in that is started are the rows of the handle's organization
//! ([`Tenant::Org`](crate::Tenant::Org)). An identity, like the account it belongs to,
//! is nobody's. What is asked before the organization is known (which sign-in comes
//! back, which organization a login page is for) is in `directory.rs`.

use fvoci_model::{User, new_id};

use crate::Store;
use crate::db::{Id, Row, Scope, now_millis};
use crate::error::StoreError;
use crate::members;
use crate::users::{InstanceRole, insert_user_row};

/// The columns of a provider, in the order [`provider_from_row`] reads them.
const PROVIDER_COLUMNS: &str =
    "id, org_id, issuer, client_id, client_secret, auto_join, enabled, create_at, update_at";

/// The provider of an organization, as it is stored.
#[derive(Clone, PartialEq, Eq)]
pub struct StoredProvider {
    /// The provider's identifier, which a session made through it names.
    pub id: String,
    /// The organization whose provider it is.
    pub org_id: String,
    /// The issuer, exactly as the provider names itself.
    pub issuer: String,
    /// The client identifier.
    pub client_id: String,
    /// The client secret as the server sealed it; empty for none. The store never
    /// sees the secret itself.
    pub sealed_client_secret: String,
    /// Whether somebody it vouches for who is no member becomes one by signing in.
    pub auto_join: bool,
    /// Whether signing in through it is on.
    pub enabled: bool,
    /// When it was first set, in milliseconds since the Unix epoch.
    pub create_at: i64,
    /// When it was last changed, in milliseconds since the Unix epoch.
    pub update_at: i64,
}

impl std::fmt::Debug for StoredProvider {
    /// Leaves out the sealed secret.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StoredProvider")
            .field("id", &self.id)
            .field("org_id", &self.org_id)
            .field("issuer", &self.issuer)
            .field("client_id", &self.client_id)
            .field("auto_join", &self.auto_join)
            .field("enabled", &self.enabled)
            .finish_non_exhaustive()
    }
}

/// A sign-in through a provider that was started and has not come back.
#[derive(Clone, PartialEq, Eq)]
pub struct LoginFlow {
    /// The digest of the `state` sent to the provider, by which it is found.
    pub state_hash: String,
    /// The digest of the secret in the cookie of the browser that started it.
    pub browser_hash: String,
    /// The `nonce` the ID token must carry.
    pub nonce: String,
    /// The PKCE code verifier.
    pub pkce_verifier: String,
    /// The organization whose provider it goes through.
    pub org_id: String,
    /// The provider it goes through.
    pub provider_id: String,
    /// When it was started, in milliseconds since the Unix epoch.
    pub create_at: i64,
    /// When it can no longer come back, in milliseconds since the Unix epoch.
    pub expires_at: i64,
}

impl std::fmt::Debug for LoginFlow {
    /// Leaves out the nonce and the verifier.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoginFlow")
            .field("org_id", &self.org_id)
            .field("provider_id", &self.provider_id)
            .field("expires_at", &self.expires_at)
            .finish_non_exhaustive()
    }
}

/// The identity a provider vouches for: who the subject is at the issuer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Identity<'a> {
    /// The issuer, exactly as in the ID token.
    pub issuer: &'a str,
    /// The subject, exactly as in the ID token.
    pub subject: &'a str,
}

impl Store {
    /// The provider of the handle's organization, if it has one.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NoOrganization`] if the handle names none, and
    /// [`StoreError::Database`] if the query fails.
    pub async fn oidc_provider(&self) -> Result<Option<StoredProvider>, StoreError> {
        self.with_conn(async |scope| {
            let mut sql = scope.sql(&format!(
                "SELECT {PROVIDER_COLUMNS} FROM oidc_providers WHERE org_id = "
            ));
            sql.bind(scope.org()?);
            scope
                .fetch_optional(sql)
                .await?
                .as_ref()
                .map(provider_from_row)
                .transpose()
        })
        .await
    }

    /// Stores the provider of the handle's organization, in place of the one it has,
    /// and returns it as stored. A provider whose issuer and client stay keeps its
    /// identifier and the time it was first set; one with another issuer or client is
    /// another provider, under a new identifier, and the sign-ins under way through the
    /// one before end with it. `provider.id`, `org_id` and the times are the store's.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NoOrganization`] if the handle names none, and
    /// [`StoreError::Database`] if a statement fails.
    pub async fn put_oidc_provider(
        &self,
        provider: &StoredProvider,
    ) -> Result<StoredProvider, StoreError> {
        self.transaction(async |scope| {
            let now = now_millis();
            let mut sql = scope.sql("DELETE FROM oidc_providers WHERE org_id = ");
            sql.bind(scope.org()?)
                .push_bind(" AND (issuer <> ", &provider.issuer)
                .push_bind(" OR client_id <> ", &provider.client_id)
                .push(")");
            scope.execute(sql).await?;
            let mut sql = scope.sql(&format!(
                "INSERT INTO oidc_providers ({PROVIDER_COLUMNS}) VALUES ("
            ));
            sql.bind(Id(&new_id()))
                .push_bind(", ", scope.org()?)
                .push_bind(", ", &provider.issuer)
                .push_bind(", ", &provider.client_id)
                .push_bind(", ", &provider.sealed_client_secret)
                .push_bind(", ", provider.auto_join)
                .push_bind(", ", provider.enabled)
                .push_bind(", ", now)
                .push_bind(", ", now)
                .push(
                    ") ON CONFLICT (org_id) DO UPDATE SET \
                     client_secret = excluded.client_secret, \
                     auto_join = excluded.auto_join, enabled = excluded.enabled, \
                     update_at = excluded.update_at",
                )
                .push(&format!(" RETURNING {PROVIDER_COLUMNS}"));
            let row = scope.fetch_optional(sql).await?.ok_or(StoreError::NoRows)?;
            provider_from_row(&row)
        })
        .await
    }

    /// Removes the provider of the handle's organization, and the sign-ins under way
    /// through it. Says whether there was one.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NoOrganization`] if the handle names none, and
    /// [`StoreError::Database`] if the statement fails.
    pub async fn delete_oidc_provider(&self) -> Result<bool, StoreError> {
        self.with_conn(async |scope| {
            let mut sql = scope.sql("DELETE FROM oidc_providers WHERE org_id = ");
            sql.bind(scope.org()?);
            Ok(scope.execute(sql).await? > 0)
        })
        .await
    }

    /// Stores a sign-in that is started through the handle's organization's provider.
    /// `flow.org_id` is the handle's.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NoOrganization`] if the handle names none, and
    /// [`StoreError::Database`] if the statement fails, as it does for a provider that
    /// is not the organization's.
    pub async fn create_oidc_login_flow(&self, flow: &LoginFlow) -> Result<(), StoreError> {
        self.with_conn(async |scope| {
            let mut sql = scope.sql(
                "INSERT INTO oidc_login_flows (state_hash, browser_hash, nonce, pkce_verifier, \
                 org_id, provider_id, create_at, expires_at) SELECT ",
            );
            sql.bind(&flow.state_hash)
                .push_bind(", ", &flow.browser_hash)
                .push_bind(", ", &flow.nonce)
                .push_bind(", ", &flow.pkce_verifier)
                .push(", org_id, id")
                .push_bind(", ", flow.create_at)
                .push_bind(", ", flow.expires_at)
                .push_bind(" FROM oidc_providers WHERE id = ", Id(&flow.provider_id))
                .push_bind(" AND org_id = ", scope.org()?);
            if scope.execute(sql).await? == 0 {
                return Err(StoreError::not_found("oidc provider"));
            }
            Ok(())
        })
        .await
    }

    /// The account a provider vouches for under `identity`, if there is one. A
    /// deactivated account is none.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the query fails.
    pub async fn user_of_identity(
        &self,
        identity: Identity<'_>,
    ) -> Result<Option<User>, StoreError> {
        self.with_conn(async |scope| {
            let mut sql =
                scope.sql("SELECT user_id FROM user_identities AS i JOIN users AS u ON u.id = i.user_id WHERE i.issuer = ");
            sql.bind(identity.issuer)
                .push_bind(" AND i.subject = ", identity.subject)
                .push_bind(" AND u.delete_at = ", 0_i64);
            let Some(row) = scope.fetch_optional(sql).await? else {
                return Ok(None);
            };
            let user_id = row.id(0)?;
            crate::users::get_user_by_id(scope, &user_id).await.map(Some)
        })
        .await
    }

    /// Stores a new account that a provider of the handle's organization vouches for
    /// under `identity`, with no password: an empty hash, which no password matches. In
    /// the same transaction the account joins the organization and its default
    /// workspace, as a member, if `join` is [`Join::Organization`].
    ///
    /// `user` is updated to what was stored: the current time as creation and
    /// modification time, not deleted, no administrator of the instance, no password.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Conflict`] if another account has the name or the email
    /// address in any letter case, or the identity is another account's already;
    /// [`StoreError::NoOrganization`] if the account is to join and the handle names no
    /// organization; and [`StoreError::Database`] if a statement fails. Nothing is
    /// stored then.
    pub async fn create_oidc_user(
        &self,
        user: &mut User,
        identity: Identity<'_>,
        join: Join,
    ) -> Result<(), StoreError> {
        self.transaction(async |scope| {
            let now = now_millis();
            user.create_at = now;
            user.update_at = now;
            user.delete_at = 0;
            user.is_admin = false;
            user.password_hash = String::new();
            insert_user_row(scope, user).await?;
            insert_identity(scope, identity, &user.id, now).await?;
            if join == Join::Organization {
                members::join_organization(scope, &user.id, InstanceRole::User).await?;
            }
            Ok(())
        })
        .await
    }

    /// Makes a user a member of the handle's organization and of its default
    /// workspace, as a member, unless the user is a member of the organization already.
    /// Says whether the user joined.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NoOrganization`] if the handle names none, and
    /// [`StoreError::Database`] if a statement fails.
    pub async fn join_organization_through_oidc(&self, user_id: &str) -> Result<bool, StoreError> {
        self.transaction(async |scope| {
            let mut sql = scope.sql("SELECT 1 FROM organization_members WHERE org_id = ");
            sql.bind(scope.org()?)
                .push_bind(" AND user_id = ", Id(user_id));
            if scope.fetch_optional(sql).await?.is_some() {
                return Ok(false);
            }
            members::join_organization(scope, user_id, InstanceRole::User).await?;
            Ok(true)
        })
        .await
    }

    /// Whether the user is a member of the handle's organization.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NoOrganization`] if the handle names none, and
    /// [`StoreError::Database`] if the query fails.
    pub async fn is_organization_member(&self, user_id: &str) -> Result<bool, StoreError> {
        self.with_conn(async |scope| {
            let mut sql = scope.sql("SELECT 1 FROM organization_members WHERE org_id = ");
            sql.bind(scope.org()?)
                .push_bind(" AND user_id = ", Id(user_id));
            Ok(scope.fetch_optional(sql).await?.is_some())
        })
        .await
    }
}

/// Whether an account that is made joins the organization of the handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Join {
    /// It joins the organization and its default workspace.
    Organization,
    /// It is made alone.
    Nothing,
}

async fn insert_identity(
    scope: &mut Scope<'_>,
    identity: Identity<'_>,
    user_id: &str,
    now: i64,
) -> Result<(), StoreError> {
    let mut sql =
        scope.sql("INSERT INTO user_identities (issuer, subject, user_id, create_at) VALUES (");
    sql.bind(identity.issuer)
        .push_bind(", ", identity.subject)
        .push_bind(", ", Id(user_id))
        .push_bind(", ", now)
        .push(")");
    match scope.execute(sql).await {
        Ok(_) => Ok(()),
        Err(StoreError::Database(error)) => Err(crate::users::unique_violation(error)),
        Err(error) => Err(error),
    }
}

fn provider_from_row(row: &Row) -> Result<StoredProvider, StoreError> {
    Ok(StoredProvider {
        id: row.id(0)?,
        org_id: row.id(1)?,
        issuer: row.text(2)?,
        client_id: row.text(3)?,
        sealed_client_secret: row.text(4)?,
        auto_join: row.bool(5)?,
        enabled: row.bool(6)?,
        create_at: row.int(7)?,
        update_at: row.int(8)?,
    })
}

/// Reads a sign-in under way from the columns `state_hash, browser_hash, nonce,
/// pkce_verifier, org_id, provider_id, create_at, expires_at`.
pub(crate) fn flow_from_row(row: &Row) -> Result<LoginFlow, StoreError> {
    Ok(LoginFlow {
        state_hash: row.text(0)?,
        browser_hash: row.text(1)?,
        nonce: row.text(2)?,
        pkce_verifier: row.text(3)?,
        org_id: row.id(4)?,
        provider_id: row.id(5)?,
        create_at: row.int(6)?,
        expires_at: row.int(7)?,
    })
}
