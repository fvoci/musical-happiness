//! Starting an organization from an account that is there already.

use std::net::IpAddr;

use fvoci_model::{OrgKind, Session, UserWorkspace, WorkspaceRole};

use super::App;
use crate::error::AppError;

impl App {
    /// Makes an organization that people work in together, and its first workspace,
    /// both under `name`, for the account of `session`, which administers both.
    /// Returns the workspace as the account is listed it. The caller has read the name
    /// as one is stored (`NewOrganization::stored_name`).
    ///
    /// # Errors
    ///
    /// Returns a permission error (reason `invitation_required`) on a server that
    /// takes one organization; 400 with the reason `too_many_organizations` and the
    /// limit for an account that made as many as one may ([`App::account_limits`]); a
    /// 429 if there are too many registrations from the address, with which this is
    /// counted; and the store's error if the organization cannot be stored.
    pub async fn start_organization(
        &self,
        session: &Session,
        name: &str,
        address: Option<IpAddr>,
    ) -> Result<UserWorkspace, AppError> {
        // Where one organization is taken, the first user started it by registering.
        if !self.config().takes_several_organizations() {
            return Err(AppError::invitation_required());
        }
        // An organization is what a registration makes, and is counted against where
        // it comes from as one is: a session is not a way around that count.
        let _turn = self.admit_registration(address)?;
        let started = self
            .store()
            .start_organization(
                &session.user_id,
                name,
                self.account_limits().organizations_made,
            )
            .await?;
        tracing::info!(
            user_id = %session.user_id,
            org_id = %started.org_id,
            "an account started an organization"
        );
        Ok(UserWorkspace {
            id: started.workspace_id,
            org_id: started.org_id,
            kind: OrgKind::Team,
            name: name.to_owned(),
            role: WorkspaceRole::Admin,
            // An organization that was just made has no provider to require.
            sso_required: false,
            password_required: false,
        })
    }
}
