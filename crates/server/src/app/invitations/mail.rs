//! Sending an existing invitation under the same permissions as making its link.

use std::collections::BTreeSet;

use fvoci_model::{BoardRole, GUEST_INVITATION_USES, Invitation, InvitationMail, WorkspaceRole};
use fvoci_store::NewLink;

use super::super::mail::{Content, InvitationRefusal, Language, Letter, Queued};
use super::App;
use crate::auth::now_millis;
use crate::auth::secret::{new_secret, token_digest};
use crate::error::{AppError, Reason};
use fvoci_store::Tenant;

impl App {
    /// Queues individual messages carrying this workspace's existing invitation.
    ///
    /// # Errors
    /// Returns 404 without SMTP or a usable link in this workspace, 403 when the
    /// sender cannot invite, 400 if any recipient is not a single email address or
    /// there are more than `invitation_mail_recipients`, 429 when the workspace's or the
    /// organization's hourly limit would be passed, and 503 when the organization's half
    /// of the invitation queue or the queue cannot hold them all.
    /// Any refusal queues none of the messages.
    pub async fn email_invitation(
        &self,
        workspace_id: &str,
        sender: &str,
        mut request: InvitationMail,
    ) -> Result<(), AppError> {
        if !self.config().smtp.enabled() {
            return Err(AppError::not_found("route"));
        }
        let limit = self.config().smtp.invitation_mail_recipients;
        if !u32::try_from(request.recipients.len()).is_ok_and(|named| named <= limit) {
            return Err(AppError::bad_request(format!(
                "at most {limit} recipients may be named at once"
            ))
            .with_field("recipients")
            .with_reason(Reason::TooManyRecipients)
            .with_limit(u64::from(limit)));
        }
        let mut recipients = BTreeSet::new();
        for recipient in &request.recipients {
            let normalized = recipient.trim().to_owned();
            if recipient.contains(['\r', '\n'])
                || !fvoci_model::is_email_valid(&normalized)
                || normalized.parse::<lettre::Address>().is_err()
            {
                return Err(AppError::bad_request(
                    "every recipient must be a single email address",
                )
                .with_field("recipients"));
            }
            recipients.insert(with_lowercase_domain(&normalized));
        }
        if recipients.is_empty() {
            return Err(AppError::bad_request("at least one recipient is required")
                .with_field("recipients"));
        }
        self.invitation_for_mail(workspace_id, sender, &request.token)
            .await?;
        // Distinct recipients are no more than those named, which fit in the limit.
        let count = u32::try_from(recipients.len()).unwrap_or(limit);
        // The handle is the organization's the workspace is in: what it holds of the
        // mail is counted under it, at its own limit.
        let Tenant::Org(org) = self.store().tenant() else {
            return Err(AppError::internal(
                "invitation mail outside an organization",
            ));
        };
        let org = org.to_string();
        let queued = self
            .inner
            .mail
            .queue_invitations(
                workspace_id,
                &org,
                self.limits().invitation_mail_per_hour,
                count,
            )
            .map_err(|refused| match refused {
                InvitationRefusal::WorkspaceHourlyLimit(wait) => AppError::quota_reached(
                    "the workspace sent its hourly invitation mail; try again later",
                    Reason::InvitationMailLimit,
                    wait,
                ),
                InvitationRefusal::OrgHourlyLimit(wait) => AppError::quota_reached(
                    "the organization sent its hourly invitation mail; try again later",
                    Reason::InvitationMailLimit,
                    wait,
                ),
                InvitationRefusal::OrgShareFull => AppError::service_unavailable(
                    "the organization has too much mail waiting; try again later",
                )
                .with_reason(Reason::MailQueueFull),
                InvitationRefusal::QueueFull => {
                    AppError::service_unavailable("too much mail is waiting; try again later")
                        .with_reason(Reason::MailQueueFull)
                }
            })?;
        request.recipients = recipients.into_iter().collect();
        let app = self.clone();
        let workspace_id = workspace_id.to_owned();
        let sender = sender.to_owned();
        self.spawn_background(async move {
            // The cause is the store's or a refusal: what became of each message is told
            // where it is sent.
            if let Err(error) = app
                .deliver_invitation(&workspace_id, &sender, request, queued)
                .await
            {
                tracing::warn!(%error, "invitation mail delivery failed");
            }
        });
        Ok(())
    }

    async fn invitation_for_mail(
        &self,
        workspace_id: &str,
        sender: &str,
        token: &str,
    ) -> Result<Invitation, AppError> {
        let invitation = self
            .store()
            .live_invitation(&token_digest(token), now_millis())
            .await?
            .filter(|invitation| invitation.workspace_id == workspace_id)
            .ok_or_else(|| AppError::not_found("invitation"))?;
        if self
            .inviter_who_may_still_invite(&invitation)
            .await?
            .is_none()
        {
            return Err(AppError::not_found("invitation"));
        }
        let mut by_sender = invitation.clone();
        sender.clone_into(&mut by_sender.created_by);
        if self
            .inviter_who_may_still_invite(&by_sender)
            .await?
            .is_none()
        {
            return Err(AppError::forbidden(
                "only who may make this invitation may email it",
            ));
        }
        Ok(invitation)
    }

    async fn deliver_invitation(
        &self,
        workspace_id: &str,
        sender: &str,
        request: InvitationMail,
        queued: Vec<Queued>,
    ) -> Result<(), AppError> {
        let sender_language =
            Language::written_in(self.user_settings(sender).await?.language.as_str());
        let requested = Language::written_in(&request.language);
        let root = self.config().server_root.trim_end_matches('/');
        for (recipient, place) in request.recipients.into_iter().zip(queued) {
            place
                .deliver(&self.inner.mail, || async {
                    // Permissions and expiry may have changed while this waited its turn.
                    let invitation = self
                        .invitation_for_mail(workspace_id, sender, &request.token)
                        .await?;
                    let recipient_language = match self.store().get_user_by_email(&recipient).await
                    {
                        Ok(user) => Language::written_in(
                            self.user_settings(&user.id).await?.language.as_str(),
                        ),
                        Err(error) if error.is_not_found() => None,
                        Err(error) => return Err(AppError::from(error)),
                    };
                    let language =
                        Language::first_of([recipient_language, sender_language, requested]);
                    // A guest link lets one person in once: each recipient has their
                    // own, made as their message leaves. Members share the one link.
                    let token = match (invitation.board_id.as_deref(), invitation.board_role) {
                        (Some(board_id), Some(role)) if invitation.role == WorkspaceRole::Guest => {
                            self.guest_link_like(&invitation, board_id, role, sender)
                                .await?
                        }
                        _ => request.token.clone(),
                    };
                    let link = format!("{root}/invite/{token}");
                    Ok(Some(Letter {
                        content: Content::invitation(&link, language, invitation.role),
                        to: recipient,
                    }))
                })
                .await?;
        }
        Ok(())
    }
}

impl App {
    /// A new one-use guest link like `invitation` (its board, its role on it and its
    /// end), made by `sender` for one recipient of its mail; returns its token. The
    /// caller has asked whether `sender` may make such a link.
    async fn guest_link_like(
        &self,
        invitation: &Invitation,
        board_id: &str,
        role: BoardRole,
        sender: &str,
    ) -> Result<String, AppError> {
        let token = new_secret();
        let link = NewLink {
            workspace_id: &invitation.workspace_id,
            guest_of: Some((board_id, role)),
            token_hash: &token_digest(&token),
            created_by: sender,
            now: now_millis(),
            expires_at: invitation.expires_at,
            max_uses: Some(GUEST_INVITATION_USES),
        };
        let made = match self.store().create_invitation(&link).await {
            Ok(made) => made,
            Err(error) if error.is_not_found() => return Err(AppError::not_found("board")),
            Err(error) => {
                return Err(AppError::from(error).context("unable to store an invitation"));
            }
        };
        tracing::info!(
            invitation_id = %made.id,
            from_invitation_id = %invitation.id,
            workspace_id = %invitation.workspace_id,
            board_id,
            user_id = sender,
            "a guest link was made for one recipient of invitation mail"
        );
        Ok(token)
    }
}

/// The address with its domain in lower case, which names the same mailbox; the local
/// part keeps its case, which a mail server may tell apart.
fn with_lowercase_domain(address: &str) -> String {
    match address.rsplit_once('@') {
        Some((local, domain)) => format!("{local}@{}", domain.to_lowercase()),
        None => address.to_owned(),
    }
}
