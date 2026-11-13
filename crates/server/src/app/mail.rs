//! Outgoing mail in tracked background work: bounded queues, shares of the sending, one
//! pace for all, and bounded attempts. How a message leaves is the [`Outbound`]'s.

mod content;
mod outbound;
mod pace;
mod quota;
mod smtp;
#[cfg(test)]
mod tests;

use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use crate::auth::limiter::RetryAfter;
use crate::config::smtp::SmtpSettings;
use crate::metrics::{MailFailure, MailKind, Metrics, ResetMailDrop};

pub(super) use content::{Content, Language};
use outbound::{Outbound, Outcome, Outgoing};

/// At most four messages are being sent at once; HTTP requests never wait for them.
const CONNECTIONS: usize = 4;
/// Invitations use at most this many of them, so one is always left for recovery.
const INVITATION_CONNECTIONS: usize = CONNECTIONS - 1;
/// One attempt, including connecting and every command or request, gets ten seconds.
const ATTEMPT_TIMEOUT: Duration = Duration::from_secs(10);
/// The waits before the second and the third attempt, when the way did not say.
const RETRY_DELAYS: [Duration; 2] = [Duration::from_secs(1), Duration::from_secs(4)];
/// The longest a message waits for another attempt, whatever the way asked: a way that
/// asks for longer has its message given up after the last attempt.
const LONGEST_RETRY_WAIT: Duration = Duration::from_mins(1);

/// Why invitation messages were not queued; nothing of the request was.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum InvitationRefusal {
    /// The workspace queued its hourly limit; the oldest of those leave after this.
    HourlyLimit(Duration),
    /// The invitation queue holds as many messages as it may.
    QueueFull,
}

/// The optional way of sending, with the queues, shares and pace in front of it;
/// constructing it performs no network I/O.
#[derive(Clone)]
pub(super) struct Mailer {
    outbound: Option<Arc<dyn Outbound>>,
    /// The sender's address of every message (`smtp_from`).
    from: Arc<str>,
    connections: Arc<Semaphore>,
    invitation_connections: Arc<Semaphore>,
    reset_queue: Arc<Semaphore>,
    invitation_queue: Arc<Semaphore>,
    invitation_quota: Arc<quota::HourlyQuota>,
    reset_quota: Arc<quota::HourlyQuota>,
    pace: Arc<pace::Pace>,
    metrics: Metrics,
}

impl std::fmt::Debug for Mailer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Mailer")
            .field("enabled", &self.outbound.is_some())
            .finish_non_exhaustive()
    }
}

impl Mailer {
    /// `server_root` is where the links of the mail lead; the way of sending is SMTP
    /// when `smtp_host` is set.
    ///
    /// # Errors
    /// Returns an error for settings that are invalid or cannot make the way.
    pub(super) fn new(
        settings: &SmtpSettings,
        server_root: &str,
        metrics: Metrics,
    ) -> anyhow::Result<Self> {
        settings.check()?;
        if settings.enabled() && is_loopback_root(server_root) {
            tracing::warn!(
                server_root,
                "mail is sent, but serverroot is a loopback address: its links lead nowhere \
                 for their recipients"
            );
        }
        let outbound: Option<Arc<dyn Outbound>> = if settings.enabled() {
            Some(Arc::new(smtp::Smtp::new(settings, ATTEMPT_TIMEOUT)?))
        } else {
            None
        };
        Self::with_outbound(settings, outbound, metrics)
    }

    /// The mailer of `settings` that sends through `outbound`, or sends nothing.
    fn with_outbound(
        settings: &SmtpSettings,
        outbound: Option<Arc<dyn Outbound>>,
        metrics: Metrics,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            outbound,
            from: Arc::from(settings.smtp_from.as_str()),
            connections: Arc::new(Semaphore::new(CONNECTIONS)),
            invitation_connections: Arc::new(Semaphore::new(INVITATION_CONNECTIONS)),
            reset_queue: Arc::new(Semaphore::new(usize::try_from(settings.mail_reset_queue)?)),
            invitation_queue: Arc::new(Semaphore::new(usize::try_from(
                settings.mail_invitation_queue,
            )?)),
            invitation_quota: Arc::new(quota::HourlyQuota::new(settings.invitation_mail_per_hour)),
            reset_quota: Arc::new(quota::HourlyQuota::new(
                settings.mail_reset_per_account_hour,
            )),
            pace: Arc::new(pace::Pace::new(settings.mail_per_second)),
            metrics,
        })
    }

    /// Takes the place of one recovery message to the account `user_id` in its queue.
    /// A message that finds no place counts nothing against the account's hour.
    ///
    /// # Errors
    /// Returns why the message is dropped: the account's hourly limit, asked first, or
    /// a full queue.
    pub(super) fn queue_reset(&self, user_id: &str) -> Result<Queued, ResetMailDrop> {
        self.reset_quota
            .take(user_id, 1, Instant::now(), || {
                Arc::clone(&self.reset_queue).try_acquire_owned().ok()
            })
            .map(|place| Queued {
                kind: MailKind::PasswordReset,
                place,
            })
            .map_err(|refusal| match refusal {
                quota::Refusal::Limit(_) => ResetMailDrop::AccountLimit,
                quota::Refusal::Unplaced => ResetMailDrop::QueueFull,
            })
    }

    /// Takes the places of all `messages` of one invitation request, or none of them:
    /// the queue must hold them all and the workspace's hourly limit must allow them.
    /// The hourly limit is asked first, so a workspace past it is told when to come
    /// back even while the queue is full.
    pub(super) fn queue_invitations(
        &self,
        workspace_id: &str,
        messages: u32,
    ) -> Result<Vec<Queued>, InvitationRefusal> {
        let mut places = self
            .invitation_quota
            .take(workspace_id, messages, Instant::now(), || {
                Arc::clone(&self.invitation_queue)
                    .try_acquire_many_owned(messages)
                    .ok()
            })
            .map_err(|refused| match refused {
                quota::Refusal::Limit(RetryAfter(wait)) => InvitationRefusal::HourlyLimit(wait),
                quota::Refusal::Unplaced => InvitationRefusal::QueueFull,
            })?;
        let mut queued = Vec::new();
        while let Some(place) = places.split(1) {
            queued.push(Queued {
                kind: MailKind::Invitation,
                place,
            });
        }
        Ok(queued)
    }
}

/// Whether `server_root` names no address its recipients reach, as the default does:
/// `localhost`, a loopback address (also one mapped into IPv6), or the unspecified one.
/// A root that is no URL is left to the configuration's own checks.
fn is_loopback_root(server_root: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(server_root) else {
        return false;
    };
    let Some(host) = url.host_str() else {
        return false;
    };
    // A URL writes an IPv6 address in brackets, and an IPv4 one in its usual form.
    if let Ok(address) = host
        .trim_start_matches('[')
        .trim_end_matches(']')
        .parse::<IpAddr>()
    {
        let address = address.to_canonical();
        address.is_loopback() || address.is_unspecified()
    } else {
        // The parser writes the name in lower case; a trailing dot names the same host.
        let name = host.trim_end_matches('.');
        name == "localhost" || name.ends_with(".localhost")
    }
}

/// One message's place in its queue, held until the message was delivered or given up.
#[derive(Debug)]
pub(super) struct Queued {
    kind: MailKind,
    place: OwnedSemaphorePermit,
}

/// A message the caller made, to one recipient; the sender is the mailer's.
pub(super) struct Letter {
    pub(super) to: String,
    pub(super) content: Content,
}

impl Queued {
    /// Delivers the message `write` makes, keeping the place until it was delivered or
    /// given up. It waits for a share of the sending its kind may use and for its turn
    /// of the pace, and only then asks `write` for the message, so that what `write`
    /// creates (a reset token) is as young as can be; `None` sends nothing. Attempts
    /// follow the way's [`Outcome`]: up to three while it asks for later, one when it
    /// refuses. What became of the message is logged and counted here, without a reply
    /// text, an address or a token.
    ///
    /// # Errors
    /// Returns what `write` returned.
    pub(super) async fn deliver<E, Writing>(
        self,
        mailer: &Mailer,
        write: impl FnOnce() -> Writing,
    ) -> Result<(), E>
    where
        Writing: Future<Output = Result<Option<Letter>, E>>,
    {
        let Some(outbound) = mailer.outbound.as_ref() else {
            tracing::warn!("a message was queued while outgoing mail is disabled");
            return Ok(());
        };
        let kind = self.kind;
        // Dropped in this order: the connection is free before the share that let an
        // invitation take it, so invitations never hold all four at once.
        let share = match kind {
            MailKind::PasswordReset => None,
            MailKind::Invitation => Arc::clone(&mailer.invitation_connections)
                .acquire_owned()
                .await
                .ok(),
        };
        let connection = Arc::clone(&mailer.connections).acquire_owned().await.ok();
        mailer.pace.turn(kind).await;
        let Some(letter) = write().await? else {
            return Ok(());
        };
        let message = Outgoing {
            from: mailer.from.to_string(),
            to: letter.to,
            subject: letter.content.subject,
            plain: letter.content.plain,
            html: letter.content.html,
            language: letter.content.language,
        };
        let mut waits = RETRY_DELAYS.into_iter();
        for attempt in 1.. {
            let outcome = tokio::time::timeout(ATTEMPT_TIMEOUT, outbound.send(&message))
                .await
                .unwrap_or(Outcome::Later {
                    after: None,
                    code: None,
                });
            mailer.pace.observe(outcome);
            let wait = match outcome {
                Outcome::Sent => break,
                Outcome::Refused { code } => {
                    tracing::warn!(kind = kind.as_str(), attempt, code, "mail refused for good");
                    mailer.metrics.mail_failed(kind, MailFailure::Permanent);
                    break;
                }
                Outcome::Later { after, code } => {
                    tracing::warn!(kind = kind.as_str(), attempt, code, "mail deferred");
                    let Some(backoff) = waits.next() else {
                        tracing::warn!(kind = kind.as_str(), attempt, "mail given up");
                        mailer.metrics.mail_failed(kind, MailFailure::Temporary);
                        break;
                    };
                    after.map_or(backoff, |after| after.min(LONGEST_RETRY_WAIT))
                }
            };
            tokio::time::sleep(wait).await;
            mailer.pace.turn(kind).await;
        }
        drop((connection, share, self.place));
        Ok(())
    }
}
