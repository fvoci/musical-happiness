//! Invitations: making the link that lets somebody into a workspace, and letting in
//! whoever comes with one.
//!
//! The token of a link is made here, told once to who makes the link, and kept as its
//! digest. A link lets somebody in while it is live (`fvoci_store::Store::live_invitation`)
//! and while who made it may still invite: that is asked anew every time the link is
//! used, of the facts every other check reads (`App::actor`), and an acceptance is
//! stored only if the maker is still what was found then, where it is written
//! (`fvoci_store::Inviter`).

mod mail;

use std::net::IpAddr;

use fvoci_model::{
    AcceptedInvitation, BoardMember, BoardRole, GUEST_INVITATION_USES, GuestInvitationsChange,
    INVITATION_LIFETIME_MILLIS, Invitation, InvitationPreview, NewAccount, Workspace,
    WorkspaceRole,
};
use fvoci_store::{Acceptance, Acceptor, Inviter, InviterOfBoard, NewLink, Standing, StoreError};

use super::auth::account_error;
use super::{App, Asker, BoardAccess};
use crate::auth::now_millis;
use crate::auth::secret::{new_secret, token_digest};
use crate::error::AppError;
use crate::permissions::{self, Actor, BoardScope, WorkspacePermission};

/// How often an acceptance is tried against a maker of the link who is changed
/// meanwhile.
const ATTEMPTS: usize = 3;

/// What the maker of a link was found to be when their link was found to let somebody
/// in: what the acceptance holds them to (`fvoci_store::Inviter`).
#[derive(Debug)]
struct Maker {
    workspace_role: Option<WorkspaceRole>,
    /// Their membership of the board of a guest's link.
    member: Option<BoardMember>,
    of_board: InviterOfBoard,
}

/// Who accepts a link.
#[derive(Debug, Clone, Copy)]
pub enum Accepting<'a> {
    /// The user of the request's session, and the provider it was made through.
    SignedIn(Asker<'a>),
    /// Somebody without an account, who asks for this one. The caller has checked and
    /// tidied it.
    NewAccount(&'a NewAccount),
}

/// What a new link is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Inviting<'a> {
    /// Members of the workspace: the link is used as often as it is until it ends.
    Members,
    /// A guest of one board of the workspace, in a role on it: the link is used once.
    Guest {
        /// The board.
        board_id: &'a str,
        /// The guest's role on the board.
        role: BoardRole,
    },
}

impl App {
    /// Makes a link that lets whoever accepts it into the workspace, for seven days,
    /// and returns it with its token: the one time the token is told. The handle is
    /// the one the user entered the workspace with, and the caller has asked whether
    /// they may invite so.
    ///
    /// # Errors
    ///
    /// Returns a 404 if the workspace is not in use or has no such board to invite a
    /// guest to, and the store's error if the link cannot be stored.
    pub async fn invite(
        &self,
        workspace_id: &str,
        created_by: &str,
        inviting: Inviting<'_>,
    ) -> Result<Invitation, AppError> {
        let token = new_secret();
        let now = now_millis();
        let (guest_of, max_uses) = match inviting {
            Inviting::Members => (None, None),
            Inviting::Guest { board_id, role } => {
                (Some((board_id, role)), Some(GUEST_INVITATION_USES))
            }
        };
        let link = NewLink {
            workspace_id,
            guest_of,
            token_hash: &token_digest(&token),
            created_by,
            now,
            expires_at: now.saturating_add(INVITATION_LIFETIME_MILLIS),
            max_uses,
        };
        let mut invitation = match self.store().create_invitation(&link).await {
            Ok(invitation) => invitation,
            Err(error) if error.is_not_found() => {
                return Err(AppError::not_found(match inviting {
                    Inviting::Members => "workspace",
                    Inviting::Guest { .. } => "board",
                }));
            }
            Err(error) => {
                return Err(AppError::from(error).context("unable to store an invitation"));
            }
        };
        tracing::info!(
            invitation_id = %invitation.id,
            workspace_id,
            board_id = invitation.board_id.as_deref().unwrap_or_default(),
            user_id = created_by,
            role = invitation.role.as_str(),
            "a link was made"
        );
        invitation.token = Some(token);
        Ok(invitation)
    }

    /// The live links into the workspace that the user sees: all of them for who
    /// invites its members, their own for anybody else. The newest first, and none
    /// with its token.
    ///
    /// # Errors
    ///
    /// Returns the store's error if the links cannot be read.
    pub async fn invitations(
        &self,
        workspace_id: &str,
        user_id: &str,
        actor: Actor,
    ) -> Result<Vec<Invitation>, AppError> {
        let own = (!sees_every_link(actor)).then_some(user_id);
        Ok(self
            .store()
            .live_invitations(workspace_id, own, now_millis())
            .await?)
    }

    /// Revokes a live link into the workspace: any of them for who invites its
    /// members, one of their own for anybody else. The link lets nobody in from then
    /// on.
    ///
    /// # Errors
    ///
    /// Returns one and the same 404 if there is no such live link, or it is not the
    /// user's to revoke.
    pub async fn revoke_invitation(
        &self,
        workspace_id: &str,
        invitation_id: &str,
        user_id: &str,
        actor: Actor,
    ) -> Result<(), AppError> {
        let own = (!sees_every_link(actor)).then_some(user_id);
        let revoked = fvoci_model::is_id(invitation_id)
            && self
                .store()
                .revoke_invitation(workspace_id, invitation_id, own, now_millis())
                .await?;
        if !revoked {
            return Err(AppError::not_found("invitation"));
        }
        tracing::info!(invitation_id, workspace_id, user_id, "a link was revoked");
        Ok(())
    }

    /// Turns the invitations of guests to the boards of the workspace on or off, on
    /// behalf of the user, and returns the workspace. Turned off, the links that invite
    /// guests are revoked with it, and stay so when it is turned on again; who is a
    /// guest already stays one. The caller has asked whether the user may.
    ///
    /// # Errors
    ///
    /// Returns a 404 if the workspace is not in use.
    pub async fn set_guest_invitations(
        &self,
        workspace_id: &str,
        change: GuestInvitationsChange,
        user_id: &str,
    ) -> Result<Workspace, AppError> {
        match self
            .store()
            .set_guest_invitations(workspace_id, change, user_id)
            .await
        {
            Ok(workspace) => {
                tracing::info!(
                    workspace_id,
                    user_id,
                    enabled = change.enabled,
                    "the invitations of guests were switched"
                );
                Ok(workspace)
            }
            Err(error) if error.is_not_found() => Err(AppError::not_found("workspace")),
            Err(error) => Err(error.into()),
        }
    }

    /// What the user is to a board of the workspace, and whether they invite guests to
    /// it (`permissions::invites_guests`): what making a link for a guest takes, and
    /// what such a link needs of its maker for as long as it is to let anybody in.
    /// `None` for no such board in the workspace.
    ///
    /// # Errors
    ///
    /// Returns the store's error if a fact cannot be read.
    pub async fn guest_inviter(
        &self,
        user_id: &str,
        workspace_id: &str,
        board_id: &str,
    ) -> Result<Option<(BoardAccess, bool)>, AppError> {
        let Some(access) = self.board_access(user_id, board_id).await? else {
            return Ok(None);
        };
        if access.board.workspace_id != workspace_id {
            return Ok(None);
        }
        let workspace = match self.store().get_workspace(workspace_id).await {
            Ok(workspace) => workspace,
            Err(error) if error.is_not_found() => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let invites =
            permissions::invites_guests(access.grant, BoardScope::of(&access.board), &workspace);
        Ok(Some((access, invites)))
    }

    /// The live link `token` is of, with the application behind the wall of the
    /// link's organization; `None` if the token is of no link that lets anybody in:
    /// no link at all, one that ended, was used up or revoked, or one whose maker may
    /// no longer invite.
    async fn usable_invitation(
        &self,
        token: &str,
    ) -> Result<Option<(Self, Invitation, Maker)>, AppError> {
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
        let Some(maker) = app.inviter_who_may_still_invite(&invitation).await? else {
            return Ok(None);
        };
        Ok(Some((app, invitation, maker)))
    }

    /// Who made the link, as they are now, if they may make it now: a link outlives
    /// nothing that its maker lost. A link for members is a workspace administrator's;
    /// one for a guest is of who adds members to its board, while the workspace invites
    /// guests.
    async fn inviter_who_may_still_invite(
        &self,
        invitation: &Invitation,
    ) -> Result<Option<Maker>, AppError> {
        let (maker, workspace_id) = (&invitation.created_by, &invitation.workspace_id);
        match (invitation.role, &invitation.board_id) {
            (WorkspaceRole::Member, _) => {
                let actor = self.actor(maker, workspace_id).await?;
                let invites =
                    permissions::allows_in_workspace(actor, WorkspacePermission::InviteMembers);
                Ok(invites.then_some(Maker {
                    workspace_role: actor.workspace,
                    member: None,
                    of_board: InviterOfBoard::Member,
                }))
            }
            (WorkspaceRole::Guest, Some(board_id)) => Ok(self
                .guest_inviter(maker, workspace_id, board_id)
                .await?
                .filter(|(_, invites)| *invites)
                .map(|(access, _)| Maker {
                    workspace_role: access.actor.workspace,
                    // A manager who does not read the board puts nobody on it off the
                    // record, by a link as little as by adding them.
                    of_board: if access.grant.manages_only() {
                        InviterOfBoard::ManagerOnly
                    } else {
                        InviterOfBoard::Member
                    },
                    member: access.member,
                })),
            (WorkspaceRole::Guest, None) | (WorkspaceRole::Admin, _) => Ok(None),
        }
    }

    /// What the link `token` is of shows to whoever holds it, signed in or not: the name
    /// of the workspace it leads into and of who made it, for a guest the title of the
    /// board, and nothing else of any of them.
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
        let Some((app, invitation, _)) = self.usable_invitation(token).await? else {
            return Err(AppError::invitation_unavailable());
        };
        let workspace = app.workspace(&invitation.workspace_id).await?;
        let inviter = app.user(&invitation.created_by).await?;
        let board_title = match &invitation.board_id {
            Some(board_id) => match app.store().get_board(board_id).await {
                Ok(board) => Some(board.title),
                // Gone since the link was read: with it the link.
                Err(error) if error.is_not_found() => {
                    return Err(AppError::invitation_unavailable());
                }
                Err(error) => return Err(error.into()),
            },
            None => None,
        };
        Ok(InvitationPreview {
            workspace_name: workspace.title,
            inviter_name: if inviter.display_name.is_empty() {
                inviter.username
            } else {
                inviter.display_name
            },
            board_title,
        })
    }

    /// Lets somebody in by the link `token` is of, and says where: the user of a
    /// session, or a new account that is made with it, in one transaction with the
    /// counting of the use. An account that has all the link gives already is let in
    /// again, and uses nothing of the link.
    ///
    /// The maker of the link is looked at again where the acceptance is written, once
    /// the link is held: a link whose maker was taken out of the workspace while the
    /// acceptance waited lets nobody in, as a look at it says by then.
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
        let mut new_user = None;
        let mut settled = None;
        for _ in 0..ATTEMPTS {
            let Some((app, invitation, maker)) = self.usable_invitation(token).await? else {
                return Err(AppError::invitation_unavailable());
            };
            // Only now, for a link that is there, is a password hashed, and once.
            let acceptor = match accepting {
                Accepting::SignedIn(asker) => {
                    // Who holds the link knows the organization it leads into: a
                    // session that organization does not take is told so, and let in
                    // nowhere.
                    app.require_session_opens_organization(asker).await?;
                    Acceptor::Account(asker.user_id)
                }
                Accepting::NewAccount(account) => match &mut new_user {
                    Some(user) => Acceptor::NewAccount(user),
                    empty => Acceptor::NewAccount(empty.insert(app.new_account(account).await?)),
                },
            };
            let inviter = Inviter {
                standing: Standing {
                    user_id: &invitation.created_by,
                    workspace_role: maker.workspace_role,
                    member: maker.member.as_ref(),
                },
                of_board: maker.of_board,
            };
            let acceptance = app
                .store()
                .accept_invitation(&token_digest(token), acceptor, now_millis(), inviter)
                .await
                .map_err(|error| match error {
                    error @ StoreError::Conflict(_) => account_error(error),
                    error => AppError::from(error).context("unable to accept an invitation"),
                })?;
            match acceptance {
                Acceptance::Accepted(accepted) => {
                    settled = Some((app, accepted));
                    break;
                }
                // Ended, used up or revoked since it was read.
                Acceptance::Unavailable => return Err(AppError::invitation_unavailable()),
                // Whether the maker may still invite is asked again, of what they are now.
                Acceptance::InviterChanged => {}
            }
        }
        // Its maker was changed every time it was tried: as good as no link.
        let Some((app, accepted)) = settled else {
            return Err(AppError::invitation_unavailable());
        };
        if new_user.is_some() {
            self.metrics().registered();
        }
        // Who is on the board of a guest's link sees the newcomer among its members.
        if let (true, Some(board_id)) = (accepted.used, &accepted.board_id) {
            match app
                .store()
                .get_member_for_board(board_id, &accepted.user_id)
                .await
            {
                Ok(member) => {
                    app.broadcast_member_change_in_background(
                        accepted.workspace_id.clone(),
                        member,
                    );
                }
                Err(error) => tracing::warn!(
                    %error,
                    board_id,
                    "a guest who came in was not announced to the board"
                ),
            }
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

/// Whether the user sees and revokes every link into a workspace, and not their own
/// alone: who invites its members.
const fn sees_every_link(actor: Actor) -> bool {
    permissions::allows_in_workspace(actor, WorkspacePermission::InviteMembers)
}
