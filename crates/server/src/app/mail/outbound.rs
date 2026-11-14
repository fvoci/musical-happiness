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
    /// The sender's address (`mail_from`).
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

/// What became of one attempt to send a message: all the queue decides by. Each way
/// sorts its own answers into these, by what they mean rather than by their numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// The way took the message.
    Sent,
    /// Not now: the way is busy, does not answer in time, is throttled or past a quota,
    /// or anything else that another attempt later may get past. Three in a row hold
    /// invitations back for a while as recovery goes on. `after` is how long the way
    /// asked to be left alone, if it said; `code` is its reply or status code, if it
    /// gave one.
    Later {
        after: Option<Duration>,
        code: Option<u16>,
    },
    /// Never: the way refused this message for good (a recipient it does not take, a
    /// message it will not carry); another attempt would get the same.
    Refused { code: Option<u16> },
    /// The whole way is closed to this server: it refuses the server's keys, token,
    /// signature or permissions (SMTP `530`, `534`, `535`, `538`; HTTP `401`, `403`),
    /// it answers a configuration every message meets alike (OCI's and Cloudflare's
    /// `404`, SES's paused account), or a connection to it cannot be made (each way
    /// says which of its failures are such). No message goes until it opens again, so
    /// this counts against none of the message's attempts: the queue holds invitations
    /// back and tries one message a minute. `code` is the reply or status code, if there was one.
    PathDown { code: Option<u16> },
}

/// A way of sending a message. One call is one attempt; the queue times it, paces it
/// and decides on another from the [`Outcome`] alone. An implementation reports no
/// text of the provider's, no address and no part of the message.
pub(crate) trait Outbound: Send + Sync + 'static {
    /// The way's name in logs (`smtp`), which says what the codes are codes of.
    fn name(&self) -> &'static str;

    /// Sends `message` once.
    fn send<'a>(
        &'a self,
        message: &'a Outgoing,
    ) -> Pin<Box<dyn Future<Output = Outcome> + Send + 'a>>;
}
