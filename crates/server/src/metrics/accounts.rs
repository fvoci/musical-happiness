//! The metrics of the accounts: how logins ended, how many users registered, and how
//! many sessions are live. No label names a user, an address or a session.

use prometheus::{IntCounter, IntCounterVec, IntGauge, Opts, Registry};

use super::{Metrics, PREFIX, ScrapeRead};

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
}

impl LoginOutcome {
    const ALL: [Self; 4] = [
        Self::Succeeded,
        Self::InvalidCredentials,
        Self::Limited,
        Self::Failed,
    ];

    /// The value of the `outcome` label.
    fn as_str(self) -> &'static str {
        match self {
            Self::Succeeded => "succeeded",
            Self::InvalidCredentials => "invalid_credentials",
            Self::Limited => "limited",
            Self::Failed => "failed",
        }
    }

    /// The outcome's place in [`LoginOutcome::ALL`].
    fn index(self) -> usize {
        match self {
            Self::Succeeded => 0,
            Self::InvalidCredentials => 1,
            Self::Limited => 2,
            Self::Failed => 3,
        }
    }
}

/// The metrics of the accounts.
#[derive(Debug)]
pub(super) struct AccountMetrics {
    /// The counter of each outcome, in the order of [`LoginOutcome::ALL`].
    logins: [IntCounter; 4],
    registrations: IntCounter,
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
                 was too busy), failed (the server could not check it). An attempt the \
                 server could not read is not counted.",
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
                "How many users registered: the first user of the instance, and those \
                 with a signup token.",
            )
            .namespace(PREFIX),
        )?;
        registry.register(Box::new(registrations.clone()))?;
        let sessions_live = IntGauge::with_opts(
            Opts::new(
                "sessions_live",
                "How many sessions are live, as a scrape counts them: not past their end, \
                 used within session_idle_days, of a user who is not deactivated.",
            )
            .namespace(PREFIX),
        )?;
        registry.register(Box::new(sessions_live.clone()))?;
        let sessions_read = ScrapeRead::register(
            registry,
            "sessions_read_failures_total",
            "How many scrapes could not count the live sessions (the database failed or \
             took over two seconds) and gave the value counted before.",
        )?;
        Ok(Self {
            logins,
            registrations,
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

    /// Sets the live sessions to what `read` answers, for a scrape, as
    /// [`Metrics::read_open_uploads`] sets the open uploads.
    pub(crate) async fn read_live_sessions<E: std::fmt::Display>(
        &self,
        read: impl Future<Output = Result<i64, E>>,
    ) {
        let Some(kept) = &self.kept else {
            return;
        };
        let accounts = &kept.accounts;
        if let Some(live) = accounts.sessions_read.read("the live sessions", read).await {
            accounts.sessions_live.set(live);
        }
    }
}
