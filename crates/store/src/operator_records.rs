//! What the server's operator does to a person's password, recorded in every
//! organization the person is a member of: one record each, as the operator's, in the
//! transaction of what it records. The one place besides the directory that writes
//! across organizations, with the system's reach, narrowed by the person's
//! memberships.

use fvoci_model::AuditAction;

use crate::audit::{self, AuditActor, AuditEntry};
use crate::db::{Id, Scope};
use crate::error::StoreError;
use crate::{Store, users};

impl Store {
    /// Sets the password hash of the user with the given name, in any letter case, and
    /// deletes their sessions, so that nobody stays signed in with the old password. For
    /// a password the server's operator sets: recorded as theirs in every organization
    /// the user is a member of ([`AuditAction::PasswordSetByOperator`]), with no word
    /// of the password. Returns the identifier of that user.
    ///
    /// All of it happens or none.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::UserNotFound`] if no user has that name.
    pub async fn reset_user_password(
        &self,
        username: &str,
        password_hash: &str,
    ) -> Result<String, StoreError> {
        self.system()
            .transaction(async |scope| {
                let user_id = users::reset_password(scope, username, password_hash).await?;
                record_in_organizations_of(scope, &user_id, AuditAction::PasswordSetByOperator)
                    .await?;
                Ok(user_id)
            })
            .await
    }

    /// Records that the server's operator made a link to set the password of the user
    /// `user_id` anew ([`AuditAction::PasswordResetLinkMade`]), in every organization
    /// they are a member of: one record each, with neither the link nor its token, and
    /// not which organization asked. Returns in how many organizations it was recorded.
    /// For a link stored elsewhere; a store function that stores the link records it in
    /// the same transaction with `record_in_organizations_of`.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if a statement fails.
    pub async fn record_password_reset_link_made(
        &self,
        user_id: &str,
    ) -> Result<usize, StoreError> {
        self.system()
            .transaction(async |scope| {
                record_in_organizations_of(scope, user_id, AuditAction::PasswordResetLinkMade).await
            })
            .await
    }
}

/// Records `action` by the server's operator, done to the user `user_id`, in every
/// organization in use that they are a member of, one record each, of no workspace;
/// returns how many. Needs the system's reach: the wall hides the other organizations.
/// What stores what is recorded (a reset link's token) calls it in its own
/// transaction, so that neither is kept without the other.
pub(crate) async fn record_in_organizations_of(
    scope: &mut Scope<'_>,
    user_id: &str,
    action: AuditAction,
) -> Result<usize, StoreError> {
    let mut sql = scope.sql(
        "SELECT m.org_id FROM organization_members AS m \
         JOIN organizations AS o ON o.id = m.org_id WHERE m.user_id = ",
    );
    sql.bind(Id(user_id))
        .push(" AND o.delete_at = 0 AND o.is_system = false ORDER BY m.org_id");
    let organizations = scope
        .fetch_all(sql)
        .await?
        .iter()
        .map(|row| row.id(0))
        .collect::<Result<Vec<_>, _>>()?;
    for org_id in &organizations {
        let entry =
            AuditEntry::by(&AuditActor::ServerOperator, "", action, org_id).with_subject(user_id);
        audit::record_entry_in(scope, org_id, &entry).await?;
    }
    Ok(organizations.len())
}
