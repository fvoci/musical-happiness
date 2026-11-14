//! The account of somebody who signs in through a provider for the first time: made
//! under a user name of what the provider says, a member of the organization. The
//! sign-in it ends is in `oidc.rs`.

use fvoci_model::{Session, User, display_name, new_id};
use fvoci_store::{Applied, Identity, OidcAccount, StoreError, StoredProvider, Taken};

use super::App;
use super::oidc::{Ending, LoginFailure, logged, refused};
use crate::auth::oidc::provider::IdClaims;
use crate::auth::secret::new_secret;
use crate::error::Reason;

/// The longest user name made from what a provider says.
const USERNAME_MAX_CHARS: usize = 64;

impl App {
    /// Makes the account of a person who signs in for the first time, a member of the
    /// handle's organization and its default workspace, and signs in to it.
    #[expect(
        clippy::too_many_arguments,
        reason = "the sign-in's parts, each of its own kind, passed down once"
    )]
    pub(super) async fn make_account(
        &self,
        claims: &IdClaims,
        identity: Identity<'_>,
        email: &str,
        stored: &StoredProvider,
        ending: Ending<'_>,
        session: &mut Session,
        digest: &str,
    ) -> Result<Option<Applied>, LoginFailure> {
        let base = username_base(claims, email);
        for attempt in 0..3 {
            let username = if attempt == 0 {
                base.clone()
            } else {
                format!("{base}-{}", new_secret()[..6].to_ascii_lowercase())
            };
            match self.store().get_user_by_username(&username).await {
                Ok(_) => continue,
                Err(error) if error.is_not_found() => {}
                Err(error) => return Err(logged(&error, "looking up a user name")),
            }
            // No address of its own: the provider's is kept with the identity.
            let mut user = User {
                id: new_id(),
                username,
                display_name: claims
                    .name
                    .as_deref()
                    .and_then(|name| display_name(name).ok())
                    .unwrap_or_default(),
                ..User::default()
            };
            let account = OidcAccount::New {
                user: &mut user,
                identity,
                provider_email: email,
            };
            match self
                .finish_in_store(stored, ending, account, session, digest)
                .await
            {
                Ok(Ok((_, applied))) => {
                    self.metrics().registered();
                    tracing::info!(user_id = %user.id, org_id = %stored.org_id, issuer = %stored.issuer, "an account was made at its first sign-in through a provider, and joined the organization");
                    return Ok(applied);
                }
                Ok(Err(refusal)) => return Err(refused(refusal, stored)),
                Err(StoreError::Conflict(Taken::Username)) => {}
                // Another sign-in of the same person made the account a moment ago.
                Err(StoreError::Conflict(Taken::Identity)) => {
                    let user = self
                        .store()
                        .user_of_identity(identity)
                        .await
                        .map_err(|error| logged(&error, "looking up an identity"))?
                        .ok_or(Reason::OidcUnavailable)?;
                    return self
                        .let_in(&user, identity, stored, ending, session, digest)
                        .await;
                }
                Err(error) => return Err(logged(&error, "storing a new account")),
            }
        }
        tracing::warn!("no free user name was found for a new account");
        Err(Reason::OidcUnavailable.into())
    }
}

/// The user name of a new account, before it is seen to be free: what the provider
/// says the person goes by, or else the part of the email address before the `@`, of
/// letters, digits, `.`, `_` and `-` only.
fn username_base(claims: &IdClaims, email: &str) -> String {
    let clean = |text: &str| -> String {
        text.chars()
            .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
            .take(USERNAME_MAX_CHARS)
            .collect()
    };
    let local = email.split('@').next().unwrap_or_default();
    [
        claims.preferred_username.as_deref().unwrap_or_default(),
        local,
    ]
    .into_iter()
    .map(clean)
    .find(|name| !name.is_empty())
    .unwrap_or_else(|| "user".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claims(preferred: Option<&str>) -> IdClaims {
        IdClaims {
            issuer: String::new(),
            subject: String::new(),
            email: None,
            email_verified: None,
            name: None,
            preferred_username: preferred.map(str::to_owned),
        }
    }

    #[test]
    fn a_user_name_is_made_of_what_the_provider_says_or_of_the_address() {
        assert_eq!(username_base(&claims(Some("ada.l")), "x@y.z"), "ada.l");
        assert_eq!(username_base(&claims(Some("한글")), "ada+1@y.z"), "ada1");
        assert_eq!(username_base(&claims(None), "@y.z"), "user");
    }
}
