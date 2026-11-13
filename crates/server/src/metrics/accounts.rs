//! The metrics of the accounts: how logins ended, how many users registered, how many
//! recovery messages were dropped, and how many sessions are live. No label names a
//! user, an address or a session.

use prometheus::{IntCounter, IntCounterVec, IntGauge, Opts, Registry};

use super::{Metrics, PREFIX, ScrapeRead};

/// How long a count of the live sessions stays fresh: it goes through the whole table
/// of sessions, which has no index of their times, so a scrape before then reads none.
const SESSIONS_FRESH_FOR: std::time::Duration = std::time::Duration::from_secs(60);

/// How an attempt to sign in ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LoginOutcome {
    /// A session was opened.
    Succeeded,
    /// The name or the password was wrong: one count for both, as the answer is one,
    /// so that the metrics do not tell whether an account exists.
    InvalidCredentials,
    /// A limit refused it: the login limits, or the queue of password work was full.
    Limited,
    /// The server could not check it: the database or the password work failed.
    Failed,
    /// Its task was stopped before it ended: the server shut down. (A request's handler
    /// is run to its end however its client goes.)
    Abandoned,
}

impl LoginOutcome {
    const ALL: [Self; 5] = [
        Self::Succeeded,
        Self::InvalidCredentials,
        Self::Limited,
        Self::Failed,
        Self::Abandoned,
    ];

    /// The value of the `outcome` label.
    fn as_str(self) -> &'static str {
        match self {
            Self::Succeeded => "succeeded",
            Self::InvalidCredentials => "invalid_credentials",
            Self::Limited => "limited",
            Self::Failed => "failed",
            Self::Abandoned => "abandoned",
        }
    }

    /// The outcome's place in [`LoginOutcome::ALL`].
    fn index(self) -> usize {
        match self {
            Self::Succeeded => 0,
            Self::InvalidCredentials => 1,
            Self::Limited => 2,
            Self::Failed => 3,
            Self::Abandoned => 4,
        }
    }
}

/// The metrics of the accounts.
#[derive(Debug)]
pub(super) struct AccountMetrics {
    /// The counter of each outcome, in the order of [`LoginOutcome::ALL`].
    logins: [IntCounter; 5],
    registrations: IntCounter,
    reset_mail_dropped: IntCounter,
    sessions_live: IntGauge,
    /// The scrapes' reads of the live sessions.
    sessions_read: ScrapeRead,
}

impl AccountMetrics {
    /// Makes the metrics and registers them in `registry`, every count at zero.
    pub(super) fn register(registry: &Registry) -> prometheus::Result<Self> {
        let logins = IntCounterVec::new(
            Opts::new(
                "logins_total",
                "How many attempts to sign in with a password ended, by outcome: \
                 succeeded, invalid_credentials (no such account or a wrong password, \
                 which are not told apart), limited (a login limit, or the password work \
                 was too busy), failed (the server could not check it), abandoned (the \
                 server shut down first). An attempt the server could not read is not \
                 counted.",
            )
            .namespace(PREFIX),
            &["outcome"],
        )?;
        registry.register(Box::new(logins.clone()))?;
        let logins = LoginOutcome::ALL.map(|outcome| {
            let counter = logins.with_label_values(&[outcome.as_str()]);
            counter.reset();
            counter
        });
        let registrations = IntCounter::with_opts(
            Opts::new(
                "registrations_total",
                "How many accounts were made by registering: those that started an organization, \
                 and those an invitation let in.",
            )
            .namespace(PREFIX),
        )?;
        registry.register(Box::new(registrations.clone()))?;
        let reset_mail_dropped = IntCounter::with_opts(
            Opts::new(
                "password_reset_mail_dropped_total",
                "How many password recovery messages were not sent because \
                 mail_reset_queue messages were already waiting or being delivered. The \
                 request was answered as any other.",
            )
            .namespace(PREFIX),
        )?;
        registry.register(Box::new(reset_mail_dropped.clone()))?;
        let sessions_live = IntGauge::with_opts(
            Opts::new(
                "sessions_live",
                "How many sessions are live, as a scrape counts them at most once a \
                 minute: not past their end, used within session_idle_days, of a user who \
                 is not deactivated.",
            )
            .namespace(PREFIX),
        )?;
        registry.register(Box::new(sessions_live.clone()))?;
        let sessions_read = ScrapeRead::register(
            registry,
            "sessions_read_failures_total",
            "How many counts of the live sessions failed (the database failed or took \
             over two seconds); the scrapes until the next count give the value counted \
             before.",
            SESSIONS_FRESH_FOR,
        )?;
        Ok(Self {
            logins,
            registrations,
            reset_mail_dropped,
            sessions_live,
            sessions_read,
        })
    }
}

impl Metrics {
    /// Counts an attempt to sign in that ended so.
    pub(crate) fn login_ended(&self, outcome: LoginOutcome) {
        if let Some(kept) = &self.kept {
            kept.accounts.logins[outcome.index()].inc();
        }
    }

    /// Counts a user who registered.
    pub(crate) fn registered(&self) {
        if let Some(kept) = &self.kept {
            kept.accounts.registrations.inc();
        }
    }

    /// Counts a password recovery message dropped because its queue was full.
    pub(crate) fn password_reset_mail_dropped(&self) {
        if let Some(kept) = &self.kept {
            kept.accounts.reset_mail_dropped.inc();
        }
    }

    /// Sets the live sessions to what `read` answers, for a scrape, as
    /// [`Metrics::read_open_uploads`] sets the open uploads; but not more often than
    /// once a minute, as the count goes through the whole table of sessions.
    pub(crate) async fn read_live_sessions<E: std::fmt::Display + Send + 'static>(
        &self,
        read: impl Future<Output = Result<i64, E>> + Send + 'static,
    ) {
        let Some(kept) = &self.kept else {
            return;
        };
        let accounts = &kept.accounts;
        let live = accounts.sessions_live.clone();
        accounts
            .sessions_read
            .read("live_sessions", read, move |count| live.set(count))
            .await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_outcome_has_its_own_counter() {
        for (place, outcome) in LoginOutcome::ALL.into_iter().enumerate() {
            assert_eq!(outcome.index(), place, "{outcome:?}");
        }
    }
}
