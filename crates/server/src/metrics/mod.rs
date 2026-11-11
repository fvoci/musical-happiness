//! The Prometheus metrics: kept only if `prometheusaddress` is set, and served in the
//! Prometheus text format at `/metrics` on a listener of their own, never on the API's.
//!
//! Every name starts with [`PREFIX`], but for the process metrics, which have the names
//! every Prometheus client gives them (`process_cpu_seconds_total` and so on). No label
//! carries an identifier of a user, a board, a team or a file, nor a path as it was
//! asked: whatever a client sends maps onto a set of values fixed in this code.
//!
//! The listener has no authentication. It is meant for an address only the monitoring
//! can reach (`docs/configuration.md`, "키: 지표").

use std::sync::Arc;

use anyhow::Context as _;
use axum::Router;
use axum::http::HeaderValue;
use axum::http::header::CONTENT_TYPE;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use prometheus::{Encoder as _, IntGaugeVec, Opts, Registry, TextEncoder};

use crate::app::App;
use crate::config::MetricsAddress;
use crate::error::AppError;

mod background;
mod http;
mod storage;
mod websocket;

pub(crate) use background::{Job, JobOutcome, LoginLimit};
pub(crate) use http::{Measured, measure_requests, note_route};
pub(crate) use websocket::Disconnect;

/// The prefix of the name of every metric of this server's own.
pub const PREFIX: &str = "fvoci";

/// The path the metrics are served at, on the metrics listener.
pub const PATH: &str = "/metrics";

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
    http: http::HttpMetrics,
    websocket: websocket::WebsocketMetrics,
    storage: storage::StorageMetrics,
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
        let http = http::HttpMetrics::register(&registry)?;
        let websocket = websocket::WebsocketMetrics::register(&registry)?;
        let storage = storage::StorageMetrics::register(&registry)?;
        let background = background::BackgroundMetrics::register(&registry)?;
        Ok(Self {
            kept: Some(Arc::new(Kept {
                registry,
                http,
                websocket,
                storage,
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
    fn render(&self) -> Option<anyhow::Result<Vec<u8>>> {
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

/// The router of the metrics listener: [`PATH`] and nothing else of the server. No
/// route of the API, the websocket or the web app is on it. What it refuses it refuses
/// as the API does, with a problem (RFC 9457), and so it serves the pages such an
/// answer points at; without the identifier of a request, as nothing is logged here.
pub(crate) fn router(app: App) -> Router {
    Router::new()
        .route(PATH, get(answer_scrape))
        .fallback(|| async { AppError::not_found("route") })
        .method_not_allowed_fallback(|| async { AppError::method_not_allowed() })
        .with_state(app)
        .merge(crate::problems::routes())
}

/// Answers a scrape: reads what is measured by asking, then writes all.
async fn answer_scrape(axum::extract::State(app): axum::extract::State<App>) -> Response {
    let metrics = app.metrics();
    metrics.read_storage(&app).await;
    match metrics.render() {
        Some(Ok(text)) => (
            [(
                CONTENT_TYPE,
                HeaderValue::from_static(prometheus::TEXT_FORMAT),
            )],
            text,
        )
            .into_response(),
        // The cause is logged where the error becomes its answer, and not sent.
        Some(Err(error)) => {
            AppError::from(error.context("unable to answer a scrape")).into_response()
        }
        None => AppError::not_found("route").into_response(),
    }
}

/// Opens the metrics listener at `address`. A host of none listens on every interface,
/// IPv6 and IPv4 where the system maps one onto the other, as the API's listener does.
///
/// # Errors
///
/// Returns an error if the host cannot be resolved, or the address cannot be listened
/// on: taken, or not this machine's.
pub(crate) async fn bind(address: &MetricsAddress) -> anyhow::Result<tokio::net::TcpListener> {
    let port = address.port;
    let listener = match &address.host {
        Some(host) => tokio::net::TcpListener::bind((host.as_str(), port)).await,
        None => match tokio::net::TcpListener::bind(("::", port)).await {
            Ok(listener) => Ok(listener),
            Err(_) => tokio::net::TcpListener::bind(("0.0.0.0", port)).await,
        },
    };
    let shown = match &address.host {
        Some(host) if host.contains(':') => format!("[{host}]:{port}"),
        Some(host) => format!("{host}:{port}"),
        None => format!(":{port}"),
    };
    listener
        .with_context(|| format!("listening for metrics scrapes on {shown} (prometheusaddress)"))
}
