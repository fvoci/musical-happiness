//! The metrics listener: the metrics served at [`PATH`] on a listener of their own,
//! never on the API's.
//!
//! A scrape must send the bearer token of `prometheustoken` if it is set, and needs
//! nothing if it is empty; the listener is meant for an address only the monitoring
//! can reach either way (`docs/configuration.md`, "키: 지표").

use anyhow::Context as _;
use axum::Router;
use axum::extract::{Request, State};
use axum::http::header::{AUTHORIZATION, CONTENT_TYPE, WWW_AUTHENTICATE};
use axum::http::{HeaderMap, HeaderValue};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::get;

use crate::app::App;
use crate::config::{MetricsAddress, MetricsToken};
use crate::error::AppError;
use crate::metrics;

/// The path the metrics are served at, on the metrics listener.
const PATH: &str = "/metrics";

/// The router of the metrics listener: [`PATH`] and nothing else of the server. No
/// route of the API, the websocket or the web app is on it. What it refuses it refuses
/// as the API does, with a problem (RFC 9457), and so it serves the pages such an
/// answer points at; without the identifier of a request, as nothing is logged here.
pub(crate) fn router(app: App) -> Router {
    let token = app.config().metrics_token();
    Router::new()
        .route(PATH, get(answer_scrape))
        .route_layer(middleware::from_fn_with_state(token, require_token))
        .fallback(|| async { AppError::not_found("route") })
        .method_not_allowed_fallback(|| async { AppError::method_not_allowed() })
        .with_state(app)
        .merge(crate::problems::routes())
}

/// Lets a scrape through if it sends `token` as its bearer token, or if there is none
/// to send; else answers 401, before anything is read for it. The token is compared in
/// constant time and written nowhere.
async fn require_token(
    State(token): State<Option<MetricsToken>>,
    request: Request,
    next: Next,
) -> Response {
    let Some(token) = &token else {
        return next.run(request).await;
    };
    let challenge = match bearer(request.headers()) {
        Some(presented) if token.admits(presented) => return next.run(request).await,
        // A token was sent, and is not this one (RFC 6750, 3.1).
        Some(_) => "Bearer realm=\"metrics\", error=\"invalid_token\"",
        None => "Bearer realm=\"metrics\"",
    };
    let mut refused = AppError::unauthorized("a scrape needs the bearer token").into_response();
    refused
        .headers_mut()
        .insert(WWW_AUTHENTICATE, HeaderValue::from_static(challenge));
    refused
}

/// The bearer token of the `Authorization` header (RFC 6750): after the scheme, which is
/// read without regard to case, and the spaces.
fn bearer(headers: &HeaderMap) -> Option<&str> {
    let value = headers.get(AUTHORIZATION)?.to_str().ok()?;
    let (scheme, token) = value.split_once(' ')?;
    scheme
        .eq_ignore_ascii_case("bearer")
        .then(|| token.trim_start_matches(' '))
}

/// Answers a scrape: reads what is measured by asking, then writes all.
async fn answer_scrape(axum::extract::State(app): axum::extract::State<App>) -> Response {
    // Reads of the database side by side make a large future: on the heap.
    Box::pin(async { tokio::join!(app.readiness(), app.read_storage_metrics()) }).await;
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

#[cfg(test)]
mod tests {
    use std::io;
    use std::sync::{Arc, Mutex};

    use axum::body::Body;
    use axum::http::StatusCode;
    use tower::ServiceExt as _;

    use super::*;

    const TOKEN: &str = "the-token-of-the-scrapes";

    /// What a subscriber of the log writes.
    #[derive(Clone, Default)]
    struct Captured(Arc<Mutex<Vec<u8>>>);

    impl io::Write for Captured {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0
                .lock()
                .expect("lock poisoned")
                .extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    /// The status of a scrape that sends `authorization`, and the log of it at every
    /// level.
    async fn scraped(token: Option<&str>, authorization: Option<&str>) -> (StatusCode, String) {
        let captured = Captured::default();
        let writer = captured.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .with_max_level(tracing::Level::TRACE)
            .with_writer(move || writer.clone())
            .finish();
        let _logging = tracing::subscriber::set_default(subscriber);
        let config = crate::config::Configuration {
            prometheus_token: token.unwrap_or_default().to_owned(),
            ..crate::config::Configuration::default()
        };
        let router = Router::new()
            .route(PATH, get(|| async { "the metrics" }))
            .route_layer(middleware::from_fn_with_state(
                config.metrics_token(),
                require_token,
            ));
        let mut request = Request::get(PATH);
        if let Some(value) = authorization {
            request = request.header(AUTHORIZATION, value);
        }
        let response = router
            .oneshot(request.body(Body::empty()).expect("a request"))
            .await
            .expect("an answer");
        let status = response.status();
        if status == StatusCode::UNAUTHORIZED {
            let challenge = response
                .headers()
                .get(WWW_AUTHENTICATE)
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default();
            assert!(
                challenge.starts_with("Bearer realm=\"metrics\""),
                "{challenge}"
            );
        }
        let log = String::from_utf8_lossy(&captured.0.lock().expect("lock poisoned")).into_owned();
        (status, log)
    }

    #[tokio::test]
    async fn with_a_token_only_a_scrape_that_sends_it_is_answered() {
        let right = format!("Bearer {TOKEN}");
        for (authorization, status) in [
            (None, StatusCode::UNAUTHORIZED),
            (Some("Bearer"), StatusCode::UNAUTHORIZED),
            (Some("Bearer "), StatusCode::UNAUTHORIZED),
            (Some("Bearer the-token"), StatusCode::UNAUTHORIZED),
            (
                Some(&format!("Basic {TOKEN}")[..]),
                StatusCode::UNAUTHORIZED,
            ),
            (Some(TOKEN), StatusCode::UNAUTHORIZED),
            (Some(&right[..]), StatusCode::OK),
            (Some(&format!("bearer  {TOKEN}")[..]), StatusCode::OK),
        ] {
            let (got, log) = scraped(Some(TOKEN), authorization).await;
            assert_eq!(got, status, "{authorization:?}");
            assert!(!log.contains(TOKEN), "the token is not logged: {log}");
            assert!(!log.contains("the-token"), "nor a guess: {log}");
        }
    }

    #[tokio::test]
    async fn without_a_token_every_scrape_is_answered() {
        for authorization in [None, Some("Bearer anything")] {
            let (status, _) = scraped(None, authorization).await;
            assert_eq!(status, StatusCode::OK, "{authorization:?}");
        }
    }
}
