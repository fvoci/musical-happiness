//! Limits on the size of request bodies.
//!
//! A body over the limit is refused with 413 and the limit, before any handler runs and
//! without being kept in memory.

use std::time::Duration;

use axum::body::{Body, BodyDataStream};
use axum::extract::{Request, State};
use axum::http::header::CONTENT_LENGTH;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use bytes::BytesMut;
use futures_util::StreamExt as _;

use crate::error::AppError;

/// The largest body of a request to a route that needs no session: the login and the
/// registration. Anyone can send these, so they get no more room than they need; the
/// largest measured was 115 bytes.
pub(crate) const UNAUTHENTICATED_BODY_LIMIT: BodyLimit = BodyLimit(64 * 1024);

/// The largest text message a websocket connection may send: the commands are a few
/// hundred bytes of JSON.
pub(crate) const WEBSOCKET_MESSAGE_LIMIT: usize = 64 * 1024;

/// How much of a refused body is read and thrown away before the refusal is sent.
///
/// A client that is still sending its body when the answer comes, and finds the
/// connection closed under it, may never read the answer: in the tests one request in
/// nine of 4 MiB + 1 byte ended in a broken pipe instead of a 413. So the rest of the
/// body is taken off the wire first, without keeping it, up to this much and for at
/// most [`DISCARD_TIMEOUT`]. What is longer or slower is cut off.
const DISCARD_LIMIT: u64 = 8 * 1024 * 1024;

/// How long a refused body is read for before the refusal is sent regardless.
const DISCARD_TIMEOUT: Duration = Duration::from_secs(5);

/// The largest request body, in bytes, that the routes behind [`limit_request_body`]
/// read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BodyLimit(pub(crate) u64);

/// Reads the request body up to the limit and hands the request on with it, or answers
/// 413 with the limit.
///
/// A body that declares more than the limit is refused without any of it being kept.
/// One that declares nothing, or less than it sends, is refused as soon as it passes the
/// limit.
pub(crate) async fn limit_request_body(
    State(BodyLimit(limit)): State<BodyLimit>,
    request: Request,
    next: Next,
) -> Response {
    let declared = request
        .headers()
        .get(CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok());
    let (parts, body) = request.into_parts();
    let mut stream = body.into_data_stream();
    if declared.is_some_and(|length| length > limit) {
        discard(stream, 0).await;
        return AppError::payload_too_large(limit).into_response();
    }

    let mut read = BytesMut::new();
    while let Some(chunk) = stream.next().await {
        let chunk = match chunk {
            Ok(chunk) => chunk,
            Err(error) => {
                return AppError::bad_request(format_args!(
                    "the request body could not be read: {error}"
                ))
                .into_response();
            }
        };
        if byte_count(read.len()).saturating_add(byte_count(chunk.len())) > limit {
            drop(read);
            discard(stream, byte_count(chunk.len())).await;
            return AppError::payload_too_large(limit).into_response();
        }
        read.extend_from_slice(&chunk);
    }
    next.run(Request::from_parts(parts, Body::from(read.freeze())))
        .await
}

/// A length in memory as a number of bytes of a body. A `usize` fits everywhere this
/// server runs; where it would not, the length counts as more than any limit.
fn byte_count(length: usize) -> u64 {
    u64::try_from(length).unwrap_or(u64::MAX)
}

/// Reads what is left of a refused body and throws it away, so that the client can
/// finish sending and read the refusal. `discarded` is how much was thrown away already.
async fn discard(mut stream: BodyDataStream, mut discarded: u64) {
    let reading = async {
        while discarded <= DISCARD_LIMIT {
            match stream.next().await {
                Some(Ok(chunk)) => {
                    discarded = discarded.saturating_add(byte_count(chunk.len()));
                }
                Some(Err(_)) | None => break,
            }
        }
    };
    // Whether the body ended, was too long or too slow makes no difference to the answer.
    let _ = tokio::time::timeout(DISCARD_TIMEOUT, reading).await;
}

#[cfg(test)]
mod tests {
    use axum::Router;
    use axum::body::Bytes;
    use axum::http::StatusCode;
    use axum::middleware;
    use axum::routing::post;
    use tower::ServiceExt as _;

    use super::*;

    /// A route that answers with the length of the body it was given.
    fn limited(limit: u64) -> Router {
        Router::new()
            .route(
                "/",
                post(|body: Bytes| async move { body.len().to_string() }),
            )
            .layer(middleware::from_fn_with_state(
                BodyLimit(limit),
                limit_request_body,
            ))
    }

    async fn answer(router: Router, request: Request) -> (StatusCode, String) {
        let response = router.oneshot(request).await.expect("the router answers");
        let status = response.status();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("the body is read");
        (status, String::from_utf8_lossy(&body).into_owned())
    }

    #[tokio::test]
    async fn a_body_up_to_the_limit_reaches_the_handler_whole() {
        for length in [0, 1, 15, 16] {
            let request = Request::post("/")
                .body(Body::from(vec![b'x'; length]))
                .expect("a valid request");
            let (status, body) = answer(limited(16), request).await;
            assert_eq!(status, StatusCode::OK, "{length} bytes: {body}");
            assert_eq!(body, length.to_string(), "the handler got every byte");
        }
    }

    #[tokio::test]
    async fn a_body_that_declares_more_than_the_limit_is_refused_without_being_kept() {
        let request = Request::post("/")
            .header(CONTENT_LENGTH, "17")
            .body(Body::from(vec![b'x'; 17]))
            .expect("a valid request");
        let (status, body) = answer(limited(16), request).await;
        assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE, "{body}");
        let error: fvoci_model::ErrorBody = serde_json::from_str(&body).expect("an error body");
        assert_eq!(error.error, "payload_too_large");
        assert_eq!(error.limit, Some(16), "the answer names the limit");
    }

    #[tokio::test]
    async fn a_body_longer_than_what_is_thrown_away_is_refused_all_the_same() {
        // Twice the amount that is discarded, in pieces, without a declared length.
        let pieces = usize::try_from(2 * DISCARD_LIMIT / 65_536).expect("a small number");
        let chunks = futures_util::stream::iter(
            (0..pieces).map(|_| Ok::<_, std::io::Error>(Bytes::from(vec![b'x'; 65_536]))),
        );
        let request = Request::post("/")
            .body(Body::from_stream(chunks))
            .expect("a valid request");
        let (status, body) = answer(limited(16), request).await;
        assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE, "{body}");
    }

    #[tokio::test]
    async fn a_body_that_declares_no_length_is_refused_once_it_passes_the_limit() {
        // A stream has no length to declare.
        let chunks = futures_util::stream::iter(
            (0..4).map(|_| Ok::<_, std::io::Error>(Bytes::from(vec![b'x'; 8]))),
        );
        let request = Request::post("/")
            .body(Body::from_stream(chunks))
            .expect("a valid request");
        let (status, body) = answer(limited(16), request).await;
        assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE, "{body}");
        let error: fvoci_model::ErrorBody = serde_json::from_str(&body).expect("an error body");
        assert_eq!(error.limit, Some(16), "the answer names the limit");
    }
}
