//! Sending an existing invitation under the same permissions as making its link.

use std::collections::BTreeSet;

use fvoci_model::{Invitation, InvitationMail};

use super::super::mail::{Content, Language, Queued, Refused};
use super::App;
use crate::auth::{now_millis, secret::token_digest};
use crate::error::{AppError, Reason};

impl App {
    /// Queues individual messages carrying this workspace's existing invitation.
    ///
    /// # Errors
    /// Returns 404 without SMTP or a usable link in this workspace, 403 when the
    /// sender cannot invite, 400 if any recipient is not a single email address or
    /// there are more than `invitation_mail_recipients`, 429 when the workspace's hourly
    /// limit would be passed, and 503 when the invitation queue cannot hold them all.
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
        let queued = self
            .inner
            .mail
            .queue_invitations(workspace_id, count)
            .map_err(|refused| match refused {
                Refused::HourlyLimit(wait) => AppError::quota_reached(
                    "the workspace sent its hourly invitation mail; try again later",
                    Reason::InvitationMailLimit,
                    wait,
                ),
                Refused::QueueFull => {
                    AppError::service_unavailable("too much mail is waiting; try again later")
                        .with_reason(Reason::MailQueueFull)
                }
            })?;
        request.recipients = recipients.into_iter().collect();
        let app = self.clone();
        let workspace_id = workspace_id.to_owned();
        let sender = sender.to_owned();
        self.spawn_background(async move {
            if app
                .deliver_invitation(&workspace_id, &sender, request, queued)
                .await
                .is_err()
            {
                tracing::warn!("invitation mail delivery failed");
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
        let settings = self.user_settings(sender).await?;
        let fallback =
            Language::recipient(settings.language.as_str(), Language::of(&request.language));
        let link = format!(
            "{}/invite/{}",
            self.config().server_root.trim_end_matches('/'),
            request.token
        );
        for (recipient, place) in request.recipients.into_iter().zip(queued) {
            let delivery = place
                .delivery(&self.inner.mail)
                .await
                .map_err(|_| AppError::internal("mail delivery is unavailable"))?;
            // Permissions and expiry may have changed while this job waited for capacity.
            let invitation = self
                .invitation_for_mail(workspace_id, sender, &request.token)
                .await?;
            let language = match self.store().get_user_by_email(&recipient).await {
                Ok(user) => Language::recipient(
                    self.user_settings(&user.id).await?.language.as_str(),
                    fallback,
                ),
                Err(error) if error.is_not_found() => fallback,
                Err(error) => return Err(error.into()),
            };
            // One failed address must not stop delivery to the other recipients.
            if delivery
                .send(
                    &recipient,
                    Content::invitation(&link, language, invitation.role),
                )
                .await
                .is_err()
            {
                tracing::warn!("invitation mail delivery exhausted its retries");
            }
        }
        Ok(())
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
