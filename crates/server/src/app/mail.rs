//! SMTP delivery in tracked background work, with bounded attempts and transport concurrency.

mod content;

use std::sync::Arc;
use std::time::Duration;

use anyhow::Context as _;
use lettre::address::Envelope;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{Address, AsyncSmtpTransport, AsyncTransport as _, Tokio1Executor};
use mail_builder::MessageBuilder;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use crate::config::smtp::SmtpSettings;

pub(super) use content::{Content, Language};

/// At most four SMTP connections deliver concurrently; HTTP requests never wait for them.
const CONNECTIONS: usize = 4;
/// A whole SMTP attempt, including connection and all commands, gets ten seconds.
const ATTEMPT_TIMEOUT: Duration = Duration::from_secs(10);
/// The first attempt plus two retries, after one and four seconds, bounds delivery work.
const RETRY_DELAYS: [u64; 3] = [0, 1, 4];

struct Transport {
    smtp: AsyncSmtpTransport<Tokio1Executor>,
    from: Address,
}

/// The optional transport; constructing it performs no network I/O.
#[derive(Clone)]
pub(super) struct Mailer {
    transport: Option<Arc<Transport>>,
    connections: Arc<Semaphore>,
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
        })
    }

    /// Waits for capacity before the caller creates short-lived message credentials.
    pub(super) async fn delivery(&self) -> anyhow::Result<Delivery> {
        let transport = Arc::clone(
            self.transport
                .as_ref()
                .context("outgoing mail is disabled")?,
        );
        let connection = Arc::clone(&self.connections)
            .acquire_owned()
            .await
            .context("mail transport is closed")?;
        Ok(Delivery {
            transport,
            _connection: connection,
        })
    }
}

/// A reserved SMTP connection, retained through all attempts and released on drop.
pub(super) struct Delivery {
    transport: Arc<Transport>,
    _connection: OwnedSemaphorePermit,
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
