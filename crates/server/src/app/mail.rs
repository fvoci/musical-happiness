//! Outgoing mail in tracked background work: bounded queues, shares of the sending, one
//! pace for all, and bounded attempts. How a message leaves is the [`Outbound`]'s.

mod content;
mod outbound;
mod pace;
mod quota;
mod share;
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
/// What a mail semaphore's acquire expects: none is ever closed.
const CLOSED: &str = "the mail semaphores are never closed";
/// The longest a message waits for another attempt: a way that asks for longer has its
/// message given up then, rather than asked again before it said.
const LONGEST_RETRY_WAIT: Duration = Duration::from_mins(1);

/// Why invitation messages were not queued; nothing of the request was.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum InvitationRefusal {
    /// The workspace queued its hourly limit; the oldest of those leave after this.
    WorkspaceHourlyLimit(Duration),
    /// The organization queued its hourly limit; the oldest of those leave after this.
    OrgHourlyLimit(Duration),
    /// The organization holds half of the invitation queue.
    OrgShareFull,
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
    /// Invitation messages per workspace and hour, up to `invitation_mail_per_hour`.
    workspace_quota: Arc<quota::HourlyQuota>,
    workspace_per_hour: u32,
    /// Invitation messages per organization and hour, up to the organization's limit.
    org_quota: Arc<quota::HourlyQuota>,
    org_shares: Arc<share::OrgShares>,
    /// Recovery messages per account and hour, up to `mail_reset_per_account_hour`.
    reset_quota: Arc<quota::HourlyQuota>,
    reset_per_hour: u32,
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
            workspace_quota: Arc::default(),
            workspace_per_hour: settings.invitation_mail_per_hour,
            org_quota: Arc::default(),
            org_shares: Arc::new(share::OrgShares::new(settings.mail_invitation_queue / 2)),
            reset_quota: Arc::default(),
            reset_per_hour: settings.mail_reset_per_account_hour,
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
            .take(user_id, 1, self.reset_per_hour, Instant::now(), || {
                Arc::clone(&self.reset_queue).try_acquire_owned()
            })
            .map(|place| Queued {
                kind: MailKind::PasswordReset,
                place,
                org: None,
            })
            .map_err(|refusal| match refusal {
                quota::Refusal::Limit(_) => ResetMailDrop::AccountLimit,
                quota::Refusal::Unplaced(_) => ResetMailDrop::QueueFull,
            })
    }

    /// Takes the places of all `messages` of one invitation request of the workspace
    /// `workspace_id` in the organization `org_id`, or none of them. In this order, the
    /// workspace's hourly limit, the organization's (`org_per_hour`), the
    /// organization's half of the queue and the queue must each take them all: a
    /// workspace or an organization past its hour is told when to come back even while
    /// the queue is full, and one organization never holds the whole queue.
    pub(super) fn queue_invitations(
        &self,
        workspace_id: &str,
        org_id: &str,
        org_per_hour: u32,
        messages: u32,
    ) -> Result<Vec<Queued>, InvitationRefusal> {
        let now = Instant::now();
        let (orgs, mut places) = self
            .workspace_quota
            .take(workspace_id, messages, self.workspace_per_hour, now, || {
                self.org_quota
                    .take(org_id, messages, org_per_hour, now, || {
                        let orgs = self
                            .org_shares
                            .take(org_id, messages)
                            .ok_or(InvitationRefusal::OrgShareFull)?;
                        let places = Arc::clone(&self.invitation_queue)
                            .try_acquire_many_owned(messages)
                            .map_err(|_| InvitationRefusal::QueueFull)?;
                        Ok((orgs, places))
                    })
                    .map_err(|refusal| match refusal {
                        quota::Refusal::Limit(RetryAfter(wait)) => {
                            InvitationRefusal::OrgHourlyLimit(wait)
                        }
                        quota::Refusal::Unplaced(refusal) => refusal,
                    })
            })
            .map_err(|refusal| match refusal {
                quota::Refusal::Limit(RetryAfter(wait)) => {
                    InvitationRefusal::WorkspaceHourlyLimit(wait)
                }
                quota::Refusal::Unplaced(refusal) => refusal,
            })?;
        let mut queued = Vec::new();
        for org in orgs {
            let Some(place) = places.split(1) else { break };
            queued.push(Queued {
                kind: MailKind::Invitation,
                place,
                org: Some(org),
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
    /// For an invitation, its part of its organization's share.
    org: Option<share::OrgPlace>,
}

/// A message the caller made, to one recipient; the sender is the mailer's.
pub(super) struct Letter {
    pub(super) to: String,
    pub(super) content: Content,
}

impl Queued {
    /// Delivers the message `write` makes, keeping the place until it was delivered or
    /// given up. Each attempt waits for its turn of the pace and then for a sending its
    /// kind may use, holding nothing while it waits for the turn or for the next
    /// attempt; the first one only then asks `write` for the message, so that what
    /// `write` creates (a reset token) is as young as can be. `None` sends nothing.
    /// Attempts follow the way's [`Outcome`]: up to three while it asks for later, one
    /// when it refuses. What became of the message is logged and counted here, without
    /// a reply text, an address or a token.
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
        let mut write = Some(write);
        let mut message = None;
        let mut waits = RETRY_DELAYS.into_iter();
        for attempt in 1.. {
            // The organization's own sending first: it keeps waiting only that
            // organization's messages, and a message that has its turn has its sending.
            let org_sending = match &self.org {
                Some(org) => Some(
                    Arc::clone(&org.sending)
                        .acquire_owned()
                        .await
                        .expect(CLOSED),
                ),
                None => None,
            };
            mailer.pace.turn(kind).await;
            let sending = mailer.sending(kind, org_sending).await;
            if let Some(write) = write.take() {
                let Some(letter) = write().await? else {
                    return Ok(());
                };
                message = Some(mailer.outgoing(letter));
            }
            let Some(message) = message.as_ref() else {
                break;
            };
            let outcome = tokio::time::timeout(ATTEMPT_TIMEOUT, outbound.send(message))
                .await
                .unwrap_or(Outcome::Later {
                    after: None,
                    code: None,
                });
            drop(sending);
            mailer.pace.observe(outcome);
            let way = outbound.name();
            let wait = match outcome {
                Outcome::Sent => break,
                Outcome::Refused { code } => {
                    tracing::warn!(
                        kind = kind.as_str(),
                        way,
                        attempt,
                        code,
                        "mail refused for good"
                    );
                    mailer.metrics.mail_failed(kind, MailFailure::Permanent);
                    break;
                }
                Outcome::Later { after, code } => {
                    tracing::warn!(kind = kind.as_str(), way, attempt, code, "mail deferred");
                    // A way that asks for longer than another attempt may wait is
                    // not asked again before it said.
                    let wait = waits.next().map(|backoff| after.unwrap_or(backoff));
                    let Some(wait) = wait.filter(|wait| *wait <= LONGEST_RETRY_WAIT) else {
                        tracing::warn!(kind = kind.as_str(), way, attempt, "mail given up");
                        mailer.metrics.mail_failed(kind, MailFailure::Temporary);
                        break;
                    };
                    wait
                }
            };
            tokio::time::sleep(wait).await;
        }
        drop((self.place, self.org));
        Ok(())
    }
}

/// The sendings one attempt holds while it is made; given back in the order of the
/// fields, so the connection is free before the share that let an invitation take it,
/// and that before its organization's: invitations never hold all four at once, nor one
/// organization all three of theirs.
struct Sending {
    _connection: OwnedSemaphorePermit,
    _invitations: Option<OwnedSemaphorePermit>,
    _org: Option<OwnedSemaphorePermit>,
}

impl Mailer {
    /// Waits for a sending a message of `kind` may use, beside its organization's `org`
    /// that an invitation already holds.
    async fn sending(&self, kind: MailKind, org: Option<OwnedSemaphorePermit>) -> Sending {
        let invitations = match kind {
            MailKind::PasswordReset => None,
            MailKind::Invitation => Some(
                Arc::clone(&self.invitation_connections)
                    .acquire_owned()
                    .await
                    .expect(CLOSED),
            ),
        };
        Sending {
            _connection: Arc::clone(&self.connections)
                .acquire_owned()
                .await
                .expect(CLOSED),
            _invitations: invitations,
            _org: org,
        }
    }

    /// The message of `letter`, from this mailer's sender.
    fn outgoing(&self, letter: Letter) -> Outgoing {
        Outgoing {
            from: self.from.to_string(),
            to: letter.to,
            subject: letter.content.subject,
            plain: letter.content.plain,
            html: letter.content.html,
            language: letter.content.language,
        }
    }
}
