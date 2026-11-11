//! The metrics listener: the metrics served at [`PATH`] on a listener of their own,
//! never on the API's.
//!
//! The listener has no authentication. It is meant for an address only the monitoring
//! can reach (`docs/configuration.md`, "키: 지표").

use anyhow::Context as _;
use axum::Router;
use axum::http::HeaderValue;
use axum::http::header::CONTENT_TYPE;
use axum::response::{IntoResponse, Response};
use axum::routing::get;

use crate::app::App;
use crate::config::MetricsAddress;
use crate::error::AppError;
use crate::metrics;

/// The path the metrics are served at, on the metrics listener.
const PATH: &str = "/metrics";

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
    app.read_storage_metrics().await;
    match app.metrics().render() {
        Some(Ok(text)) => (
            [(CONTENT_TYPE, HeaderValue::from_static(metrics::TEXT_FORMAT))],
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
