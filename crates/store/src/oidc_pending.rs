//! The pending settings of an organization's provider: what an administrator saved of
//! its sign-in (issuer, client, secret) while the organization takes no password. They
//! are kept aside, and nobody signs in through them, until a sign-in through them ends
//! well (a trial): then, in the transaction of that sign-in, they become the
//! provider's. Until then the provider is signed in through as it was, so that settings
//! that do not work lock nobody out. The provider and its sign-ins are in `oidc.rs`.

use fvoci_model::{AuditAction, LoginOrigin, Session};

use crate::Store;
use crate::audit::{self, AuditEntry};
use crate::db::{Dialect, Id, Row, Scope, now_millis};
use crate::error::StoreError;
use crate::oidc::{
    self, Newcomers, OidcAccount, OidcRefusal, StoredProvider, hold_provider_of_organization,
    read_provider,
};
use crate::sessions;

/// The columns of pending settings, in the order [`pending_from_row`] reads them.
const PENDING_COLUMNS: &str = "provider_id, issuer, client_id, client_secret, create_at";

/// The pending settings of the provider of an organization, as they are stored.
#[derive(Clone, PartialEq, Eq)]
pub struct PendingProvider {
    /// The provider they are of; they go with it.
    pub provider_id: String,
    /// The issuer, exactly as the provider names itself.
    pub issuer: String,
    /// The client identifier.
    pub client_id: String,
    /// The client secret as the server sealed it, for this issuer and client; empty for
    /// none.
    pub sealed_client_secret: String,
    /// When they were saved, in milliseconds since the Unix epoch: later than the
    /// settings saved before, so that it names this state of them. The store's.
    pub create_at: i64,
}

impl std::fmt::Debug for PendingProvider {
    /// Leaves out the sealed secret.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PendingProvider")
            .field("provider_id", &self.provider_id)
            .field("issuer", &self.issuer)
            .field("client_id", &self.client_id)
            .field("create_at", &self.create_at)
            .finish_non_exhaustive()
    }
}

/// What a trial of the pending settings that ended well did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Applied {
    /// The provider as it was before.
    pub before: StoredProvider,
    /// The provider as it is now, with the pending settings: another one, under a new
    /// identifier, for another issuer or client.
    pub provider: StoredProvider,
    /// Whether the account joined the organization by it.
    pub joined: bool,
}

impl Store {
    /// The pending settings of the provider of the handle's organization, if there are.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NoOrganization`] if the handle names none, and
    /// [`StoreError::Database`] if the query fails.
    pub async fn oidc_pending_provider(&self) -> Result<Option<PendingProvider>, StoreError> {
        self.with_conn(async |scope| read_pending(scope).await)
            .await
    }

    /// Saves `pending` as the pending settings of the provider of the handle's
    /// organization, in place of the ones there are, if the provider is still the one
    /// read, `read` (the same provider, last changed at the same time), and the
    /// organization still takes no password; recorded as `actor_id`'s. Returns them as
    /// stored, or `None` if the provider changed meanwhile, and nothing is stored.
    /// `pending.provider_id` and `create_at` are the store's.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NoOrganization`] if the handle names none, and
    /// [`StoreError::Database`] if a statement fails.
    pub async fn put_oidc_pending_provider(
        &self,
        pending: &PendingProvider,
        read: &StoredProvider,
        actor_id: &str,
    ) -> Result<Option<PendingProvider>, StoreError> {
        self.transaction(async |scope| {
            hold_provider_of_organization(scope).await?;
            let Some(provider) = read_provider(scope).await? else {
                return Ok(None);
            };
            if (&provider.id, provider.update_at) != (&read.id, read.update_at)
                || !provider.sso_required
            {
                return Ok(None);
            }
            // Later than the settings before, also within one millisecond or when the
            // clock went back: a trial names one state of them.
            let now = match read_pending(scope).await? {
                Some(before) => now_millis().max(before.create_at.saturating_add(1)),
                None => now_millis(),
            };
            let mut sql = scope.sql(&format!(
                "INSERT INTO oidc_pending_providers (org_id, {PENDING_COLUMNS}) VALUES ("
            ));
            sql.bind(scope.org()?)
                .push_bind(", ", Id(&provider.id))
                .push_bind(", ", &pending.issuer)
                .push_bind(", ", &pending.client_id)
                .push_bind(", ", &pending.sealed_client_secret)
                .push_bind(", ", now)
                .push(
                    ") ON CONFLICT (org_id) DO UPDATE SET provider_id = excluded.provider_id, \
                     issuer = excluded.issuer, client_id = excluded.client_id, \
                     client_secret = excluded.client_secret, create_at = excluded.create_at",
                )
                .push(&format!(" RETURNING {PENDING_COLUMNS}"));
            let row = scope.fetch_optional(sql).await?.ok_or(StoreError::NoRows)?;
            let stored = pending_from_row(&row)?;
            let entry = AuditEntry::new(
                "",
                actor_id,
                AuditAction::OidcProviderPendingSaved,
                &provider.org_id,
            );
            audit::record_entry(scope, &entry).await?;
            Ok(Some(stored))
        })
        .await
    }

    /// Throws the pending settings of the provider of the handle's organization away,
    /// recorded as `actor_id`'s. Returns whether there were any.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NoOrganization`] if the handle names none, and
    /// [`StoreError::Database`] if a statement fails.
    pub async fn discard_oidc_pending_provider(&self, actor_id: &str) -> Result<bool, StoreError> {
        let org_id = self.tenant().org_id().ok_or(StoreError::NoOrganization)?;
        self.transaction(async |scope| {
            hold_provider_of_organization(scope).await?;
            let mut sql = scope.sql("DELETE FROM oidc_pending_providers WHERE org_id = ");
            sql.bind(scope.org()?);
            if scope.execute(sql).await? == 0 {
                return Ok(false);
            }
            let entry = AuditEntry::new(
                "",
                actor_id,
                AuditAction::OidcProviderPendingDiscarded,
                &org_id,
            );
            audit::record_entry(scope, &entry).await?;
            Ok(true)
        })
        .await
    }

    /// Ends a trial of the pending settings saved at `pending_at` of the provider
    /// `provider_id` of the handle's organization, in one transaction that holds the
    /// provider: they must still be the provider's pending settings, and it must still
    /// be on. Lets `account` in as a sign-in does (`Store::finish_oidc_sign_in`), makes
    /// the pending settings the provider's, as a save of them would (another issuer
    /// ends the sessions of the one before), records that as the account's doing, and
    /// stores `session` under `token_digest`, made through the provider as it is then:
    /// it proves it. Nothing is stored if any of it is refused.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Conflict`] if another account has the new account's name
    /// in any letter case, or the identity is another account's already;
    /// [`StoreError::NoOrganization`] if the handle names no organization; and
    /// [`StoreError::Database`] if a statement fails.
    pub async fn finish_oidc_trial(
        &self,
        provider_id: &str,
        pending_at: i64,
        account: OidcAccount<'_>,
        session: &mut Session,
        token_digest: &str,
    ) -> Result<Result<Applied, OidcRefusal>, StoreError> {
        self.transaction(async |scope| {
            hold_provider_of_organization(scope).await?;
            let (Some(before), Some(pending)) =
                (read_provider(scope).await?, read_pending(scope).await?)
            else {
                return Ok(Err(OidcRefusal::ProviderGone));
            };
            if before.id != provider_id
                || !before.enabled
                || pending.provider_id != provider_id
                || pending.create_at != pending_at
            {
                return Ok(Err(OidcRefusal::ProviderGone));
            }
            let subject = account.identity().subject.to_owned();
            let newcomers = Newcomers::from(&before);
            let joined = match oidc::admit(scope, account, newcomers, session).await? {
                Ok(joined) => joined,
                Err(refusal) => return Ok(Err(refusal)),
            };
            let mut sql = scope.sql("DELETE FROM oidc_pending_providers WHERE org_id = ");
            sql.bind(scope.org()?);
            scope.execute(sql).await?;
            let applied = StoredProvider {
                issuer: pending.issuer,
                client_id: pending.client_id,
                sealed_client_secret: pending.sealed_client_secret,
                ..before.clone()
            };
            let provider = oidc::write_provider(scope, Some(&before), &applied)
                .await?
                .provider;
            let entry = AuditEntry::new(
                "",
                &session.user_id,
                AuditAction::OidcProviderPendingApplied,
                &provider.org_id,
            );
            audit::record_entry(scope, &entry).await?;
            session.login = Some(LoginOrigin {
                provider_id: provider.id.clone(),
                org_id: provider.org_id.clone(),
                issuer: provider.issuer.clone(),
                subject: Some(subject),
                revision: Some(provider.revision),
            });
            sessions::insert_session(scope, session, token_digest).await?;
            Ok(Ok(Applied {
                before,
                provider,
                joined,
            }))
        })
        .await
    }
}

/// The pending settings of the transaction's organization's provider, if there are.
async fn read_pending(scope: &mut Scope<'_>) -> Result<Option<PendingProvider>, StoreError> {
    let mut sql = scope.sql(&format!(
        "SELECT {PENDING_COLUMNS} FROM oidc_pending_providers WHERE org_id = "
    ));
    sql.bind(scope.org()?);
    if scope.dialect() == Dialect::Postgres {
        sql.push(" FOR UPDATE");
    }
    scope
        .fetch_optional(sql)
        .await?
        .as_ref()
        .map(pending_from_row)
        .transpose()
}

fn pending_from_row(row: &Row) -> Result<PendingProvider, StoreError> {
    Ok(PendingProvider {
        provider_id: row.id(0)?,
        issuer: row.text(1)?,
        client_id: row.text(2)?,
        sealed_client_secret: row.text(3)?,
        create_at: row.int(4)?,
    })
}
