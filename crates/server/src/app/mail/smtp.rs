//! Sending over SMTP: the way of [`Outbound`] that `smtp_host` configures.

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use anyhow::Context as _;
use lettre::address::Envelope;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{Address, AsyncSmtpTransport, AsyncTransport as _, Tokio1Executor};
use mail_builder::MessageBuilder;
use mail_builder::headers::raw::Raw;

use super::outbound::{Outbound, Outcome, Outgoing};
use crate::config::smtp::{SmtpSettings, SmtpTls};

/// An SMTP relay; constructing it performs no network I/O.
pub(super) struct Smtp {
    transport: AsyncSmtpTransport<Tokio1Executor>,
}

impl Smtp {
    /// The relay of `settings`, whose commands each get `timeout`.
    ///
    /// # Errors
    /// Returns an error if the host cannot be used for TLS.
    pub(super) fn new(settings: &SmtpSettings, timeout: Duration) -> anyhow::Result<Self> {
        let mut builder = match settings.smtp_tls {
            SmtpTls::Tls => AsyncSmtpTransport::<Tokio1Executor>::relay(&settings.smtp_host)?,
            SmtpTls::Starttls => {
                AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&settings.smtp_host)?
            }
            SmtpTls::Unencrypted => {
                AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&settings.smtp_host)
            }
        }
        .port(settings.smtp_port)
        .timeout(Some(timeout));
        if !settings.smtp_user.is_empty() {
            builder = builder.credentials(Credentials::new(
                settings.smtp_user.clone(),
                settings.smtp_password.clone(),
            ));
        }
        Ok(Self {
            transport: builder.build(),
        })
    }

    async fn deliver(&self, message: &Outgoing) -> Outcome {
        let Ok((envelope, bytes)) = build(message) else {
            // Addresses were checked before the message was queued; one the builder
            // still refuses would be refused by every server.
            return Outcome::Refused { code: None };
        };
        match self.transport.send_raw(&envelope, &bytes).await {
            Ok(_) => Outcome::Sent,
            Err(error) => outcome_of(error.status().map(u16::from)),
        }
    }
}

impl Outbound for Smtp {
    fn send<'a>(
        &'a self,
        message: &'a Outgoing,
    ) -> Pin<Box<dyn Future<Output = Outcome> + Send + 'a>> {
        Box::pin(self.deliver(message))
    }
}

/// What an SMTP error makes of the attempt: a 5xx reply refuses the message for good;
/// a 4xx reply (`421`, `451`, `454`) and anything without a reply (the connection, TLS,
/// a timeout) is worth another attempt. Only the reply code is kept, never its text.
fn outcome_of(reply: Option<u16>) -> Outcome {
    match reply {
        Some(code @ 500..=599) => Outcome::Refused { code: Some(code) },
        code => Outcome::Later { after: None, code },
    }
}

/// The envelope and the MIME text of `message`.
fn build(message: &Outgoing) -> anyhow::Result<(Envelope, Vec<u8>)> {
    let from: Address = message.from.parse()?;
    let envelope = Envelope::new(Some(from), vec![message.to.parse()?])?;
    let domain = message.from.split_once('@').context("invalid sender")?.1;
    let message_id = format!("{}@{domain}", crate::auth::secret::new_secret());
    let bytes = MessageBuilder::new()
        .from(message.from.as_str())
        .to(message.to.as_str())
        .message_id(message_id)
        .header("Content-Language", Raw::new(message.language.tag()))
        .subject(message.subject)
        .text_body(message.plain.as_str())
        .html_body(message.html.as_str())
        .write_to_vec()?;
    Ok((envelope, bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_5xx_reply_refuses_for_good_and_anything_else_asks_for_later() {
        assert_eq!(outcome_of(Some(550)), Outcome::Refused { code: Some(550) });
        assert_eq!(outcome_of(Some(554)), Outcome::Refused { code: Some(554) });
        for code in [Some(421), Some(451), Some(454), None] {
            assert_eq!(outcome_of(code), Outcome::Later { after: None, code });
        }
    }
}
