//! The identities tied to an account: listing them, tying one at the end of a sign-in
//! to link, and removing one again, which ends the account's sessions of its issuer.
//! Signing in through a provider, and the provider itself, are in `oidc.rs`.

use fvoci_model::LinkedIdentity;

use crate::Store;
use crate::db::{Dialect, Id, now_millis};
use crate::error::StoreError;
use crate::oidc::{Identity, insert_identity};
use crate::sessions;

impl Store {
    /// The identities tied to an account, each with the address its provider gave:
    /// oldest first.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the query fails.
    pub async fn identities_of_user(
        &self,
        user_id: &str,
    ) -> Result<Vec<LinkedIdentity>, StoreError> {
        self.with_conn(async |scope| {
            let mut sql = scope.sql(
                "SELECT issuer, subject, email, create_at FROM user_identities WHERE user_id = ",
            );
            sql.bind(Id(user_id))
                .push(" ORDER BY create_at, issuer, subject");
            scope
                .fetch_all(sql)
                .await?
                .iter()
                .map(|row| {
                    Ok(LinkedIdentity {
                        issuer: row.text(0)?,
                        subject: row.text(1)?,
                        email: row.text(2)?,
                        create_at: row.int(3)?,
                        // Of the session that asks, which the store does not know.
                        made_this_session: false,
                    })
                })
                .collect()
        })
        .await
    }

    /// Ends a sign-in through the provider `provider_id` of the handle's organization
    /// that was started from a session to link: ties `identity` to the account of that
    /// session, in one transaction. The provider is read again and held, as at the end
    /// of a sign-in; the session `link.session_id` must still be there under the token
    /// digest the browser came back with, good at `link.now`, and of an account in use.
    /// Nothing is stored otherwise. An identity that is the account's already is left
    /// as it is.
    ///
    /// The identity is tied only where `provider_email`, the address the provider
    /// vouches for, is the account's own (whatever the case of its letters): who knows
    /// an account's password ties no other person of the provider to it, who would
    /// then sign in to it without the password.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NoOrganization`] if the handle names no organization, and
    /// [`StoreError::Database`] if a statement fails.
    pub async fn finish_oidc_link(
        &self,
        provider_id: &str,
        link: LinkingSession<'_>,
        identity: Identity<'_>,
        provider_email: &str,
    ) -> Result<Result<Linked, LinkRefusal>, StoreError> {
        self.transaction(async |scope| {
            let mut sql = scope.sql("SELECT 1 FROM oidc_providers WHERE org_id = ");
            sql.bind(scope.org()?)
                .push_bind(" AND id = ", Id(provider_id))
                .push_bind(" AND enabled = ", true);
            if scope.dialect() == Dialect::Postgres {
                sql.push(" FOR SHARE");
            }
            if scope.fetch_optional(sql).await?.is_none() {
                return Ok(Err(LinkRefusal::ProviderGone));
            }
            let mut sql = scope.sql(
                "SELECT s.user_id, u.email FROM sessions AS s JOIN users AS u \
                 ON u.id = s.user_id WHERE s.id = ",
            );
            sql.bind(Id(link.session_id))
                .push_bind(" AND s.token_hash = ", link.token_digest)
                .push_bind(" AND s.expires_at > ", link.now)
                .push_bind(" AND s.last_used_at > ", link.idle_since)
                .push_bind(" AND u.delete_at = ", 0_i64);
            if scope.dialect() == Dialect::Postgres {
                // The account stays while its identity is stored.
                sql.push(" FOR SHARE OF u");
            }
            let Some(row) = scope.fetch_optional(sql).await? else {
                return Ok(Err(LinkRefusal::SessionEnded));
            };
            let user_id = row.id(0)?;
            let is_the_accounts_address = !provider_email.is_empty()
                && provider_email.to_lowercase() == row.text(1)?.to_lowercase();
            if !is_the_accounts_address {
                // Nothing is tied; one that is the account's already is that still.
                let mut sql = scope.sql("SELECT 1 FROM user_identities WHERE issuer = ");
                sql.bind(identity.issuer)
                    .push_bind(" AND subject = ", identity.subject)
                    .push_bind(" AND user_id = ", Id(&user_id));
                return Ok(match scope.fetch_optional(sql).await? {
                    Some(_) => Ok(Linked {
                        user_id,
                        newly: false,
                    }),
                    None => Err(LinkRefusal::AnotherAddress),
                });
            }
            // Tied unless it is somebody's already, in one statement: of two links of
            // one identity at once the second waits for the first and ties nothing, and
            // the transaction goes on to read whose it is.
            let mut sql = scope.sql(
                "INSERT INTO user_identities (issuer, subject, user_id, email, create_at) VALUES (",
            );
            sql.bind(identity.issuer)
                .push_bind(", ", identity.subject)
                .push_bind(", ", Id(&user_id))
                .push_bind(", ", provider_email)
                .push_bind(", ", now_millis())
                .push(") ON CONFLICT (issuer, subject) DO NOTHING");
            let newly = scope.execute(sql).await? > 0;
            let mut sql = scope.sql("SELECT user_id FROM user_identities WHERE issuer = ");
            sql.bind(identity.issuer)
                .push_bind(" AND subject = ", identity.subject);
            let owner = match scope.fetch_optional(sql).await? {
                Some(row) => row.id(0)?,
                None => return Err(StoreError::NoRows),
            };
            Ok(if owner == user_id {
                Ok(Linked { user_id, newly })
            } else {
                Err(LinkRefusal::IdentityTaken)
            })
        })
        .await
    }

    /// Removes an identity from the account `user_id`, whose password hash was read as
    /// `seen_password_hash`, in one transaction that holds the account: so that of two
    /// removals at once the second sees what the first left. The last identity of an
    /// account without a password stays: nothing else lets its owner in.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if a statement fails.
    pub async fn unlink_identity(
        &self,
        user_id: &str,
        seen_password_hash: &str,
        identity: Identity<'_>,
    ) -> Result<Unlinked, StoreError> {
        self.transaction(async |scope| {
            let mut sql =
                scope.sql("SELECT password_hash FROM users WHERE delete_at = 0 AND id = ");
            sql.bind(Id(user_id));
            if scope.dialect() == Dialect::Postgres {
                sql.push(" FOR UPDATE");
            }
            let password_hash = match scope.fetch_optional(sql).await? {
                Some(row) => row.text(0)?,
                None => return Ok(Unlinked::AccountChanged),
            };
            if password_hash != seen_password_hash {
                return Ok(Unlinked::AccountChanged);
            }
            let mut sql = scope.sql("SELECT issuer, subject FROM user_identities WHERE user_id = ");
            sql.bind(Id(user_id));
            let tied: Vec<(String, String)> = scope
                .fetch_all(sql)
                .await?
                .iter()
                .map(|row| Ok((row.text(0)?, row.text(1)?)))
                .collect::<Result<_, StoreError>>()?;
            if !tied
                .iter()
                .any(|(issuer, subject)| issuer == identity.issuer && subject == identity.subject)
            {
                return Ok(Unlinked::NotTied);
            }
            if password_hash.is_empty() && tied.len() == 1 {
                return Ok(Unlinked::LastWayIn);
            }
            let mut sql = scope.sql("DELETE FROM user_identities WHERE user_id = ");
            sql.bind(Id(user_id))
                .push_bind(" AND issuer = ", identity.issuer)
                .push_bind(" AND subject = ", identity.subject);
            scope.execute(sql).await?;
            sessions::end_sessions_of_issuer(scope, user_id, identity.issuer).await?;
            Ok(Unlinked::Removed)
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

    /// Ties an identity to an account, as a provider gave it. For tests that need an
    /// account that signs in through a provider without the sign-in that makes one.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Conflict`] if the identity is an account's already, and
    /// [`StoreError::Database`] if the statement fails.
    #[cfg(any(test, feature = "test-util"))]
    pub async fn add_identity(
        &self,
        user_id: &str,
        identity: Identity<'_>,
        provider_email: &str,
    ) -> Result<(), StoreError> {
        self.with_conn(async |scope| {
            insert_identity(scope, identity, user_id, provider_email, now_millis()).await
        })
        .await
    }
}

/// The session a sign-in to link was started from, as the browser brings it back.
#[derive(Debug, Clone, Copy)]
pub struct LinkingSession<'a> {
    /// The session the flow names.
    pub session_id: &'a str,
    /// The digest of the session token the browser came back with.
    pub token_digest: &'a str,
    /// Now, in milliseconds since the Unix epoch: the session must not have ended.
    pub now: i64,
    /// The session must have been used after this time.
    pub idle_since: i64,
}

/// An identity that is tied to an account at the end of a sign-in to link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Linked {
    /// The account.
    pub user_id: String,
    /// Whether this tied it; false if it was the account's already.
    pub newly: bool,
}

/// Why a sign-in to link tied nothing at its end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkRefusal {
    /// The provider is switched off, removed, or another one, since it began.
    ProviderGone,
    /// The session it was started from has ended, or is not the one that came back.
    SessionEnded,
    /// The identity is another account's.
    IdentityTaken,
    /// The address the provider vouches for is not the account's own.
    AnotherAddress,
}

/// What came of removing an identity from an account.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unlinked {
    /// It is removed.
    Removed,
    /// It is not the account's.
    NotTied,
    /// It stays: the account has no password, and no other identity.
    LastWayIn,
    /// The account was deleted, or its password changed, since it was read.
    AccountChanged,
}
