//! An organization that takes no password: the one check every way into what is an
//! organization's passes, and what switches it on and off.
//!
//! The check is [`require_session_opens`]. A workspace is entered through it
//! (`App::enter_workspace`), an organization (`App::enter_org_as_admin`), and the
//! organization the two routes that name neither work in
//! (`App::require_session_opens_here`). It is asked of every request: the sessions
//! stay, and one that does not open an organization still opens the others.

use fvoci_model::Session;
use fvoci_store::StoredProvider;

use super::App;
use crate::error::{AppError, Reason};
use crate::permissions::session_opens;

/// What was done to an organization's taking no password.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SsoChange {
    /// An administrator switched it on.
    Required,
    /// An administrator switched it off.
    Released,
    /// Who runs the server switched it off, on the administration socket.
    ReleasedByOperator,
}

impl SsoChange {
    fn as_str(self) -> &'static str {
        match self {
            Self::Required => "sso_required",
            Self::Released => "sso_released",
            Self::ReleasedByOperator => "sso_released_by_operator",
        }
    }
}

/// Writes down that an organization began or stopped taking no password: the one
/// place it is recorded. `user_id` is the administrator who did it, `None` for who
/// runs the server.
///
/// TEMPORARY: a line of the log, under the target `audit`. Its place is the
/// organization's records (`audit_log`), which take it once a record may be of who
/// runs the server: `actor_id` is a user's there, and a release on the administration
/// socket has none.
fn record(provider: &StoredProvider, change: SsoChange, user_id: Option<&str>) {
    tracing::info!(
        target: "audit",
        org_id = %provider.org_id,
        provider_id = %provider.id,
        issuer = %provider.issuer,
        user_id = user_id.unwrap_or_default(),
        action = change.as_str(),
        "an organization's taking no password was changed"
    );
}

/// Records that the administrator `user_id` switched an organization's taking no
/// password on or off, if the provider as stored differs in that from the one before.
/// What is recorded is the provider it was switched on for, and the one it was
/// switched off for: the one before, where the same change set another.
pub(crate) fn record_if_changed(
    before: Option<&StoredProvider>,
    stored: &StoredProvider,
    user_id: &str,
) {
    match (
        before.filter(|before| before.sso_required),
        stored.sso_required,
    ) {
        (None, true) => record(stored, SsoChange::Required, Some(user_id)),
        (Some(before), false) => record(before, SsoChange::Released, Some(user_id)),
        _ => {}
    }
}

/// The refusal of a change to the provider that an organization that takes no
/// password does not let be made.
pub(crate) fn enforced() -> AppError {
    AppError::conflict(
        "the organization takes no password: its provider is not switched off, removed \
         or set to another issuer or client until ssoRequired is off",
    )
    .with_reason(Reason::SsoEnforced)
}

/// Checks a change of an organization's provider, from `existing` (as it was read) to
/// `next`, against what keeps the organization from locking itself out.
///
/// - Taking no password is switched on by a session that was made through this very
///   provider, which is on and stays on: who asks has shown that it lets them in. A
///   provider of another issuer or client is another provider, which nobody has
///   signed in through yet.
/// - While it is on and stays on, the provider is not switched off or set to another
///   issuer or client. Switching it off (`ssoRequired` false) comes first, or with
///   the change.
///
/// # Errors
///
/// Returns 409 with the reason `sso_unproven` for the first and `sso_enforced` for
/// the second.
pub(crate) fn check_change(
    session: &Session,
    existing: Option<&StoredProvider>,
    next: &StoredProvider,
) -> Result<(), AppError> {
    if !next.sso_required {
        return Ok(());
    }
    let same_provider = existing
        .filter(|existing| existing.issuer == next.issuer && existing.client_id == next.client_id);
    if let Some(held) = existing.filter(|existing| existing.sso_required) {
        // Named: what of the request would let nobody in.
        let refused = if held.issuer != next.issuer {
            "issuer"
        } else if held.client_id != next.client_id {
            "clientId"
        } else if !next.enabled {
            "enabled"
        } else {
            return Ok(());
        };
        return Err(enforced().with_field(refused));
    }
    let proven = same_provider.is_some_and(|provider| {
        provider.enabled
            && next.enabled
            && session.login_provider_id.as_deref() == Some(provider.id.as_str())
    });
    if proven {
        Ok(())
    } else {
        Err(AppError::conflict(
            "ssoRequired is switched on from a session made through this provider, \
             switched on: sign in through it first",
        )
        .with_reason(Reason::SsoUnproven)
        .with_field("ssoRequired"))
    }
}

/// Refuses a session that does not open what is the organization `org_id`'s:
/// `required` is the provider the organization takes sessions of and no other, as
/// the store answered it for this user, `None` if it takes a password too.
///
/// # Errors
///
/// Returns 401 with the reason `sso_required`, naming the organization. It is for
/// who has a place in the organization only: the caller asks after it knows the
/// user has one, so that the answer tells nobody else that the organization is
/// there.
pub(crate) fn require_session_opens(
    session: &Session,
    org_id: &str,
    required: Option<&str>,
) -> Result<(), AppError> {
    if session_opens(required, session.login_provider_id.as_deref()) {
        return Ok(());
    }
    tracing::debug!(
        org_id,
        user_id = %session.user_id,
        "a session not made through the organization's provider was refused"
    );
    Err(AppError::sso_required(org_id))
}

impl App {
    /// Releases the organization `org_id` from taking no password, for who runs the
    /// server: the way back in when nobody gets in through the provider. Its provider
    /// stays as it is otherwise. Releasing one that takes a password already changes
    /// nothing.
    ///
    /// # Errors
    ///
    /// Returns 404 for an organization that is not there, not in use, or the
    /// system's, and the store's error.
    pub async fn release_sso_as_operator(&self, org_id: &str) -> Result<(), AppError> {
        let not_found = || AppError::not_found("organization");
        if !fvoci_model::is_id(org_id)
            || self
                .store()
                .login_organization(Some(org_id))
                .await?
                .is_none()
        {
            return Err(not_found());
        }
        let org = self.enter_org_for_sign_in(org_id).ok_or_else(not_found)?;
        if let Some(provider) = org.store().release_sso().await? {
            record(&provider, SsoChange::ReleasedByOperator, None);
        }
        Ok(())
    }

    /// Closes the websocket connections that the handle's organization shuts out as it
    /// takes only sessions made through `provider`: what a connection follows of a
    /// workspace was asked when it subscribed. For after that is stored; also when it
    /// was on already, which closes again what a failure may have left.
    ///
    /// If the organization's workspaces cannot be read, every connection that follows a
    /// workspace with a session made otherwise is closed, whose workspace it may be:
    /// the pages connect again, and are asked then.
    pub(crate) async fn close_connections_shut_out(&self, provider: &StoredProvider) {
        match self.store().get_workspaces().await {
            Ok(workspaces) => {
                let ids: Vec<String> = workspaces
                    .into_iter()
                    .map(|workspace| workspace.id)
                    .collect();
                self.ws().close_sessions_closed_out(&ids, &provider.id);
            }
            Err(error) => {
                tracing::warn!(%error, org_id = %provider.org_id, "unable to read the workspaces of an organization that takes no password");
                self.ws().close_sessions_made_otherwise(&provider.id);
            }
        }
    }

    /// Refuses a session that does not open what is the organization this handle
    /// stands in: the one of the routes that name no workspace and no organization and
    /// still read or change an organization's (`getMyMemberships`, `setUserAdmin`).
    /// `Ok` for a session that opens it, and for a user who has no place in that
    /// organization, who finds nothing of it anyway.
    ///
    /// TEMPORARY, with `only_organization_scope`: a route that names its workspace or
    /// its organization is asked where that is entered.
    ///
    /// # Errors
    ///
    /// Returns 401 with the reason `sso_required`; the store's error if the
    /// organization's provider cannot be read, which is no "yes"; and a server error
    /// for a handle that stands in no organization.
    pub(crate) async fn require_session_opens_here(
        &self,
        session: &Session,
    ) -> Result<(), AppError> {
        // A handle of no organization, or of all, is no route's: nothing is opened
        // for want of an organization to ask.
        let Some(org_id) = self.store().tenant().org_id() else {
            return Err(AppError::internal(
                "a route that names no workspace stands in no organization",
            ));
        };
        let required = self
            .store()
            .required_provider_for(&session.user_id, &org_id)
            .await?;
        require_session_opens(session, &org_id, required.as_deref())
    }
}
