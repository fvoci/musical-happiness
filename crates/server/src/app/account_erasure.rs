//! Erasing accounts: those whose wait ran out, by the housekeeping, and one at once on
//! the operator's word.

use fvoci_model::{ContainerMember, ContainerRole};
use fvoci_store::{DeletionStanding, ErasedFrom, Erasing, ErasureEnd, ErasureStart};

use super::App;
use crate::auth::now_millis;
use crate::collab::AccessScope;
use crate::error::AppError;

/// What became of erasing an account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Erasure {
    /// It is erased: nothing of the person is kept, and what they wrote stays.
    Erased,
    /// There is no such account, or it was erased before.
    NotThere,
    /// Its wait has not run out.
    Waits,
    /// It is the instance's only administrator, or the only one of an organization
    /// that has other people: nothing changed.
    Refused(DeletionStanding),
    /// Its erasing began and did not end: the account still has a place somewhere.
    /// The housekeeping takes it up again.
    Unfinished,
}

impl App {
    /// Erases the account `user_id`: begins it if it is to be erased
    /// ([`fvoci_store::Store::begin_account_erasure`]), takes the account out of each
    /// organization it has a place in, an organization that has nobody else going with
    /// it, and ends it, leaving of the account its identifier. One whose erasing began
    /// before is taken up where it stopped. It takes the housekeeping's reach itself.
    ///
    /// What the account had open is closed; who a board passed to is told to who is
    /// on it, and the rooms there ask about them again.
    ///
    /// # Errors
    ///
    /// Returns the store's error if a step fails: what was done stays done, and the
    /// next call goes on from there.
    pub async fn erase_account(
        &self,
        user_id: &str,
        erasing: Erasing,
    ) -> Result<Erasure, AppError> {
        let app = self.housekeeping();
        let begun = app
            .store()
            .begin_account_erasure(user_id, now_millis(), erasing)
            .await
            .map_err(|error| AppError::from(error).context("beginning to erase an account"))?;
        match begun {
            ErasureStart::Begun => {}
            ErasureStart::NotThere => return Ok(Erasure::NotThere),
            ErasureStart::Waits => return Ok(Erasure::Waits),
            ErasureStart::Refused(standing) => return Ok(Erasure::Refused(standing)),
        }
        // What the account had open ends with its sessions.
        self.ws().close_user(user_id);
        let organizations = app
            .store()
            .organizations_of_account(user_id)
            .await
            .map_err(|error| {
                AppError::from(error).context("reading the organizations of an account to erase")
            })?;
        for org_id in organizations {
            let org = self.in_organization(&org_id)?;
            let erased = org
                .store()
                .erase_account_from_organization(user_id, erasing)
                .await
                .map_err(|error| {
                    AppError::from(error).context("erasing an account from an organization")
                })?;
            org.after_erasure_from(&org_id, user_id, erased).await;
        }
        let finished = app
            .store()
            .finish_account_erasure(user_id, now_millis())
            .await
            .map_err(|error| AppError::from(error).context("ending the erasing of an account"))?;
        match finished {
            ErasureEnd::Erased => {
                tracing::info!(user_id, "an account was erased");
                Ok(Erasure::Erased)
            }
            // Another run ended it meanwhile: the housekeeping's, or the operator's.
            ErasureEnd::ErasedBefore => Ok(Erasure::Erased),
            ErasureEnd::StillPlaced | ErasureEnd::NotBegun => Ok(Erasure::Unfinished),
        }
    }

    /// Erases the account with the login name at once, on the word of the server's
    /// operator, whether its person asked or not, and returns its identifier. An
    /// erasing that began before is taken up.
    ///
    /// # Errors
    ///
    /// Returns a 404 if no account has the name (one that was erased has none), with
    /// the identifier `None`; the 409 of a refused asking (`account_deletion::refused`)
    /// for the instance's only administrator and the only administrator of an
    /// organization that has other people; a 409 if the erasing did not end, which
    /// the housekeeping takes up; and the store's error.
    pub async fn erase_account_as_operator(
        &self,
        username: &str,
    ) -> (Option<String>, Result<(), AppError>) {
        let user_id = match self.store().account_id_by_username(username).await {
            Ok(Some(user_id)) => user_id,
            Ok(None) => return (None, Err(AppError::not_found("user"))),
            Err(error) => return (None, Err(error.into())),
        };
        let erased = match self.erase_account(&user_id, Erasing::AtOnce).await {
            Ok(Erasure::Erased) => Ok(()),
            Ok(Erasure::NotThere) => Err(AppError::not_found("user")),
            Ok(Erasure::Refused(standing)) => Err(super::account_deletion::refused(&standing)),
            Ok(Erasure::Waits | Erasure::Unfinished) => Err(AppError::conflict(
                "the erasing of the account did not end; it is taken up again",
            )),
            Err(error) => Err(error),
        };
        (Some(user_id), erased)
    }

    /// What follows an account's leaving the handle's organization: the rooms of the
    /// workspaces it left ask about it again, as for a member taken out; the boards
    /// that passed to somebody are announced to who is on them, and their rooms ask
    /// about the heir, who may write there now; an organization that went, and what
    /// stays without an administrator (an organization, a workspace, a board), are
    /// logged.
    async fn after_erasure_from(&self, org_id: &str, user_id: &str, erased: ErasedFrom) {
        if erased.kept_for_newcomer {
            tracing::warn!(
                org_id,
                user_id,
                "an organization that was to go with the account of its only person has \
                 somebody else in it, and stays, without an administrator: make somebody \
                 its administrator"
            );
        }
        for workspace_id in &erased.without_administrator {
            tracing::warn!(
                org_id,
                workspace_id,
                user_id,
                "a workspace has no administrator: the account that was erased was its only \
                 one. An administrator of the organization can join it and name one"
            );
        }
        for container_id in &erased.without_admin {
            tracing::warn!(
                org_id,
                container_id,
                user_id,
                "a board has no admin: the account that was erased was its only one, and its \
                 workspace has no other administrator to pass it to"
            );
        }
        for workspace_id in &erased.left {
            self.ws().workspace_role_changed(workspace_id, user_id);
            self.collab()
                .recheck_access(self, AccessScope::workspace(workspace_id).of_user(user_id))
                .await;
        }
        if let Some(removed) = &erased.removed {
            tracing::info!(
                org_id,
                user_id,
                records = removed.records,
                files = removed.files.len(),
                "an organization went with the account of its only person"
            );
        }
        for passed in erased.handed_over {
            tracing::info!(
                org_id,
                workspace_id = %passed.workspace_id,
                container_id = %passed.container_id,
                heir_id = %passed.heir_id,
                "a board passed to an administrator of its workspace as its admin's account was erased"
            );
            let admin = ContainerMember {
                container_id: passed.container_id.clone(),
                user_id: passed.heir_id.clone(),
                ..ContainerMember::default()
            }
            .with_role(Some(ContainerRole::Admin));
            self.broadcast_member_change_in_background(passed.workspace_id, admin);
            self.collab()
                .recheck_access(
                    self,
                    AccessScope::container(&passed.container_id).of_user(&passed.heir_id),
                )
                .await;
        }
    }

    /// Erases every account whose wait ran out, and takes up the erasings that were
    /// cut off, each as [`App::erase_account`] does. An account that is not erased
    /// because something would be left without an administrator is logged for the
    /// operator and asked again at the next run; so is one that failed.
    pub async fn erase_due_accounts(&self) {
        let app = self.housekeeping();
        if let Err(error) = app.require_server_lock() {
            tracing::warn!(%error, "the accounts that wait to be deleted are not erased");
            return;
        }
        let due = match app.store().accounts_to_erase(now_millis()).await {
            Ok(due) => due,
            Err(error) => {
                tracing::error!(%error, "unable to read the accounts to erase");
                return;
            }
        };
        for user_id in due {
            match app.erase_account(&user_id, Erasing::AfterTheWait).await {
                Ok(Erasure::Erased | Erasure::NotThere | Erasure::Waits) => {}
                Ok(Erasure::Refused(standing)) => {
                    let organizations: Vec<&str> = standing
                        .sole_admin_of
                        .iter()
                        .map(|organization| organization.id.as_str())
                        .collect();
                    tracing::warn!(
                        user_id,
                        ?organizations,
                        last_instance_admin = standing.last_instance_admin,
                        "an account that waits to be deleted is not erased: it is the only \
                         administrator of what would be left without one. It is asked again at \
                         the next run"
                    );
                }
                Ok(Erasure::Unfinished) => {
                    tracing::warn!(
                        user_id,
                        "the erasing of an account did not end; it is taken up again"
                    );
                }
                Err(error) => {
                    tracing::error!(%error, user_id, "unable to erase an account");
                }
            }
        }
    }
}
