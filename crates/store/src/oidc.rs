//! Signing in through an organization's `OpenID Connect` provider: the provider, the
//! sign-ins under way, and who a provider vouches for.
//!
//! The provider and a sign-in that is started are the rows of the handle's organization
//! ([`Tenant::Org`](crate::Tenant::Org)). An identity, like the account it belongs to,
//! is nobody's. What is asked before the organization is known (which sign-in comes
//! back, which organization a login page is for) is in `directory.rs`; tying an
//! identity to an account and removing it, in `oidc_identities.rs`.

use fvoci_model::{AuditAction, LoginOrigin, OrgRole, Session, User, WorkspaceRole, new_id};

use crate::Store;
use crate::accounts::{self, Which};
use crate::audit::{self, AuditActor, AuditEntry};
use crate::db::{Dialect, Id, OptId, Row, Scope, now_millis};
use crate::error::StoreError;
use crate::sessions;
use crate::users::InstanceRole;

/// The columns of a sign-in under way, in the order [`flow_from_row`] reads them.
pub(crate) const FLOW_COLUMNS: &str = "state_hash, browser_hash, nonce, pkce_verifier, org_id, \
                                       provider_id, create_at, expires_at, link_session_id";

/// The columns of a provider, in the order [`provider_from_row`] reads them.
const PROVIDER_COLUMNS: &str = "id, org_id, issuer, client_id, client_secret, auto_join, enabled, \
                                create_at, update_at, sso_required";

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
    /// Whether what is the organization's opens only for a session made through it.
    pub sso_required: bool,
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
            .field("sso_required", &self.sso_required)
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
    /// The session it was started from to link what the provider vouches for to that
    /// session's account; `None` for a sign-in.
    pub link_session_id: Option<String>,
}

impl std::fmt::Debug for LoginFlow {
    /// Leaves out the nonce and the verifier.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoginFlow")
            .field("org_id", &self.org_id)
            .field("provider_id", &self.provider_id)
            .field("expires_at", &self.expires_at)
            .field("link_session_id", &self.link_session_id)
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
    #[cfg(any(test, feature = "test-util"))]
    pub async fn put_oidc_provider(
        &self,
        provider: &StoredProvider,
    ) -> Result<StoredProvider, StoreError> {
        self.put_provider(provider, AsRead::Whatever, None)
            .await?
            .ok_or(StoreError::NoRows)
    }

    /// Stores the provider of the handle's organization, in place of the one it has, if
    /// that one is still the one read, `read` (the same provider, last changed at the
    /// same time), or there is still none for `None`; and returns it as stored. `None`
    /// if it changed or was removed meanwhile, and nothing is stored: what was decided
    /// from it (the secret kept, the settings kept) is to be decided again. A provider whose issuer and client stay keeps its
    /// identifier and the time it was first set; one with another issuer or client is
    /// another provider, under a new identifier, and the sign-ins under way through the
    /// one before end with it; with another issuer, so do the sessions made through a
    /// provider of the organization under the issuer before. `provider.id`, `org_id`
    /// and the times are the store's;
    /// the time of a change of a provider is always later than that of the one before,
    /// and a provider removed and set again is another, so that a reading names one
    /// state of the provider.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NoOrganization`] if the handle names none, and
    /// [`StoreError::Database`] if a statement fails.
    pub async fn put_oidc_provider_as_read(
        &self,
        provider: &StoredProvider,
        read: Option<&StoredProvider>,
        actor_id: &str,
    ) -> Result<Option<StoredProvider>, StoreError> {
        let read = read.map(|read| (read.id.clone(), read.update_at));
        self.put_provider(provider, AsRead::Was(read), Some(actor_id))
            .await
    }

    async fn put_provider(
        &self,
        provider: &StoredProvider,
        as_read: AsRead,
        actor_id: Option<&str>,
    ) -> Result<Option<StoredProvider>, StoreError> {
        self.transaction(async |scope| {
            hold_provider_of_organization(scope).await?;
            let mut sql = scope.sql(&format!(
                "SELECT {PROVIDER_COLUMNS} FROM oidc_providers WHERE org_id = "
            ));
            sql.bind(scope.org()?);
            let before = match scope.fetch_optional(sql).await? {
                Some(row) => Some(provider_from_row(&row)?),
                None => None,
            };
            let there = before
                .as_ref()
                .map(|before| (before.id.clone(), before.update_at));
            if let AsRead::Was(read) = &as_read
                && there != *read
            {
                return Ok(None);
            }
            // Later than the change before, also within one millisecond or when the
            // clock went back: no two states of one provider have one time.
            let now = there.as_ref().map_or_else(now_millis, |(_, before)| {
                now_millis().max(before.saturating_add(1))
            });
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
                .push_bind(", ", provider.sso_required)
                .push(
                    ") ON CONFLICT (org_id) DO UPDATE SET \
                     issuer = excluded.issuer, client_id = excluded.client_id, \
                     client_secret = excluded.client_secret, \
                     auto_join = excluded.auto_join, enabled = excluded.enabled, \
                     update_at = excluded.update_at, sso_required = excluded.sso_required",
                )
                .push(&format!(" RETURNING {PROVIDER_COLUMNS}"));
            let row = scope.fetch_optional(sql).await?.ok_or(StoreError::NoRows)?;
            let stored = provider_from_row(&row)?;
            if let Some(before) = &before
                && before.issuer != stored.issuer
            {
                sessions::end_sessions_of_origin(scope, &before.org_id, &before.issuer).await?;
            }
            if let Some(actor_id) = actor_id {
                record_provider_change(scope, actor_id, before.as_ref(), &stored).await?;
            }
            Ok(Some(stored))
        })
        .await
    }

    /// Removes the provider of the handle's organization, the sign-ins under way
    /// through it, and the sessions made through a provider of the organization under
    /// its issuer, unless it is the one the organization takes nothing else than
    /// (`sso_required`): that one stays, and is released first. Asked and removed in
    /// one transaction that holds the provider, so that no removal passes an
    /// enforcement that is switched on beside it.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NoOrganization`] if the handle names none, and
    /// [`StoreError::Database`] if a statement fails.
    pub async fn delete_oidc_provider(&self, actor_id: &str) -> Result<Removal, StoreError> {
        let org_id = self.tenant().org_id().ok_or(StoreError::NoOrganization)?;
        self.transaction(async |scope| {
            hold_provider_of_organization(scope).await?;
            let mut sql =
                scope.sql("SELECT sso_required, issuer FROM oidc_providers WHERE org_id = ");
            sql.bind(scope.org()?);
            let issuer = match scope.fetch_optional(sql).await? {
                None => return Ok(Removal::NoneThere),
                Some(row) if row.bool(0)? => return Ok(Removal::Enforced),
                Some(row) => row.text(1)?,
            };
            let mut sql = scope.sql("DELETE FROM oidc_providers WHERE org_id = ");
            sql.bind(scope.org()?);
            scope.execute(sql).await?;
            sessions::end_sessions_of_origin(scope, &org_id, &issuer).await?;
            let entry = AuditEntry::new("", actor_id, AuditAction::OidcProviderRemoved, &org_id);
            audit::record_entry(scope, &entry).await?;
            Ok(Removal::Removed { issuer })
        })
        .await
    }

    /// Releases the handle's organization from taking only sessions made through its
    /// provider, on the word of whoever runs the server (the administration socket),
    /// whatever the provider is: the way out when nobody gets in through it. It is
    /// recorded as the server operator's; nothing that a person asks for calls it.
    /// Returns the provider as it is then if this released it, `None` if there was
    /// nothing to release.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NoOrganization`] if the handle names none, and
    /// [`StoreError::Database`] if a statement fails.
    pub async fn release_sso_by_operator(&self) -> Result<Option<StoredProvider>, StoreError> {
        self.transaction(async |scope| {
            hold_provider_of_organization(scope).await?;
            let mut sql = scope.sql("SELECT update_at FROM oidc_providers WHERE org_id = ");
            sql.bind(scope.org()?)
                .push_bind(" AND sso_required = ", true);
            let Some(row) = scope.fetch_optional(sql).await? else {
                return Ok(None);
            };
            // Another state of the provider, with a later time, as every change is.
            let now = now_millis().max(row.int(0)?.saturating_add(1));
            let mut sql = scope.sql("UPDATE oidc_providers SET sso_required = ");
            sql.bind(false)
                .push_bind(", update_at = ", now)
                .push_bind(" WHERE org_id = ", scope.org()?)
                .push(&format!(" RETURNING {PROVIDER_COLUMNS}"));
            let row = scope.fetch_optional(sql).await?.ok_or(StoreError::NoRows)?;
            let stored = provider_from_row(&row)?;
            let entry = AuditEntry::by(
                &AuditActor::ServerOperator,
                "",
                AuditAction::SsoReleased,
                &stored.org_id,
            );
            audit::record_entry(scope, &entry).await?;
            Ok(Some(stored))
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
                 org_id, provider_id, create_at, expires_at, link_session_id) SELECT ",
            );
            sql.bind(&flow.state_hash)
                .push_bind(", ", &flow.browser_hash)
                .push_bind(", ", &flow.nonce)
                .push_bind(", ", &flow.pkce_verifier)
                .push(", org_id, id")
                .push_bind(", ", flow.create_at)
                .push_bind(", ", flow.expires_at)
                .push_bind(
                    ", ",
                    OptId(flow.link_session_id.as_deref().unwrap_or_default()),
                )
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
    /// workspace. A known account ([`OidcAccount::Known`]) joins them if it has no
    /// place in the organization (a guest of one of its workspaces has one), as often
    /// as that is asked at once.
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
            let mut sql =
                scope.sql("SELECT auto_join, issuer, org_id FROM oidc_providers WHERE org_id = ");
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
            let (OidcAccount::New { identity, .. } | OidcAccount::Known { identity, .. }) =
                &account;
            let origin = LoginOrigin {
                provider_id: provider_id.to_owned(),
                org_id: row.id(2)?,
                issuer: row.text(1)?,
                subject: Some(identity.subject.to_owned()),
            };
            let joined = match account {
                OidcAccount::New {
                    user,
                    identity,
                    provider_email,
                } => {
                    if !auto_join {
                        return Ok(Err(OidcRefusal::JoinClosed));
                    }
                    user.password_hash = String::new();
                    user.email = String::new();
                    accounts::insert_account(scope, user, InstanceRole::User).await?;
                    insert_identity(scope, identity, &user.id, provider_email, user.create_at)
                        .await?;
                    join_as_member(scope, &user.id).await?;
                    session.user_id.clone_from(&user.id);
                    true
                }
                OidcAccount::Known {
                    user_id,
                    password_hash,
                    identity,
                } => {
                    // Held, the account waits for an unlinking under way, which ends
                    // the sessions of the identity: one made after it is refused here.
                    if !sessions::hold_unchanged_user(scope, user_id, password_hash).await?
                        || !is_identity_of(scope, identity, user_id).await?
                    {
                        return Ok(Err(OidcRefusal::AccountChanged));
                    }
                    user_id.clone_into(&mut session.user_id);
                    // A guest of one of its workspaces is let in as they are, and joins
                    // nothing: their board is what they have there.
                    if has_place(scope, user_id).await? {
                        false
                    } else if auto_join {
                        join_as_member(scope, user_id).await?
                    } else {
                        return Ok(Err(OidcRefusal::JoinClosed));
                    }
                }
            };
            session.login = Some(origin);
            sessions::insert_session(scope, session, token_digest).await?;
            Ok(Ok(joined))
        })
        .await
    }

    /// Whether the user is a member of the handle's organization.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NoOrganization`] if the handle names none, and
    /// [`StoreError::Database`] if the query fails.
    #[cfg(any(test, feature = "test-util"))]
    pub async fn is_organization_member(&self, user_id: &str) -> Result<bool, StoreError> {
        self.with_conn(async |scope| is_member(scope, user_id).await)
            .await
    }
}

/// Holds the provider of the transaction's organization, there or not, until the
/// transaction ends: changes and removals of it are one after the other. On PostgreSQL
/// the organization's row is held (no key of it changes, so its references are not held
/// up); on SQLite a transaction that writes holds the database.
async fn hold_provider_of_organization(scope: &mut Scope<'_>) -> Result<(), StoreError> {
    if scope.dialect() == Dialect::Postgres {
        let mut sql = scope.sql("SELECT 1 FROM organizations WHERE id = ");
        sql.bind(scope.org()?).push(" FOR NO KEY UPDATE");
        scope.fetch_optional(sql).await?;
    }
    Ok(())
}

/// What came of removing the provider of an organization.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Removal {
    /// It is removed, and the sessions made through a provider of the organization
    /// under its issuer ended with it.
    Removed {
        /// The issuer the removed provider had.
        issuer: String,
    },
    /// The organization had none.
    NoneThere,
    /// It stays: the organization takes only sessions made through it.
    Enforced,
}

/// Records what this save changed from `before`, in the transaction that stores it:
/// the provider's settings, and the switch of enforcement, each when it changed. A
/// save that changes nothing records nothing.
async fn record_provider_change(
    scope: &mut Scope<'_>,
    actor_id: &str,
    before: Option<&StoredProvider>,
    stored: &StoredProvider,
) -> Result<(), StoreError> {
    let configured = before.is_none_or(|before| {
        (
            &before.issuer,
            &before.client_id,
            &before.sealed_client_secret,
            before.auto_join,
            before.enabled,
        ) != (
            &stored.issuer,
            &stored.client_id,
            &stored.sealed_client_secret,
            stored.auto_join,
            stored.enabled,
        )
    });
    let was_required = before.is_some_and(|before| before.sso_required);
    let enforcement = match (was_required, stored.sso_required) {
        (false, true) => Some(AuditAction::SsoRequired),
        (true, false) => Some(AuditAction::SsoReleased),
        _ => None,
    };
    let actions = configured
        .then_some(AuditAction::OidcProviderConfigured)
        .into_iter()
        .chain(enforcement);
    for action in actions {
        let entry = AuditEntry::new("", actor_id, action, &stored.org_id);
        audit::record_entry(scope, &entry).await?;
    }
    Ok(())
}

/// What a provider that is replaced must still be.
#[derive(Debug, Clone)]
enum AsRead {
    /// Anything: only for the tests, which set a provider without reading it first.
    #[cfg(any(test, feature = "test-util"))]
    Whatever,
    /// The provider of this identifier, last changed at this time; or none for `None`.
    Was(Option<(String, i64)>),
}

/// The account a sign-in through a provider ends in.
#[derive(Debug)]
pub enum OidcAccount<'a> {
    /// The account of the identity, with the password hash it was read with: a
    /// password set, the account deleted, or the identity unlinked meanwhile stops the
    /// sign-in.
    Known {
        /// The account.
        user_id: &'a str,
        /// Its password hash as it was read: empty for an account without one.
        password_hash: &'a str,
        /// Who the provider vouches for, which must still be the account's.
        identity: Identity<'a>,
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
    /// The account was deleted, given a password, or its identity unlinked, while
    /// signing in.
    AccountChanged,
}

/// Whether the identity is still the account's.
async fn is_identity_of(
    scope: &mut Scope<'_>,
    identity: Identity<'_>,
    user_id: &str,
) -> Result<bool, StoreError> {
    let mut sql = scope.sql("SELECT user_id FROM user_identities WHERE issuer = ");
    sql.bind(identity.issuer)
        .push_bind(" AND subject = ", identity.subject)
        .push_bind(" AND user_id = ", Id(user_id));
    Ok(scope.fetch_optional(sql).await?.is_some())
}

/// Makes a user a member of the scope's organization and of its default workspace, as a
/// member of both, unless the user is a member of the organization already; says
/// whether the user joined. Two at once for the same user both succeed, and one of them
/// joins.
async fn join_as_member(scope: &mut Scope<'_>, user_id: &str) -> Result<bool, StoreError> {
    if !accounts::join_organization(scope, user_id, OrgRole::Member).await? {
        return Ok(false);
    }
    accounts::join_workspace(
        scope,
        Which::DefaultWorkspace,
        user_id,
        WorkspaceRole::Member,
    )
    .await?;
    Ok(true)
}

/// Whether the user is a member of the scope's organization.
/// Whether the user has a place in the transaction's organization: a member of it, or
/// of one of its workspaces in use (a guest of a workspace is no member of the
/// organization), as the wall counts them.
async fn has_place(scope: &mut Scope<'_>, user_id: &str) -> Result<bool, StoreError> {
    if is_member(scope, user_id).await? {
        return Ok(true);
    }
    let mut sql = scope.sql(
        "SELECT 1 FROM workspace_members AS wm JOIN workspaces AS t ON t.id = wm.workspace_id \
         WHERE t.delete_at = 0 AND t.org_id = ",
    );
    sql.bind(scope.org()?)
        .push_bind(" AND wm.user_id = ", Id(user_id));
    Ok(scope.fetch_optional(sql).await?.is_some())
}

async fn is_member(scope: &mut Scope<'_>, user_id: &str) -> Result<bool, StoreError> {
    let mut sql = scope.sql("SELECT 1 FROM organization_members WHERE org_id = ");
    sql.bind(scope.org()?)
        .push_bind(" AND user_id = ", Id(user_id));
    Ok(scope.fetch_optional(sql).await?.is_some())
}

/// Ties an identity to an account, as the provider gave its address.
pub(crate) async fn insert_identity(
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
        sso_required: row.bool(9)?,
    })
}

/// Reads a sign-in under way from the columns [`FLOW_COLUMNS`].
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
        link_session_id: Some(row.id(8)?).filter(|id| !id.is_empty()),
    })
}
