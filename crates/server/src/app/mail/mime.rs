//! The MIME text of a message, as SMTP carries it and SES's raw sending takes it.

use anyhow::Context as _;
use mail_builder::MessageBuilder;
use mail_builder::headers::raw::Raw;

use super::outbound::Outgoing;

/// The MIME text of `message`: its plain text and HTML, its language, and a Message-ID
/// of the sender's domain.
///
/// # Errors
/// Returns an error for a sender without a domain, or what the builder refuses.
pub(super) fn message(message: &Outgoing) -> anyhow::Result<Vec<u8>> {
    let domain = message.from.split_once('@').context("invalid sender")?.1;
    let message_id = format!("{}@{domain}", crate::auth::secret::new_secret());
    Ok(MessageBuilder::new()
        .from(message.from.as_str())
        .to(message.to.as_str())
        .message_id(message_id)
        .header("Content-Language", Raw::new(message.language.tag()))
        .subject(message.subject)
        .text_body(message.plain.as_str())
        .html_body(message.html.as_str())
        .write_to_vec()?)
}
