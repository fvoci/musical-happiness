//! SMTP delivery in tracked background work, with bounded attempts, queues and connections.

mod content;
mod quota;

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Context as _;
use lettre::address::Envelope;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{Address, AsyncSmtpTransport, AsyncTransport as _, Tokio1Executor};
use mail_builder::MessageBuilder;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use crate::auth::limiter::RetryAfter;
use crate::config::smtp::SmtpSettings;

pub(super) use content::{Content, Language};

/// At most four SMTP connections deliver concurrently; HTTP requests never wait for them.
const CONNECTIONS: usize = 4;
/// Invitations use at most this many connections, so one is always left for recovery.
const INVITATION_CONNECTIONS: usize = CONNECTIONS - 1;
/// A whole SMTP attempt, including connection and all commands, gets ten seconds.
const ATTEMPT_TIMEOUT: Duration = Duration::from_secs(10);
/// The first attempt plus two retries, after one and four seconds, bounds delivery work.
const RETRY_DELAYS: [u64; 3] = [0, 1, 4];

struct Transport {
    smtp: AsyncSmtpTransport<Tokio1Executor>,
    from: Address,
}

/// What a queued message is, which decides its queue and the connections it may use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    PasswordReset,
    Invitation,
}

/// Why invitation messages were not queued; nothing of the request was.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Refused {
    /// The workspace queued its hourly limit; the oldest of those leave after this.
    HourlyLimit(Duration),
    /// The invitation queue holds as many messages as it may.
    QueueFull,
}

/// The optional transport; constructing it performs no network I/O.
#[derive(Clone)]
pub(super) struct Mailer {
    transport: Option<Arc<Transport>>,
    connections: Arc<Semaphore>,
    invitation_connections: Arc<Semaphore>,
    reset_queue: Arc<Semaphore>,
    invitation_queue: Arc<Semaphore>,
    invitation_quota: Arc<quota::HourlyQuota>,
}

impl std::fmt::Debug for Mailer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Mailer")
            .field("enabled", &self.transport.is_some())
            .finish_non_exhaustive()
    }
}

impl Mailer {
    pub(super) fn new(settings: &SmtpSettings) -> anyhow::Result<Self> {
        settings.check()?;
        let transport = if settings.enabled() {
            let mut builder = match settings.smtp_tls.as_str() {
                "tls" => AsyncSmtpTransport::<Tokio1Executor>::relay(&settings.smtp_host)?,
                "starttls" => {
                    AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&settings.smtp_host)?
                }
                _ => AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&settings.smtp_host),
            }
            .port(settings.smtp_port)
            .timeout(Some(ATTEMPT_TIMEOUT));
            if !settings.smtp_user.is_empty() {
                builder = builder.credentials(Credentials::new(
                    settings.smtp_user.clone(),
                    settings.smtp_password.clone(),
                ));
            }
            Some(Arc::new(Transport {
                smtp: builder.build(),
                from: settings
                    .smtp_from
                    .parse()
                    .context("smtp_from must be a single mailbox")?,
            }))
        } else {
            None
        };
        Ok(Self {
            transport,
            connections: Arc::new(Semaphore::new(CONNECTIONS)),
            invitation_connections: Arc::new(Semaphore::new(INVITATION_CONNECTIONS)),
            reset_queue: Arc::new(Semaphore::new(usize::try_from(settings.mail_reset_queue)?)),
            invitation_queue: Arc::new(Semaphore::new(usize::try_from(
                settings.mail_invitation_queue,
            )?)),
            invitation_quota: Arc::new(quota::HourlyQuota::new(settings.invitation_mail_per_hour)),
        })
    }

    /// Takes the place of one recovery message in its queue, or `None` when it is full.
    pub(super) fn queue_reset(&self) -> Option<Queued> {
        Arc::clone(&self.reset_queue)
            .try_acquire_owned()
            .ok()
            .map(|place| Queued {
                kind: Kind::PasswordReset,
                place,
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
    ) -> Result<Vec<Queued>, Refused> {
        let mut places = self
            .invitation_quota
            .take(workspace_id, messages, Instant::now(), || {
                Arc::clone(&self.invitation_queue)
                    .try_acquire_many_owned(messages)
                    .ok()
            })
            .map_err(|refused| match refused {
                quota::Refused::Limit(RetryAfter(wait)) => Refused::HourlyLimit(wait),
                quota::Refused::Unplaced => Refused::QueueFull,
            })?;
        let mut queued = Vec::new();
        while let Some(place) = places.split(1) {
            queued.push(Queued {
                kind: Kind::Invitation,
                place,
            });
        }
        Ok(queued)
    }
}

/// One message's place in its queue, held until the message was delivered or given up.
#[derive(Debug)]
pub(super) struct Queued {
    kind: Kind,
    place: OwnedSemaphorePermit,
}

impl Queued {
    /// Waits for an SMTP connection this kind of message may use, keeping the place
    /// until then, before the caller creates short-lived message credentials.
    pub(super) async fn delivery(self, mailer: &Mailer) -> anyhow::Result<Delivery> {
        let transport = Arc::clone(
            mailer
                .transport
                .as_ref()
                .context("outgoing mail is disabled")?,
        );
        let share = match self.kind {
            Kind::PasswordReset => None,
            Kind::Invitation => Some(
                Arc::clone(&mailer.invitation_connections)
                    .acquire_owned()
                    .await
                    .context("mail transport is closed")?,
            ),
        };
        let connection = Arc::clone(&mailer.connections)
            .acquire_owned()
            .await
            .context("mail transport is closed")?;
        Ok(Delivery {
            transport,
            _connection: connection,
            _share: share,
            _place: self.place,
        })
    }
}

/// A reserved SMTP connection, retained through all attempts and released on drop,
/// with the message's place in its queue and, for invitations, their share of connections.
/// Fields drop in order: the connection is free again before the share that let an
/// invitation take it, so invitations never hold all four at once.
pub(super) struct Delivery {
    transport: Arc<Transport>,
    _connection: OwnedSemaphorePermit,
    _share: Option<OwnedSemaphorePermit>,
    _place: OwnedSemaphorePermit,
}

impl Delivery {
    pub(super) async fn send(self, recipient: &str, content: Content) -> anyhow::Result<()> {
        let transport = &self.transport;
        anyhow::ensure!(
            fvoci_model::is_email_valid(recipient),
            "invalid mail recipient"
        );
        let envelope = Envelope::new(Some(transport.from.clone()), vec![recipient.parse()?])?;
        let from = transport.from.to_string();
        let domain = from.split_once('@').context("invalid sender")?.1;
        let message_id = format!("{}@{domain}", crate::auth::secret::new_secret());
        let message = MessageBuilder::new()
            .from(from.as_str())
            .to(recipient)
            .message_id(message_id)
            .subject(content.subject)
            .text_body(content.plain)
            .html_body(content.html)
            .write_to_vec()?;
        for (attempt, delay) in RETRY_DELAYS.into_iter().enumerate() {
            tokio::time::sleep(Duration::from_secs(delay)).await;
            if matches!(
                tokio::time::timeout(
                    ATTEMPT_TIMEOUT,
                    transport.smtp.send_raw(&envelope, &message)
                )
                .await,
                Ok(Ok(_))
            ) {
                return Ok(());
            }
            // A server's error text can echo addresses or message content. Never log it.
            tracing::warn!(attempt = attempt + 1, "SMTP delivery attempt failed");
        }
        anyhow::bail!("SMTP delivery failed after three attempts")
    }
}
