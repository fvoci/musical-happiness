//! Invitations: making the link that lets somebody into a workspace, and letting in
//! whoever comes with one.
//!
//! The token of a link is made here, told once to who makes the link, and kept as its
//! digest. A link lets somebody in while it is live (`fvoci_store::Store::live_invitation`)
//! and while who made it may still invite: that is asked anew every time the link is
//! used, of the facts every other check reads (`App::actor`).

use std::net::IpAddr;

use fvoci_model::{
    AcceptedInvitation, INVITATION_LIFETIME_MILLIS, Invitation, InvitationPreview, NewAccount,
    WorkspaceRole,
};
use fvoci_store::{Acceptor, NewLink, StoreError};

use super::App;
use super::auth::account_error;
use crate::auth::now_millis;
use crate::auth::secret::{new_secret, token_digest};
use crate::error::AppError;
use crate::permissions::{self, WorkspacePermission};

/// Who accepts a link.
#[derive(Debug, Clone, Copy)]
pub enum Accepting<'a> {
    /// The user of the request's session.
    SignedIn {
        /// The identifier of the user.
        user_id: &'a str,
    },
    /// Somebody without an account, who asks for this one. The caller has checked and
    /// tidied it.
    NewAccount(&'a NewAccount),
}

impl App {
    /// Makes a link that lets whoever accepts it into the workspace as a member, for
    /// seven days and as often as it is used, and returns it with its token: the one
    /// time the token is told. The handle is the one the user entered the workspace
    /// with, and the caller has asked whether they may invite.
    ///
    /// # Errors
    ///
    /// Returns a 404 if the workspace is not in use, and the store's error if the link
    /// cannot be stored.
    pub async fn invite_members(
        &self,
        workspace_id: &str,
        created_by: &str,
    ) -> Result<Invitation, AppError> {
        let token = new_secret();
        let now = now_millis();
        let link = NewLink {
            workspace_id,
            guest_of: None,
            token_hash: &token_digest(&token),
            created_by,
            now,
            expires_at: now.saturating_add(INVITATION_LIFETIME_MILLIS),
            max_uses: None,
        };
        let mut invitation = match self.store().create_invitation(&link).await {
            Ok(invitation) => invitation,
            Err(error) if error.is_not_found() => return Err(AppError::not_found("workspace")),
            Err(error) => {
                return Err(AppError::from(error).context("unable to store an invitation"));
            }
        };
        tracing::info!(
            invitation_id = %invitation.id,
            workspace_id,
            user_id = created_by,
            "a link for members was made"
        );
        invitation.token = Some(token);
        Ok(invitation)
    }

    /// The live link `token` is of, with the application behind the wall of the
    /// link's organization; `None` if the token is of no link that lets anybody in:
    /// no link at all, one that ended, was used up or revoked, or one whose maker may
    /// no longer invite.
    async fn usable_invitation(&self, token: &str) -> Result<Option<(Self, Invitation)>, AppError> {
        let token_hash = token_digest(token);
        let now = now_millis();
        let Some(org_id) = self
            .store()
            .org_of_live_invitation(&token_hash, now)
            .await?
        else {
            return Ok(None);
        };
        let Some(app) = self.enter_org_for_sign_in(&org_id) else {
            return Err(AppError::internal(
                "the organization of an invitation as the store has it is no identifier",
            ));
        };
        let Some(invitation) = app.store().live_invitation(&token_hash, now).await? else {
            return Ok(None);
        };
        if !app.inviter_may_still_invite(&invitation).await? {
            return Ok(None);
        }
        Ok(Some((app, invitation)))
    }

    /// Whether who made the link may make it now: a link outlives nothing that its
    /// maker lost. A link for members is a workspace administrator's.
    async fn inviter_may_still_invite(&self, invitation: &Invitation) -> Result<bool, AppError> {
        let inviter = self
            .actor(&invitation.created_by, &invitation.workspace_id)
            .await?;
        Ok(match invitation.role {
            WorkspaceRole::Member => {
                permissions::allows_in_workspace(inviter, WorkspacePermission::InviteMembers)
            }
            WorkspaceRole::Admin | WorkspaceRole::Guest => false,
        })
    }

    /// What the link `token` is of shows to whoever holds it, signed in or not: the name
    /// of the workspace it leads into and of who made it, and nothing else of either.
    ///
    /// `address` is where the request comes from: a look at a link is counted against
    /// it as an acceptance is, so that tokens are not tried without end.
    ///
    /// # Errors
    ///
    /// Returns the 404 of [`App::accept_invitation`] for a token that lets nobody in,
    /// whatever is wrong with it, and a 429 for too many attempts from the address.
    pub async fn preview_invitation(
        &self,
        token: &str,
        address: Option<IpAddr>,
    ) -> Result<InvitationPreview, AppError> {
        let _attempt = self.admit_registration(address)?;
        let Some((app, invitation)) = self.usable_invitation(token).await? else {
            return Err(AppError::invitation_unavailable());
        };
        let workspace = app.workspace(&invitation.workspace_id).await?;
        let inviter = app.user(&invitation.created_by).await?;
        Ok(InvitationPreview {
            workspace_name: workspace.title,
            inviter_name: if inviter.display_name.is_empty() {
                inviter.username
            } else {
                inviter.display_name
            },
            board_title: None,
        })
    }

    /// Lets somebody in by the link `token` is of, and says where: the user of a
    /// session, or a new account that is made with it, in one transaction with the
    /// counting of the use. An account that has all the link gives already is let in
    /// again, and uses nothing of the link.
    ///
    /// `address` is where the request comes from: acceptances are counted against it
    /// as registrations are, and one of an address is under way at a time.
    ///
    /// # Errors
    ///
    /// Returns one and the same 404 (reason `invitation_unavailable`) for a token that
    /// lets nobody in, whatever is wrong with it; a conflict error if the name or the
    /// email address of a new account is taken; a 429 if there are too many attempts
    /// from the address; and the store's error if nothing could be stored.
    pub async fn accept_invitation(
        &self,
        token: &str,
        accepting: Accepting<'_>,
        address: Option<IpAddr>,
    ) -> Result<AcceptedInvitation, AppError> {
        let _attempt = self.admit_registration(address)?;
        let Some((app, _)) = self.usable_invitation(token).await? else {
            return Err(AppError::invitation_unavailable());
        };
        // Only now, for a link that is there, is a password hashed.
        let mut new_user = None;
        let acceptor = match accepting {
            Accepting::SignedIn { user_id } => Acceptor::Account(user_id),
            Accepting::NewAccount(account) => {
                Acceptor::NewAccount(new_user.insert(app.new_account(account).await?))
            }
        };
        let accepted = app
            .store()
            .accept_invitation(&token_digest(token), acceptor, now_millis())
            .await
            .map_err(|error| match error {
                error @ StoreError::Conflict(_) => account_error(error),
                error => AppError::from(error).context("unable to accept an invitation"),
            })?
            // Ended, used up or revoked since it was read.
            .ok_or_else(AppError::invitation_unavailable)?;
        if new_user.is_some() {
            self.metrics().registered();
        }
        tracing::info!(
            user_id = %accepted.user_id,
            workspace_id = %accepted.workspace_id,
            new_account = new_user.is_some(),
            used = accepted.used,
            "an invitation was accepted"
        );
        Ok(AcceptedInvitation {
            workspace_id: accepted.workspace_id,
            board_id: accepted.board_id,
        })
    }
}
