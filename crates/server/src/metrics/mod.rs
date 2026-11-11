//! The Prometheus metrics: kept only if `prometheusaddress` is set, and written in the
//! Prometheus text format. Serving them is the listener's (`metrics_listener`).
//!
//! Every name starts with [`PREFIX`], but for the process metrics, which have the names
//! every Prometheus client gives them (`process_cpu_seconds_total` and so on). No label
//! carries an identifier of a user, a board, a team or a file, nor a path as it was
//! asked: whatever a client sends maps onto a set of values fixed in this code.
//!
//! This module is what the rest of the server counts into, the logic as well as HTTP,
//! and knows neither: no request, no answer, no database. `cargo xtask check` holds it
//! to that, as a layer of its own.

use std::sync::Arc;

use anyhow::Context as _;
use prometheus::{Encoder as _, IntGaugeVec, Opts, Registry, TextEncoder};

mod background;
mod database;
mod requests;
mod storage;
mod websocket;

pub(crate) use background::{Job, JobOutcome, LoginLimit};
pub(crate) use database::{Database, DbAccess, DbOutcome};
pub(crate) use requests::{ABANDONED, OTHER_METHOD, OTHER_PROBLEM, RequestMetrics, UNMATCHED};
pub(crate) use storage::{OpenUploads, Pool};
pub(crate) use websocket::Disconnect;

/// The prefix of the name of every metric of this server's own.
pub const PREFIX: &str = "fvoci";

/// The media type of the text [`Metrics::render`] writes.
pub(crate) const TEXT_FORMAT: &str = prometheus::TEXT_FORMAT;

/// The metrics of a server, or nothing if it keeps none. Cloning is cheap; every clone
/// counts into the same metrics. A handle that keeps none does nothing, at the cost of
/// a branch.
#[derive(Debug, Clone, Default)]
pub struct Metrics {
    kept: Option<Arc<Kept>>,
}

/// The metrics of a server that keeps them.
#[derive(Debug)]
struct Kept {
    registry: Registry,
    requests: RequestMetrics,
    websocket: websocket::WebsocketMetrics,
    storage: storage::StorageMetrics,
    database: database::DatabaseMetrics,
    background: background::BackgroundMetrics,
}

impl Metrics {
    /// Metrics that are kept: a registry of their own, with the build information and,
    /// on Linux, the process metrics.
    ///
    /// # Errors
    ///
    /// Returns an error if a metric cannot be registered: two of one name, which is a
    /// bug of this module.
    pub fn kept() -> anyhow::Result<Self> {
        let registry = Registry::new();
        let build_info = IntGaugeVec::new(
            Opts::new(
                "build_info",
                "The version and the commit the server was built from; always 1.",
            )
            .namespace(PREFIX),
            &["version", "commit"],
        )?;
        build_info
            .with_label_values(&[
                // The version of this build, which `--version` prints too, and not the
                // one `ping` reports for the original's web app (`SERVER_VERSION`).
                env!("CARGO_PKG_VERSION"),
                option_env!("FVOCI_BUILD_HASH").unwrap_or_default(),
            ])
            .set(1);
        registry.register(Box::new(build_info))?;
        #[cfg(target_os = "linux")]
        registry.register(Box::new(
            prometheus::process_collector::ProcessCollector::for_self(),
        ))?;
        let requests = RequestMetrics::register(&registry)?;
        let websocket = websocket::WebsocketMetrics::register(&registry)?;
        let storage = storage::StorageMetrics::register(&registry)?;
        let database = database::DatabaseMetrics::register(&registry)?;
        let background = background::BackgroundMetrics::register(&registry)?;
        Ok(Self {
            kept: Some(Arc::new(Kept {
                registry,
                requests,
                websocket,
                storage,
                database,
                background,
            })),
        })
    }

    /// Metrics that are not kept: every call on them does nothing.
    pub fn none() -> Self {
        Self::default()
    }

    /// Whether the metrics are kept.
    pub fn is_kept(&self) -> bool {
        self.kept.is_some()
    }

    /// The metrics in the Prometheus text format; `None` if they are not kept.
    pub(crate) fn render(&self) -> Option<anyhow::Result<Vec<u8>>> {
        let kept = self.kept.as_ref()?;
        let mut text = Vec::new();
        Some(
            TextEncoder::new()
                .encode(&kept.registry.gather(), &mut text)
                .map(|()| text)
                .context("encoding the metrics"),
        )
    }
}
