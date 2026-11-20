//! Starting an organization from an account that is there already.

use std::net::IpAddr;

use fvoci_model::{Session, UserWorkspace};

use super::App;
use crate::error::AppError;

impl App {
    /// Makes an organization that people work in together, and its first workspace,
    /// both under `name`, for the account of `session`, which administers both.
    /// Returns the workspace as the account is listed it.
    ///
    /// # Errors
    ///
    /// Returns a permission error (reason `invitation_required`) on a server that
    /// takes one organization.
    pub async fn start_organization(
        &self,
        session: &Session,
        name: &str,
        address: Option<IpAddr>,
    ) -> Result<UserWorkspace, AppError> {
        // The contract first: the store does not start an organization for an account
        // yet, and every server answers as one that takes one organization.
        let _turn = self.admit_registration(address)?;
        let user = self.store().get_user_by_id(&session.user_id).await?;
        tracing::debug!(user_id = %user.id, name, "an organization is not started yet");
        Err(AppError::invitation_required())
    }
}
