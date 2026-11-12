//! Signing in through an organization's `OpenID Connect` provider: the provider, the
//! sign-ins under way, and who a provider vouches for.
//!
//! The provider and a sign-in that is started are the rows of the handle's organization
//! ([`Tenant::Org`](crate::Tenant::Org)). An identity, like the account it belongs to,
//! is nobody's. What is asked before the organization is known (which sign-in comes
//! back, which organization a login page is for) is in `directory.rs`.

use fvoci_model::{Session, User, new_id};

use crate::Store;
use crate::db::{Dialect, Id, Row, Scope, now_millis};
use crate::error::StoreError;
use crate::users::{InstanceRole, insert_user_row};
use crate::{members, sessions};

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

    /// Whether the identity is an account's, whether the account is deleted or not.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the query fails.
    pub async fn is_identity_known(&self, identity: Identity<'_>) -> Result<bool, StoreError> {
        self.with_conn(async |scope| {
            let mut sql = scope.sql("SELECT 1 FROM user_identities WHERE issuer = ");
            sql.bind(identity.issuer)
                .push_bind(" AND subject = ", identity.subject);
            Ok(scope.fetch_optional(sql).await?.is_some())
        })
        .await
    }

    /// Ends a sign-in through the provider `provider_id` of the handle's organization,
    /// in one transaction: reads the provider again, and holds it on PostgreSQL, so that
    /// a provider switched off, removed, replaced or closed to newcomers while the
    /// person was at the provider lets nobody in; then makes or lets in the account,
    /// and stores `session` (for `account`'s user) under `token_digest`. Says whether
    /// the account joined the organization, or why nothing was stored.
    ///
    /// A new account ([`OidcAccount::New`]) is stored with no password (an empty
    /// hash, which no password matches) and no email address of its own:
    /// `provider_email`, the address the provider gave, is kept with the identity,
    /// unchecked, and holds no account's place. `user` is updated to what was stored:
    /// the current time as creation and modification time, not deleted, no
    /// administrator of the instance. It joins the organization and its default
    /// workspace. A known account ([`OidcAccount::Known`]) joins them if it is no
    /// member, as often as that is asked at once.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Conflict`] if another account has the new account's name
    /// in any letter case, or the identity is another account's already;
    /// [`StoreError::NoOrganization`] if the handle names no organization; and
    /// [`StoreError::Database`] if a statement fails. Nothing is stored then.
    pub async fn finish_oidc_sign_in(
        &self,
        provider_id: &str,
        account: OidcAccount<'_>,
        session: &mut Session,
        token_digest: &str,
    ) -> Result<Result<bool, OidcRefusal>, StoreError> {
        self.transaction(async |scope| {
            let mut sql = scope.sql("SELECT auto_join FROM oidc_providers WHERE org_id = ");
            sql.bind(scope.org()?)
                .push_bind(" AND id = ", Id(provider_id))
                .push_bind(" AND enabled = ", true);
            if scope.dialect() == Dialect::Postgres {
                sql.push(" FOR SHARE");
            }
            let Some(row) = scope.fetch_optional(sql).await? else {
                return Ok(Err(OidcRefusal::ProviderGone));
            };
            let auto_join = row.bool(0)?;
            let joined = match account {
                OidcAccount::New {
                    user,
                    identity,
                    provider_email,
                } => {
                    if !auto_join {
                        return Ok(Err(OidcRefusal::JoinClosed));
                    }
                    let now = now_millis();
                    user.create_at = now;
                    user.update_at = now;
                    user.delete_at = 0;
                    user.is_admin = false;
                    user.password_hash = String::new();
                    user.email = String::new();
                    insert_user_row(scope, user).await?;
                    insert_identity(scope, identity, &user.id, provider_email, now).await?;
                    members::join_organization(scope, &user.id, InstanceRole::User).await?;
                    session.user_id.clone_from(&user.id);
                    true
                }
                OidcAccount::Known {
                    user_id,
                    password_hash,
                } => {
                    if !sessions::hold_unchanged_user(scope, user_id, password_hash).await? {
                        return Ok(Err(OidcRefusal::AccountChanged));
                    }
                    user_id.clone_into(&mut session.user_id);
                    if is_member(scope, user_id).await? {
                        false
                    } else if auto_join {
                        members::join_organization_unless_member(scope, user_id).await?
                    } else {
                        return Ok(Err(OidcRefusal::JoinClosed));
                    }
                }
            };
            session.login_provider_id = Some(provider_id.to_owned());
            sessions::insert_session(scope, session, token_digest).await?;
            Ok(Ok(joined))
        })
        .await
    }

    /// The email addresses the providers gave for an account, one for each identity of
    /// it, oldest first: their word, checked by nobody. For tests: nothing shows them
    /// yet, and what does is to mark them as unchecked.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the query fails.
    #[cfg(any(test, feature = "test-util"))]
    pub async fn provider_emails(&self, user_id: &str) -> Result<Vec<String>, StoreError> {
        self.with_conn(async |scope| {
            let mut sql = scope.sql("SELECT email FROM user_identities WHERE user_id = ");
            sql.bind(Id(user_id))
                .push(" ORDER BY create_at, issuer, subject");
            scope
                .fetch_all(sql)
                .await?
                .iter()
                .map(|row| row.text(0))
                .collect()
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
    #[cfg(any(test, feature = "test-util"))]
    pub async fn join_organization_through_oidc(&self, user_id: &str) -> Result<bool, StoreError> {
        self.transaction(async |scope| {
            if is_member(scope, user_id).await? {
                return Ok(false);
            }
            members::join_organization_unless_member(scope, user_id).await
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
        self.with_conn(async |scope| is_member(scope, user_id).await)
            .await
    }
}

/// The account a sign-in through a provider ends in.
#[derive(Debug)]
pub enum OidcAccount<'a> {
    /// The account of the identity, with the password hash it was read with: a
    /// password set or the account deleted meanwhile stops the sign-in.
    Known {
        /// The account.
        user_id: &'a str,
        /// Its password hash as it was read: empty for an account without one.
        password_hash: &'a str,
    },
    /// A new account for the identity, with the address the provider gave.
    New {
        /// The account to store; updated to what was stored.
        user: &'a mut User,
        /// Who the provider vouches for.
        identity: Identity<'a>,
        /// The address the provider gave, unchecked.
        provider_email: &'a str,
    },
}

/// Why a sign-in through a provider stored nothing at its end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OidcRefusal {
    /// The provider is switched off, removed, or another one, since the sign-in began.
    ProviderGone,
    /// The organization lets no newcomer in through its provider.
    JoinClosed,
    /// The account was deleted, or given a password, while signing in.
    AccountChanged,
}

/// Whether the user is a member of the scope's organization.
async fn is_member(scope: &mut Scope<'_>, user_id: &str) -> Result<bool, StoreError> {
    let mut sql = scope.sql("SELECT 1 FROM organization_members WHERE org_id = ");
    sql.bind(scope.org()?)
        .push_bind(" AND user_id = ", Id(user_id));
    Ok(scope.fetch_optional(sql).await?.is_some())
}

async fn insert_identity(
    scope: &mut Scope<'_>,
    identity: Identity<'_>,
    user_id: &str,
    email: &str,
    now: i64,
) -> Result<(), StoreError> {
    let mut sql = scope
        .sql("INSERT INTO user_identities (issuer, subject, user_id, email, create_at) VALUES (");
    sql.bind(identity.issuer)
        .push_bind(", ", identity.subject)
        .push_bind(", ", Id(user_id))
        .push_bind(", ", email)
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
