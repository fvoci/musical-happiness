//! The request log and the containment of handler panics.

use std::net::SocketAddr;
use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use std::time::Instant;

use axum::body::Body;
use axum::extract::connect_info::Connected;
use axum::extract::{ConnectInfo, Request, State};
use axum::http::header::CONTENT_LENGTH;
use axum::http::{HeaderMap, HeaderValue};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::serve::IncomingStream;
use futures_util::FutureExt as _;
use uuid::Uuid;

use crate::client_address::{ClientAddress, TrustedProxies};
use crate::error::{AppError, ReportedError};

/// The response header that carries the identifier of the request.
const X_REQUEST_ID: &str = "x-request-id";

/// Where a connection comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Peer {
    /// A TCP connection from this address.
    Tcp(SocketAddr),
    /// A connection through the administration socket.
    Unix,
}

impl Connected<IncomingStream<'_, tokio::net::TcpListener>> for Peer {
    fn connect_info(stream: IncomingStream<'_, tokio::net::TcpListener>) -> Self {
        Self::Tcp(*stream.remote_addr())
    }
}

#[cfg(unix)]
impl Connected<IncomingStream<'_, tokio::net::UnixListener>> for Peer {
    fn connect_info(_stream: IncomingStream<'_, tokio::net::UnixListener>) -> Self {
        Self::Unix
    }
}

/// Returns the client of a request: its address, also behind the reverse proxies the
/// configuration trusts, and how the log shows it.
fn client_of(
    proxies: &TrustedProxies,
    headers: &HeaderMap,
    peer: Option<Peer>,
) -> (ClientAddress, String) {
    match peer {
        Some(Peer::Tcp(peer)) => {
            let address = proxies.client_of(peer.ip(), headers);
            (ClientAddress(Some(address)), address.to_string())
        }
        Some(Peer::Unix) => (ClientAddress(None), "unix socket".to_owned()),
        None => (ClientAddress(None), "unknown".to_owned()),
    }
}

/// Logs every request: method, path, status, duration, client address and the
/// identifier it gave the request.
///
/// The identifier is a random one of this server's making. It goes into the log line,
/// into the `x-request-id` response header and into the body of an error answer, so that
/// what a user reports can be found in the log. An identifier the client sends is not
/// taken over: it would let a client write into the log.
///
/// The query string is left out because it can carry `access_token` and `read_token`,
/// and no header is logged, so neither are the session token and the cookies.
pub(crate) async fn log_request(
    State(proxies): State<Arc<TrustedProxies>>,
    mut request: Request,
    next: Next,
) -> Response {
    let started = Instant::now();
    let request_id = Uuid::now_v7().to_string();
    let method = request.method().clone();
    let path = request.uri().path().to_owned();
    let peer = request
        .extensions()
        .get::<ConnectInfo<Peer>>()
        .map(|ConnectInfo(peer)| *peer);
    let (address, client) = client_of(&proxies, request.headers(), peer);
    // For the handlers that count by where a request comes from: the same address the
    // log shows.
    request.extensions_mut().insert(address);

    let response = identify(next.run(request).await, &request_id);

    // Whole microseconds, shown as milliseconds.
    let duration_ms = (started.elapsed().as_secs_f64() * 1_000_000.0).round() / 1000.0;
    tracing::info!(
        %method,
        path,
        status = response.status().as_u16(),
        duration_ms,
        client,
        request_id,
        "request"
    );
    response
}

/// Puts the identifier of the request on its response: into the header, and into the
/// body if the response is an error of this server's own format.
fn identify(mut response: Response, request_id: &str) -> Response {
    if let Some(ReportedError(mut body)) = response.extensions_mut().remove::<ReportedError>() {
        body.request_id = Some(request_id.to_owned());
        // An error body is a handful of strings; serializing it cannot fail.
        if let Ok(bytes) = serde_json::to_vec(&body) {
            let (mut parts, _) = response.into_parts();
            parts.headers.remove(CONTENT_LENGTH);
            response = Response::from_parts(parts, Body::from(bytes));
        }
    }
    if let Ok(value) = HeaderValue::from_str(request_id) {
        response.headers_mut().insert(X_REQUEST_ID, value);
    }
    response
}

/// Answers with the standard 500 when a handler panics, instead of dropping the
/// connection; the original's `panicHandler` (`server/api/api.go:117-132`).
pub(crate) async fn contain_panics(request: Request, next: Next) -> Response {
    let path = request.uri().path().to_owned();
    match AssertUnwindSafe(next.run(request)).catch_unwind().await {
        Ok(response) => response,
        Err(payload) => {
            let message = payload
                .downcast_ref::<&str>()
                .copied()
                .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
                .unwrap_or("a panic without a message");
            tracing::error!(panic = message, path, "http handler panic");
            AppError::internal("http handler panic").into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io;
    use std::sync::{Arc, Mutex};

    use axum::Router;
    use axum::body::Body;
    use axum::http::StatusCode;
    use axum::middleware;
    use axum::routing::get;
    use tower::ServiceExt as _;

    use super::*;

    const X_FORWARDED_FOR: &str = "x-forwarded-for";

    /// The peer the connections of these tests come from.
    fn socket_peer() -> Peer {
        Peer::Tcp("192.0.2.1:5000".parse().expect("a socket address"))
    }

    async fn boom() -> &'static str {
        panic!("boom");
    }

    fn forwarded(value: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            X_FORWARDED_FOR,
            value.parse().expect("a valid header value"),
        );
        headers
    }

    fn proxies(ranges: &[&str]) -> TrustedProxies {
        let ranges: Vec<String> = ranges.iter().map(|range| (*range).to_owned()).collect();
        TrustedProxies::parse(&ranges).expect("valid ranges")
    }

    #[test]
    fn client_is_the_socket_peer_unless_the_peer_is_a_trusted_proxy() {
        let headers = forwarded("203.0.113.7");
        let nobody = proxies(&[]);
        let peer_address = "192.0.2.1".parse().expect("an address");
        assert_eq!(
            client_of(&nobody, &headers, Some(socket_peer())),
            (ClientAddress(Some(peer_address)), "192.0.2.1".to_owned()),
            "the header is not believed unless it comes from a trusted proxy"
        );
        assert_eq!(
            client_of(&nobody, &headers, Some(Peer::Unix)),
            (ClientAddress(None), "unix socket".to_owned())
        );
        assert_eq!(
            client_of(&nobody, &headers, None),
            (ClientAddress(None), "unknown".to_owned())
        );

        let trusting = proxies(&["192.0.2.0/24"]);
        let (address, shown) = client_of(
            &trusting,
            &forwarded("1.2.3.4, 2001:db8::7"),
            Some(socket_peer()),
        );
        assert_eq!(
            shown, "2001:db8::7",
            "what the proxy added, not the left end"
        );
        assert_eq!(
            address,
            ClientAddress(Some("2001:db8::7".parse().expect("an address")))
        );
        for junk in ["", "unknown", "203.0.113.7:1234", "evil\tclient=10.0.0.1"] {
            assert_eq!(
                client_of(&trusting, &forwarded(junk), Some(socket_peer())).1,
                "192.0.2.1",
                "{junk:?} is not an address and is not copied into the log"
            );
        }
    }

    /// A log output that tests can read back.
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

    impl Captured {
        fn text(&self) -> String {
            String::from_utf8_lossy(&self.0.lock().expect("lock poisoned")).into_owned()
        }
    }

    /// Sends one request through both layers and returns the status, the body and
    /// what was logged.
    async fn logged(proxies: TrustedProxies, request: Request) -> (StatusCode, String, String) {
        let captured = Captured::default();
        let writer = captured.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .with_writer(move || writer.clone())
            .finish();
        let _guard = tracing::subscriber::set_default(subscriber);

        let router = Router::new()
            .route("/api/v1/boards/{board_id}", get(|| async { "fine" }))
            .route("/api/v1/boom", get(boom))
            .layer(middleware::from_fn(contain_panics))
            .layer(middleware::from_fn_with_state(
                Arc::new(proxies),
                log_request,
            ));
        let response = router
            .oneshot(request)
            .await
            .expect("the router does not fail");
        let status = response.status();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("the body is read");
        (
            status,
            String::from_utf8_lossy(&body).into_owned(),
            captured.text(),
        )
    }

    #[tokio::test]
    async fn request_log_has_no_query_string_and_no_credentials() {
        let mut request =
            Request::get("/api/v1/boards/b1?read_token=SECRET-READ&access_token=SECRET-ACCESS")
                .header("Authorization", "Bearer SECRET-BEARER")
                .header("Cookie", "FVOCIAUTHTOKEN=SECRET-COOKIE")
                .header(X_FORWARDED_FOR, "203.0.113.7")
                .body(Body::empty())
                .expect("a valid request");
        request.extensions_mut().insert(ConnectInfo(socket_peer()));

        let (status, body, log) = logged(proxies(&[]), request).await;

        assert_eq!(status, StatusCode::OK, "{body}");
        for expected in [
            "request",
            "method=GET",
            "path=\"/api/v1/boards/b1\"",
            "status=200",
            "duration_ms=",
            "client=\"192.0.2.1\"",
        ] {
            assert!(log.contains(expected), "{expected:?} is logged: {log}");
        }
        for secret in ["SECRET", "read_token", "access_token", "203.0.113.7"] {
            assert!(!log.contains(secret), "{secret:?} is not logged: {log}");
        }
    }

    #[tokio::test]
    async fn request_log_names_the_forwarded_client_behind_trusted_proxies() {
        let mut request = Request::get("/api/v1/boards/b1")
            .header(X_FORWARDED_FOR, "203.0.113.7, 10.0.0.1")
            .body(Body::empty())
            .expect("a valid request");
        request.extensions_mut().insert(ConnectInfo(socket_peer()));

        let (_, _, log) = logged(proxies(&["192.0.2.1", "10.0.0.0/8"]), request).await;

        assert!(log.contains("client=\"203.0.113.7\""), "{log}");
    }

    /// `panicHandler` of `server/api/api.go`: a panic becomes the standard 500, and
    /// the request is still logged.
    #[tokio::test]
    async fn handler_panic_becomes_the_internal_server_error_response() {
        let request = Request::get("/api/v1/boom?access_token=SECRET-ACCESS")
            .body(Body::empty())
            .expect("a valid request");

        let (status, body, log) = logged(proxies(&[]), request).await;

        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{body}");
        let error: fvoci_model::ErrorBody =
            serde_json::from_str(&body).expect("the standard error body");
        assert_eq!(error.error, "internal", "{body}");
        assert_eq!(error.message, "internal server error", "{body}");
        let request_id = error.request_id.expect("the identifier of the request");
        assert_eq!(request_id.len(), 36, "a random identifier: {request_id}");
        assert!(
            log.contains(&format!("request_id=\"{request_id}\"")),
            "the log line of the request carries the identifier of the answer: {log}"
        );
        assert!(log.contains("http handler panic"), "{log}");
        assert!(log.contains("panic=\"boom\""), "{log}");
        assert!(log.contains("status=500"), "{log}");
        assert!(!log.contains("SECRET"), "the query is not logged: {log}");
    }
}
