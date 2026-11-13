//! The HTTP APIs of the mail providers, faked for the tests: the sending requests of
//! Cloudflare are recorded with what they were authorized with, and answered as it
//! answers, by default as taken, or with the statuses the test queues (`429`, `500`,
//! `401`, …) in its own shape.
#![allow(dead_code, reason = "each provider's tests use a part of it")]

use std::collections::VecDeque;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::body::Bytes;
use axum::http::{HeaderMap, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use fvoci_server::config::mail::{MailSettings, MailTransport};
use fvoci_server::config::smtp::SmtpSettings;
use serde_json::{Value, json};
use tokio::net::TcpListener;
use tokio::sync::{Mutex, Notify};
use tokio::task::JoinHandle;

/// The providers, by the path their sending request goes to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Provider {
    Cloudflare,
}

/// The account of the fake Cloudflare.
pub(crate) const CLOUDFLARE_ACCOUNT: &str = "0123456789abcdef0123456789abcdef";
/// Its API token.
pub(crate) const CLOUDFLARE_TOKEN: &str = "fake-cloudflare-token_0123456789";

/// One request the fake was sent.
#[derive(Debug, Clone)]
pub(crate) struct Recorded {
    pub(crate) provider: Provider,
    pub(crate) headers: HeaderMap,
    /// The body as JSON.
    pub(crate) body: Value,
    /// The one recipient.
    pub(crate) to: String,
    /// The plain text of the message.
    pub(crate) text: String,
    /// Whether the request carried the provider's signature, in its shape.
    pub(crate) signed: bool,
    /// The status it was answered with.
    pub(crate) status: u16,
}

#[derive(Default)]
struct State {
    recorded: Mutex<Vec<Recorded>>,
    /// The statuses of the next answers, each with the `Retry-After` it gives in
    /// seconds; an empty queue answers as taken.
    script: Mutex<VecDeque<(u16, Option<u64>)>>,
    changed: Notify,
}

pub(crate) struct FakeProvider {
    pub(crate) address: SocketAddr,
    state: Arc<State>,
    serving: JoinHandle<()>,
}

impl FakeProvider {
    pub(crate) async fn start() -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("a local port is free");
        let address = listener.local_addr().expect("the listener's address");
        let state = Arc::new(State::default());
        let serving_state = Arc::clone(&state);
        let app = Router::new().fallback(move |uri: Uri, headers: HeaderMap, body: Bytes| {
            let state = Arc::clone(&serving_state);
            async move { answer(&state, &uri, headers, &body).await }
        });
        let serving = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        Self {
            address,
            state,
            serving,
        }
    }

    /// `mail_api_endpoint` for this fake.
    pub(crate) fn endpoint(&self) -> String {
        format!("http://{}", self.address)
    }

    /// The settings of a server that sends through this fake as `provider`: the limits
    /// of all mail, and the way. The pace has tests of its own; these send without
    /// waiting for it.
    pub(crate) fn settings(&self, provider: Provider) -> (SmtpSettings, MailSettings) {
        let smtp = SmtpSettings {
            smtp_from: "sender@example.test".to_owned(),
            mail_per_second: 1000,
            ..SmtpSettings::default()
        };
        let mail = match provider {
            Provider::Cloudflare => MailSettings {
                mail_transport: Some(MailTransport::Cloudflare),
                mail_api_endpoint: self.endpoint(),
                cloudflare_account_id: CLOUDFLARE_ACCOUNT.to_owned(),
                cloudflare_api_token: CLOUDFLARE_TOKEN.to_owned(),
            },
        };
        (smtp, mail)
    }

    /// Answers the next requests with these statuses, in order, then as taken; a `429`
    /// says `Retry-After: 0`.
    pub(crate) async fn answer_next(&self, statuses: &[u16]) {
        self.state.script.lock().await.extend(
            statuses
                .iter()
                .map(|&status| (status, (status == 429).then_some(0))),
        );
    }

    /// Answers the next request with `status` and `Retry-After: seconds`.
    pub(crate) async fn answer_next_after(&self, status: u16, seconds: u64) {
        self.state
            .script
            .lock()
            .await
            .push_back((status, Some(seconds)));
    }

    pub(crate) async fn recorded(&self) -> Vec<Recorded> {
        self.state.recorded.lock().await.clone()
    }

    /// Waits until `count` requests were answered as taken; returns those.
    pub(crate) async fn taken(&self, count: usize) -> Vec<Recorded> {
        tokio::time::timeout(Duration::from_secs(20), async {
            loop {
                let changed = self.state.changed.notified();
                let taken: Vec<Recorded> = self
                    .recorded()
                    .await
                    .into_iter()
                    .filter(|recorded| (200..300).contains(&recorded.status))
                    .collect();
                if taken.len() >= count {
                    return taken;
                }
                changed.await;
            }
        })
        .await
        .expect("the provider is sent the messages")
    }

    /// Waits until `count` requests arrived, whatever they were answered.
    pub(crate) async fn requests(&self, count: usize) -> Vec<Recorded> {
        tokio::time::timeout(Duration::from_secs(20), async {
            loop {
                let changed = self.state.changed.notified();
                let recorded = self.recorded().await;
                if recorded.len() >= count {
                    return recorded;
                }
                changed.await;
            }
        })
        .await
        .expect("the provider is sent the requests")
    }

    pub(crate) fn stop(self) {
        self.serving.abort();
    }
}

async fn answer(state: &State, uri: &Uri, headers: HeaderMap, body: &[u8]) -> Response {
    let path = uri.path();
    let provider = if path == format!("/client/v4/accounts/{CLOUDFLARE_ACCOUNT}/email/sending/send")
    {
        Provider::Cloudflare
    } else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let json: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let (to, text, signed) = match provider {
        Provider::Cloudflare => (
            json["to"].as_str().unwrap_or_default().to_owned(),
            json["text"].as_str().unwrap_or_default().to_owned(),
            header(&headers, "authorization") == format!("Bearer {CLOUDFLARE_TOKEN}"),
        ),
    };
    let (status, retry_after) = state.script.lock().await.pop_front().unwrap_or((200, None));
    let mut response = match (provider, status) {
        (Provider::Cloudflare, 200) => (
            StatusCode::OK,
            axum::Json(json!({
                "success": true, "errors": [], "messages": [],
                "result": {"delivered": [to], "permanent_bounces": [], "queued": []},
            })),
        )
            .into_response(),
        (Provider::Cloudflare, status) => cloudflare_error(status),
    };
    if let Some(seconds) = retry_after {
        response.headers_mut().insert(
            "retry-after",
            seconds.to_string().parse().expect("a header value"),
        );
    }
    state.recorded.lock().await.push(Recorded {
        provider,
        headers,
        body: json,
        to,
        text,
        signed,
        status,
    });
    state.changed.notify_waiters();
    response
}

fn header(headers: &HeaderMap, name: &str) -> String {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_owned()
}

/// Cloudflare's error answer of `status`, with the code its documentation gives.
fn cloudflare_error(status: u16) -> Response {
    let (code, message) = match status {
        400 => (10001, "email.sending.error.invalid_request_schema"),
        401 => (10101, "email.sending.error.authentication.unauthorized"),
        403 => (10102, "email.sending.error.authentication.forbidden"),
        429 => (10004, "email.sending.error.throttled"),
        _ => (10002, "email.sending.error.internal_server"),
    };
    (
        StatusCode::from_u16(status).expect("a status"),
        axum::Json(json!({
            "success": false, "errors": [{"code": code, "message": message}],
            "messages": [], "result": null,
        })),
    )
        .into_response()
}
