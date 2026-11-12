//! An organization that takes no password: the one check every way into what is an
//! organization's passes, and what switches it on and off.
//!
//! The check is [`App::require_session_opens`]. A workspace is entered through it
//! (`App::enter_workspace`), an organization (`App::enter_org_as_admin`), and the
//! organization the routes that name no workspace work in
//! (`App::require_session_opens_here`); the
//! websocket hub asks the same rule of a subscription. It is asked of every request:
//! the sessions stay, and one that does not open an organization still opens the
//! others.

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
/// TEMPORARY: a line of the log, under the target `audit`. The record of an
/// organization's changes is its place when there is one; `audit_log` holds a
/// workspace's alone.
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
pub(crate) fn record_if_changed(
    before: Option<&StoredProvider>,
    stored: &StoredProvider,
    user_id: &str,
) {
    let change = match (
        before.is_some_and(|before| before.sso_required),
        stored.sso_required,
    ) {
        (false, true) => SsoChange::Required,
        (true, false) => SsoChange::Released,
        _ => return,
    };
    record(stored, change, Some(user_id));
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
    if existing.is_some_and(|existing| existing.sso_required) {
        return if same_provider.is_some() && next.enabled {
            Ok(())
        } else {
            Err(enforced().with_field(if next.enabled { "issuer" } else { "enabled" }))
        };
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

impl App {
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

    /// Refuses a session that does not open what is the organization this handle
    /// stands in: the one of the routes that name no workspace, or the one of a board a
    /// read token opened. `Ok` for a session that opens it, and for a user who has no
    /// place in that organization, who finds nothing of it anyway.
    ///
    /// TEMPORARY, with `only_organization_scope`: a route that names its workspace is
    /// asked where the workspace is entered.
    ///
    /// # Errors
    ///
    /// Returns 401 with the reason `sso_required`, and the store's error if the
    /// organization's provider cannot be read: that is no "yes".
    pub(crate) async fn require_session_opens_here(
        &self,
        session: &Session,
    ) -> Result<(), AppError> {
        let Some(org_id) = self.store().tenant().org_id() else {
            return Ok(());
        };
        let required = self
            .store()
            .required_provider_for(&session.user_id, &org_id)
            .await?;
        Self::require_session_opens(session, &org_id, required.as_deref())
    }
}
