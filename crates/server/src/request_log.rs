//! The request log and the containment of handler panics.

use std::net::{IpAddr, SocketAddr};
use std::panic::AssertUnwindSafe;
use std::time::Instant;

use axum::extract::connect_info::Connected;
use axum::extract::{ConnectInfo, Request, State};
use axum::http::HeaderMap;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::serve::IncomingStream;
use futures_util::FutureExt as _;

use crate::error::AppError;

/// The header a reverse proxy lists the client's address in.
const X_FORWARDED_FOR: &str = "x-forwarded-for";

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

/// Which address the request log names as the client.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ClientAddress {
    /// The address the connection comes from.
    SocketPeer,
    /// The first address of `X-Forwarded-For` if there is one, for a server behind a
    /// proxy that sets the header. Anyone can send the header, so it is only believed
    /// on request (`trust_forwarded_for`).
    ForwardedFor,
}

/// Returns the client of a request as the log shows it.
fn client_address(source: ClientAddress, headers: &HeaderMap, peer: Option<Peer>) -> String {
    let forwarded = match source {
        ClientAddress::SocketPeer => None,
        ClientAddress::ForwardedFor => forwarded_for(headers),
    };
    match (forwarded, peer) {
        (Some(address), _) => address.to_string(),
        (None, Some(Peer::Tcp(address))) => address.ip().to_string(),
        (None, Some(Peer::Unix)) => "unix socket".to_owned(),
        (None, None) => "unknown".to_owned(),
    }
}

/// The first address of `X-Forwarded-For`. Anything that is not an address is ignored
/// rather than copied into the log.
fn forwarded_for(headers: &HeaderMap) -> Option<IpAddr> {
    let value = headers.get(X_FORWARDED_FOR)?.to_str().ok()?;
    let first = value.split(',').next()?;
    first.trim().parse().ok()
}

/// Logs every request: method, path, status, duration and client address.
///
/// The query string is left out because it can carry `access_token` and `read_token`,
/// and no header is logged, so neither are the session token and the cookies.
pub(crate) async fn log_request(
    State(source): State<ClientAddress>,
    request: Request,
    next: Next,
) -> Response {
    let started = Instant::now();
    let method = request.method().clone();
    let path = request.uri().path().to_owned();
    let peer = request
        .extensions()
        .get::<ConnectInfo<Peer>>()
        .map(|ConnectInfo(peer)| *peer);
    let client = client_address(source, request.headers(), peer);

    let response = next.run(request).await;

    // Whole microseconds, shown as milliseconds.
    let duration_ms = (started.elapsed().as_secs_f64() * 1_000_000.0).round() / 1000.0;
    tracing::info!(
        %method,
        path,
        status = response.status().as_u16(),
        duration_ms,
        client,
        "request"
    );
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

    #[test]
    fn client_is_the_socket_peer_by_default() {
        let headers = forwarded("203.0.113.7");
        assert_eq!(
            client_address(ClientAddress::SocketPeer, &headers, Some(socket_peer())),
            "192.0.2.1",
            "the header is not believed unless the configuration says so"
        );
        assert_eq!(
            client_address(ClientAddress::SocketPeer, &headers, Some(Peer::Unix)),
            "unix socket"
        );
        assert_eq!(
            client_address(ClientAddress::SocketPeer, &headers, None),
            "unknown"
        );
    }

    #[test]
    fn client_is_the_first_forwarded_address_when_trusted() {
        let source = ClientAddress::ForwardedFor;
        let socket = Some(socket_peer());
        assert_eq!(
            client_address(source, &forwarded("203.0.113.7"), socket),
            "203.0.113.7"
        );
        assert_eq!(
            client_address(
                source,
                &forwarded(" 2001:db8::7 , 10.0.0.1, 10.0.0.2"),
                socket
            ),
            "2001:db8::7",
            "the first of several"
        );
        assert_eq!(
            client_address(source, &HeaderMap::new(), socket),
            "192.0.2.1",
            "without the header the socket peer is named"
        );
        for junk in ["", "unknown", "203.0.113.7:1234", "evil\tclient=10.0.0.1"] {
            assert_eq!(
                client_address(source, &forwarded(junk), socket),
                "192.0.2.1",
                "{junk:?} is not an address"
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
    async fn logged(source: ClientAddress, request: Request) -> (StatusCode, String, String) {
        let captured = Captured::default();
        let writer = captured.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .with_writer(move || writer.clone())
            .finish();
        let _guard = tracing::subscriber::set_default(subscriber);

        let router = Router::new()
            .route("/api/v2/boards/{board_id}", get(|| async { "fine" }))
            .route("/api/v2/boom", get(boom))
            .layer(middleware::from_fn(contain_panics))
            .layer(middleware::from_fn_with_state(source, log_request));
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
            Request::get("/api/v2/boards/b1?read_token=SECRET-READ&access_token=SECRET-ACCESS")
                .header("Authorization", "Bearer SECRET-BEARER")
                .header("Cookie", "FVOCIAUTHTOKEN=SECRET-COOKIE")
                .header(X_FORWARDED_FOR, "203.0.113.7")
                .body(Body::empty())
                .expect("a valid request");
        request.extensions_mut().insert(ConnectInfo(socket_peer()));

        let (status, body, log) = logged(ClientAddress::SocketPeer, request).await;

        assert_eq!(status, StatusCode::OK, "{body}");
        for expected in [
            "request",
            "method=GET",
            "path=\"/api/v2/boards/b1\"",
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
    async fn request_log_names_the_forwarded_client_when_trusted() {
        let mut request = Request::get("/api/v2/boards/b1")
            .header(X_FORWARDED_FOR, "203.0.113.7, 10.0.0.1")
            .body(Body::empty())
            .expect("a valid request");
        request.extensions_mut().insert(ConnectInfo(socket_peer()));

        let (_, _, log) = logged(ClientAddress::ForwardedFor, request).await;

        assert!(log.contains("client=\"203.0.113.7\""), "{log}");
    }

    /// `panicHandler` of `server/api/api.go`: a panic becomes the standard 500, and
    /// the request is still logged.
    #[tokio::test]
    async fn handler_panic_becomes_the_internal_server_error_response() {
        let request = Request::get("/api/v2/boom?access_token=SECRET-ACCESS")
            .body(Body::empty())
            .expect("a valid request");

        let (status, body, log) = logged(ClientAddress::SocketPeer, request).await;

        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{body}");
        assert_eq!(
            body, r#"{"error":"internal server error","errorCode":500}"#,
            "the standard body"
        );
        assert!(log.contains("http handler panic"), "{log}");
        assert!(log.contains("panic=\"boom\""), "{log}");
        assert!(log.contains("status=500"), "{log}");
        assert!(!log.contains("SECRET"), "the query is not logged: {log}");
    }
}
