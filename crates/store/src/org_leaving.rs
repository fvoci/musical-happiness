//! An account leaving an organization that people work in together, at its own
//! asking: what it has there goes, and what it wrote stays.
//!
//! It is what erasing an account does in an organization that stays
//! (`account_erasure.rs`), but for two things: the organization keeps the account as
//! who made it, which is counted still (`organizations_made_by`), and a workspace the
//! account joined as the organization's administrator records the joining's end, as
//! leaving one does. Who administers what is asked and the account leaves in one
//! transaction, under the instance's row and then the organization's, as every change
//! of who administers the organization holds them: of two administrators who leave at
//! once, the second is asked once the first is out.

use fvoci_model::{OrgKind, OrgRole};

use crate::Store;
use crate::account_deletion::{self, Without};
use crate::account_erasure::{self, LeftFrom};
use crate::audit::AuditActor;
use crate::db::Scope;
use crate::error::StoreError;
use crate::{directory, org_admin, users};

/// What became of an account's asking to leave the handle's organization.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Departure {
    /// It is out of the organization: what it left there.
    Left(LeftFrom),
    /// It is no member of the organization, or the organization is none in use that
    /// people work in together: nothing changed.
    NotAMember,
    /// Something would be left without anybody to run it, or empty: nothing changed.
    Refused(DepartureRefused),
}

/// What stands in the way of an account's leaving an organization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DepartureRefused {
    /// The organization is the instance's own, and the account is the only
    /// administrator of the instance in use.
    LastInstanceAdmin,
    /// The account is the only administrator of the organization whose account is in
    /// use, and other people are in it.
    SoleAdmin,
    /// Nobody else is in the organization: it would be left empty.
    Alone,
}

impl Store {
    /// Takes the account `user_id` out of the handle's organization at its own asking,
    /// unless something stands in the way ([`DepartureRefused`], asked first in that
    /// order): its joinings of workspaces as the organization's administrator end,
    /// each recorded in its workspace; then it leaves each workspace of the
    /// organization, in use or in the trash, as an erased account does, a board it was
    /// the only admin of passing to the oldest other administrator of the workspace on
    /// their own account (recorded as the account's doing); then its roles on boards
    /// left from before, what it follows, its membership of the organization and the
    /// links it made there that nobody used go. Who leaves the server's own
    /// organization runs the server no more, as one made its member does not
    /// (`users::follow_org_role`). Its sessions, its identities, and what it wrote
    /// stay, and so does the organization's word of who made it. All of it happens or
    /// none.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NoOrganization`] if the handle names none, and
    /// [`StoreError::Database`] if a statement fails; nothing is changed then.
    pub async fn leave_organization(&self, user_id: &str) -> Result<Departure, StoreError> {
        self.transaction(async |scope| {
            // The instance's row first, as a change of who administers the instance
            // takes it, then the organization's, as a change of its roles does.
            users::lock_instance(scope).await?;
            org_admin::hold_organization_row(scope).await?;
            if !is_shared_and_in_use(scope).await? {
                return Ok(Departure::NotAMember);
            }
            let Some(role) = org_admin::org_role_of(scope, user_id).await? else {
                return Ok(Departure::NotAMember);
            };
            // The instance comes first, as a refused deletion is told.
            let the_instances = directory::is_the_instances_organization(scope).await?;
            if the_instances && account_deletion::is_last_instance_admin(scope, user_id).await? {
                return Ok(Departure::Refused(DepartureRefused::LastInstanceAdmin));
            }
            if let Some(refused) = refusal(scope, user_id, role).await? {
                return Ok(Departure::Refused(refused));
            }
            // The administrators of the server's own organization are the server's for
            // now (`members::follow_instance_role`): who leaves it, as who is made its
            // member, does not run the server, or they would make anybody its
            // administrator again from outside it.
            if the_instances && !users::follow_org_role(scope, user_id, OrgRole::Member).await? {
                return Ok(Departure::Refused(DepartureRefused::LastInstanceAdmin));
            }
            let joined = org_admin::leave_joinings(scope, user_id).await?;
            let actor = AuditActor::Person(user_id.to_owned());
            let mut left = LeftFrom::default();
            account_erasure::leave_every_workspace(scope, user_id, &actor, &mut left).await?;
            account_erasure::leave_organization(scope, user_id).await?;
            // A workspace they were in by a joining alone is one they left too.
            left.left.extend(joined);
            left.left.sort_unstable();
            left.left.dedup();
            Ok(Departure::Left(left))
        })
        .await
    }
}

/// Whether the scope's organization is in use and one that people work in together:
/// of a user's own space, and of the system's, nobody is a member to leave.
async fn is_shared_and_in_use(scope: &mut Scope<'_>) -> Result<bool, StoreError> {
    let mut sql = scope.sql("SELECT 1 FROM organizations WHERE id = ");
    sql.bind(scope.org()?)
        .push_bind(" AND kind = ", OrgKind::Team.as_str())
        .push(" AND delete_at = 0 AND is_system = false");
    Ok(scope.fetch_optional(sql).await?.is_some())
}

/// What of the organization stands in the way of the user's leaving it, in which they
/// are `role`: the questions deleting the account asks of it
/// (`account_deletion::standing`), in the order a refused deletion is told in.
async fn refusal(
    scope: &mut Scope<'_>,
    user_id: &str,
    role: OrgRole,
) -> Result<Option<DepartureRefused>, StoreError> {
    let org_id = scope.org_id().ok_or(StoreError::NoOrganization)?;
    Ok(
        match (
            account_deletion::without(scope, &org_id, user_id).await?,
            role,
        ) {
            (Without::Nobody, _) => Some(DepartureRefused::Alone),
            (Without::NoAdministrator, OrgRole::Admin) => Some(DepartureRefused::SoleAdmin),
            // A member leaves an organization nobody administers as it is.
            (Without::NoAdministrator, OrgRole::Member) | (Without::Administered, _) => None,
        },
    )
}
