//! The pending settings of an organization's provider: what an administrator saved of
//! its sign-in (issuer, client, secret) while the organization takes no password. They
//! are kept aside, and nobody signs in through them, until a sign-in through them ends
//! well (a trial): then, in the transaction of that sign-in, they become the
//! provider's. Until then the provider is signed in through as it was, so that settings
//! that do not work lock nobody out. The provider and its sign-ins are in `oidc.rs`.

use std::time::Duration;

use fvoci_model::{AuditAction, LoginOrigin, OrgRole, Session};

use crate::Store;
use crate::audit::{self, AuditEntry};
use crate::db::{Dialect, Id, Row, Scope, now_millis};
use crate::error::StoreError;
use crate::oidc::{
    self, Newcomers, OidcAccount, OidcRefusal, StoredProvider, hold_provider_of_organization,
    read_provider,
};
use crate::oidc_former::Keeping;
use crate::org_admin;
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

impl PendingProvider {
    /// `provider` with these settings' issuer, client and secret: what a trial signs in
    /// through, and what the provider becomes when it ends well.
    #[must_use]
    pub fn applied_to(&self, provider: &StoredProvider) -> StoredProvider {
        StoredProvider {
            issuer: self.issuer.clone(),
            client_id: self.client_id.clone(),
            sealed_client_secret: self.sealed_client_secret.clone(),
            ..provider.clone()
        }
    }
}

/// What an administrator saves aside of the provider's sign-in.
#[derive(Clone, PartialEq, Eq)]
pub struct PendingSettings {
    /// The issuer, exactly as the provider names itself.
    pub issuer: String,
    /// The client identifier.
    pub client_id: String,
    /// The client secret as the server sealed it, for this issuer and client; empty for
    /// none.
    pub sealed_client_secret: String,
}

impl std::fmt::Debug for PendingSettings {
    /// Leaves out the sealed secret.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PendingSettings")
            .field("issuer", &self.issuer)
            .field("client_id", &self.client_id)
            .finish_non_exhaustive()
    }
}

/// A trial of the pending settings, as the sign-in that ends it names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Trial<'a> {
    /// The provider whose pending settings are tried.
    pub provider_id: &'a str,
    /// When the pending settings it tries were saved: their state.
    pub pending_at: i64,
    /// The administrator who started it, whose account alone it may end in.
    pub trier_id: &'a str,
    /// How long the provider before is kept as the former one if the trial moves
    /// the organization to another issuer; no time keeps none.
    pub keep_former: Duration,
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

    /// Saves `settings` as the pending settings of the provider of the handle's
    /// organization, in place of the ones there are, if the provider is still the one
    /// read, `read` (the same provider, last changed at the same time), and the
    /// organization still takes no password; and in the same transaction stores
    /// `kept`, the provider with what else the save changes (whether newcomers join),
    /// if that differs. Both are recorded as `actor_id`'s. Returns the provider and
    /// the pending settings as stored, or `None` if the provider changed meanwhile, and
    /// nothing is stored.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NoOrganization`] if the handle names none, and
    /// [`StoreError::Database`] if a statement fails.
    pub async fn put_oidc_pending_provider(
        &self,
        settings: &PendingSettings,
        kept: &StoredProvider,
        read: &StoredProvider,
        actor_id: &str,
    ) -> Result<Option<(StoredProvider, PendingProvider)>, StoreError> {
        self.transaction(async |scope| {
            hold_provider_of_organization(scope).await?;
            let Some(mut provider) = read_provider(scope).await? else {
                return Ok(None);
            };
            if (&provider.id, provider.update_at) != (&read.id, read.update_at)
                || !provider.sso_required
            {
                return Ok(None);
            }
            if kept.auto_join != provider.auto_join {
                let before = provider;
                let kept = StoredProvider {
                    auto_join: kept.auto_join,
                    ..before.clone()
                };
                provider = oidc::write_provider(scope, Some(&before), &kept, Keeping::NOTHING)
                    .await?
                    .provider;
                oidc::record_provider_change(scope, actor_id, Some(&before), &provider).await?;
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
                .push_bind(", ", &settings.issuer)
                .push_bind(", ", &settings.client_id)
                .push_bind(", ", &settings.sealed_client_secret)
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
            Ok(Some((provider, stored)))
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

    /// Ends `trial`, a trial of the pending settings of the provider of the handle's
    /// organization, in one transaction that holds the provider: they must still be
    /// the provider's pending settings, it must still be on, and `account` must be
    /// the trier's own, who must still administer the organization (no account is
    /// made by a trial: one that ended in a newcomer or another person would put
    /// settings in use that no administrator can sign in through). Lets `account` in
    /// as a sign-in does (`Store::finish_oidc_sign_in`), makes the pending settings
    /// the provider's, as a save of them would (another issuer ends the sessions of
    /// the one before, and keeps the provider before as the former one), records
    /// that as the account's doing, and stores `session` under `token_digest`, made
    /// through the provider as it is then: it proves it. Nothing is stored if any of
    /// it is refused.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Conflict`] if another account has the new account's name
    /// in any letter case, or the identity is another account's already;
    /// [`StoreError::NoOrganization`] if the handle names no organization; and
    /// [`StoreError::Database`] if a statement fails.
    pub async fn finish_oidc_trial(
        &self,
        trial: Trial<'_>,
        account: OidcAccount<'_>,
        session: &mut Session,
        token_digest: &str,
    ) -> Result<Result<Applied, OidcRefusal>, StoreError> {
        let Trial {
            provider_id,
            pending_at,
            trier_id,
            keep_former,
        } = trial;
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
            let is_the_trier =
                matches!(&account, OidcAccount::Known { user_id, .. } if *user_id == trier_id);
            if !is_the_trier
                || org_admin::org_role_of(scope, trier_id).await? != Some(OrgRole::Admin)
            {
                return Ok(Err(OidcRefusal::NotTheTrier));
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
            let applied = pending.applied_to(&before);
            let keeping = Keeping {
                time: keep_former,
                actor_id: Some(trier_id),
            };
            let provider = oidc::write_provider(scope, Some(&before), &applied, keeping)
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
