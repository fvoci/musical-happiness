//! The way a message leaves the server: one boundary, which SMTP implements here and
//! a mail provider's HTTP API can implement beside it. The queue, its retries, its
//! shares and its pace know nothing of the way but what one attempt became.

use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use super::content::Language;

/// One message to one recipient, as every way of sending takes it.
pub(crate) struct Outgoing {
    /// The sender's address (`smtp_from`).
    pub(crate) from: String,
    /// The one recipient's address.
    pub(crate) to: String,
    pub(crate) subject: &'static str,
    pub(crate) plain: String,
    pub(crate) html: String,
    pub(crate) language: Language,
}

impl fmt::Debug for Outgoing {
    // The addresses and the body (which holds a link with its token) stay out of
    // anything that prints this.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Outgoing")
            .field("subject", &self.subject)
            .field("language", &self.language)
            .finish_non_exhaustive()
    }
}

/// What became of one attempt to send a message: all the queue decides by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// The way took the message.
    Sent,
    /// Not now: the way is busy, limited or unreachable. `after` is how long it asked
    /// to be left alone, if it said; `code` is its reply code (SMTP 4xx, HTTP 429 or
    /// 5xx), if it gave one.
    Later {
        after: Option<Duration>,
        code: Option<u16>,
    },
    /// Never: the way refused this message for good (SMTP 5xx, HTTP 4xx but 429).
    Refused { code: Option<u16> },
}

/// A way of sending a message. One call is one attempt; the queue times it, paces it
/// and decides on another from the [`Outcome`] alone. An implementation reports no
/// text of the provider's, no address and no part of the message.
pub(crate) trait Outbound: Send + Sync + 'static {
    /// Sends `message` once.
    fn send<'a>(
        &'a self,
        message: &'a Outgoing,
    ) -> Pin<Box<dyn Future<Output = Outcome> + Send + 'a>>;
}
