//! The HTTP APIs of the mail providers, faked for the tests: the sending requests of
//! Cloudflare and Amazon SES are recorded with whether they were authorized or signed
//! as the provider checks it, and answered as it answers: by default as taken, or with
//! the statuses (and SES's error names) the test queues, in its own shape.
#![allow(dead_code, reason = "each provider's tests use a part of it")]

use std::collections::VecDeque;
use std::fmt::Write as _;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use aws_lc_rs::hmac;
use axum::Router;
use axum::body::Bytes;
use axum::http::{HeaderMap, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use fvoci_server::config::mail::{MailSettings, MailTransport};
use fvoci_server::config::smtp::SmtpSettings;
use mail_parser::MessageParser;
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use tokio::net::TcpListener;
use tokio::sync::{Mutex, Notify};
use tokio::task::JoinHandle;

/// The providers, by the path their sending request goes to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Provider {
    Cloudflare,
    Ses,
}

/// The account of the fake Cloudflare.
pub(crate) const CLOUDFLARE_ACCOUNT: &str = "0123456789abcdef0123456789abcdef";
/// Its API token.
pub(crate) const CLOUDFLARE_TOKEN: &str = "fake-cloudflare-token_0123456789";
/// The region of the fake SES.
pub(crate) const SES_REGION: &str = "eu-west-2";
/// The access key the fake SES takes.
pub(crate) const SES_ACCESS_KEY: &str = "AKIDFAKEMAILAPI";
/// Its secret.
pub(crate) const SES_SECRET: &str = "fake/ses+secret-0123456789";

/// One request the fake was sent.
#[derive(Debug, Clone)]
pub(crate) struct Recorded {
    pub(crate) provider: Provider,
    pub(crate) headers: HeaderMap,
    /// The body as JSON.
    pub(crate) body: Value,
    /// The one recipient.
    pub(crate) to: String,
    /// The plain text of the message (SES's decoded from its MIME).
    pub(crate) text: String,
    /// The MIME text of the message, for SES.
    pub(crate) mime: String,
    /// Whether the request carried the provider's token or signature, as it checks it.
    pub(crate) signed: bool,
    /// The status it was answered with.
    pub(crate) status: u16,
}

/// An answer the test queued.
#[derive(Debug, Clone, Copy)]
struct Scripted {
    status: u16,
    /// `Retry-After`, in seconds.
    retry_after: Option<u64>,
    /// SES's name of the error (`X-Amzn-ErrorType`), instead of the one of the status.
    name: Option<&'static str>,
}

#[derive(Default)]
struct State {
    recorded: Mutex<Vec<Recorded>>,
    /// The next answers; an empty queue answers as taken.
    script: Mutex<VecDeque<Scripted>>,
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
                ..MailSettings::default()
            },
            Provider::Ses => MailSettings {
                mail_transport: Some(MailTransport::Ses),
                mail_api_endpoint: self.endpoint(),
                ses_region: SES_REGION.to_owned(),
                ses_access_key_id: SES_ACCESS_KEY.to_owned(),
                ses_secret_access_key: SES_SECRET.to_owned(),
                ..MailSettings::default()
            },
        };
        (smtp, mail)
    }

    /// Answers the next requests with these statuses, in order, then as taken; a `429`
    /// says `Retry-After: 0`.
    pub(crate) async fn answer_next(&self, statuses: &[u16]) {
        self.state
            .script
            .lock()
            .await
            .extend(statuses.iter().map(|&status| Scripted {
                status,
                retry_after: (status == 429).then_some(0),
                name: None,
            }));
    }

    /// Answers the next request with `status` and `Retry-After: seconds`.
    pub(crate) async fn answer_next_after(&self, status: u16, seconds: u64) {
        self.state.script.lock().await.push_back(Scripted {
            status,
            retry_after: Some(seconds),
            name: None,
        });
    }

    /// Answers the next request with `status` and SES's error `name`.
    pub(crate) async fn answer_next_named(&self, status: u16, name: &'static str) {
        self.state.script.lock().await.push_back(Scripted {
            status,
            retry_after: None,
            name: Some(name),
        });
    }

    pub(crate) async fn recorded(&self) -> Vec<Recorded> {
        self.state.recorded.lock().await.clone()
    }

    /// Waits until `count` requests were answered as taken; returns those.
    pub(crate) async fn taken(&self, count: usize) -> Vec<Recorded> {
        self.until(|recorded| {
            let taken: Vec<Recorded> = recorded
                .iter()
                .filter(|recorded| (200..300).contains(&recorded.status))
                .cloned()
                .collect();
            (taken.len() >= count).then_some(taken)
        })
        .await
    }

    /// Waits until `count` requests arrived, whatever they were answered.
    pub(crate) async fn requests(&self, count: usize) -> Vec<Recorded> {
        self.until(|recorded| (recorded.len() >= count).then(|| recorded.to_vec()))
            .await
    }

    async fn until<T>(&self, done: impl Fn(&[Recorded]) -> Option<T>) -> T {
        tokio::time::timeout(Duration::from_secs(20), async {
            loop {
                let changed = self.state.changed.notified();
                if let Some(done) = done(&self.recorded().await) {
                    return done;
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
    } else if path == "/v2/email/outbound-emails" {
        Provider::Ses
    } else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let json: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let (to, text, mime, signed) = match provider {
        Provider::Cloudflare => (
            json["to"].as_str().unwrap_or_default().to_owned(),
            json["text"].as_str().unwrap_or_default().to_owned(),
            String::new(),
            header(&headers, "authorization") == format!("Bearer {CLOUDFLARE_TOKEN}"),
        ),
        Provider::Ses => {
            let mime = json["Content"]["Raw"]["Data"]
                .as_str()
                .and_then(|data| STANDARD.decode(data).ok())
                .and_then(|bytes| String::from_utf8(bytes).ok())
                .unwrap_or_default();
            let text = MessageParser::default()
                .parse(mime.as_bytes())
                .and_then(|parsed| parsed.body_text(0).map(std::borrow::Cow::into_owned))
                .unwrap_or_default();
            (
                json["Destination"]["ToAddresses"][0]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned(),
                text,
                mime,
                ses_signed(&headers, body),
            )
        }
    };
    let Scripted {
        status,
        retry_after,
        name,
    } = state.script.lock().await.pop_front().unwrap_or(Scripted {
        status: 200,
        retry_after: None,
        name: None,
    });
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
        (Provider::Ses, 200) => (
            StatusCode::OK,
            axum::Json(json!({"MessageId": "fake-message-id"})),
        )
            .into_response(),
        (Provider::Ses, status) => ses_error(status, name),
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
        mime,
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

/// SES's error answer of `status`: the error's name in `X-Amzn-ErrorType`, as its API
/// v2 gives it, and a message in the body.
fn ses_error(status: u16, name: Option<&'static str>) -> Response {
    let name = name.unwrap_or(match status {
        400 => "BadRequestException",
        403 => "SignatureDoesNotMatch",
        404 => "NotFoundException",
        429 => "TooManyRequestsException",
        _ => "InternalFailure",
    });
    let mut response = (
        StatusCode::from_u16(status).expect("a status"),
        axum::Json(json!({"message": "fake"})),
    )
        .into_response();
    response.headers_mut().insert(
        "x-amzn-errortype",
        format!("{name}:http://internal.amazon.com/coral/com.amazonaws.sesv2/")
            .parse()
            .expect("a header value"),
    );
    response
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut text, byte| {
        let _ = write!(text, "{byte:02x}");
        text
    })
}

/// Whether the request carries a Signature Version 4 that SES would take: made with
/// [`SES_SECRET`] for the fake's region and the service `ses`, over the content type,
/// the host and the date, and over the body as it arrived. Written apart from the
/// server's signer, after AWS's description of `SigV4`.
fn ses_signed(headers: &HeaderMap, body: &[u8]) -> bool {
    let authorization = header(headers, "authorization");
    let amz_date = header(headers, "x-amz-date");
    let Some(day) = amz_date.get(..8) else {
        return false;
    };
    let Some((credential, rest)) = authorization
        .strip_prefix("AWS4-HMAC-SHA256 Credential=")
        .and_then(|rest| rest.split_once(", SignedHeaders="))
    else {
        return false;
    };
    let Some((signed, signature)) = rest.split_once(", Signature=") else {
        return false;
    };
    let scope = format!("{day}/{SES_REGION}/ses/aws4_request");
    if credential != format!("{SES_ACCESS_KEY}/{scope}") || signed != "content-type;host;x-amz-date"
    {
        return false;
    }
    let canonical = format!(
        "POST\n/v2/email/outbound-emails\n\ncontent-type:{}\nhost:{}\nx-amz-date:{amz_date}\n\n\
         {signed}\n{}",
        header(headers, "content-type"),
        header(headers, "host"),
        hex(&Sha256::digest(body))
    );
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{amz_date}\n{scope}\n{}",
        hex(&Sha256::digest(canonical.as_bytes()))
    );
    let sign = |key: &[u8], data: &str| {
        hmac::sign(&hmac::Key::new(hmac::HMAC_SHA256, key), data.as_bytes())
    };
    let key = [SES_REGION, "ses", "aws4_request"].iter().fold(
        sign(format!("AWS4{SES_SECRET}").as_bytes(), day),
        |key, part| sign(key.as_ref(), part),
    );
    hex(sign(key.as_ref(), &string_to_sign).as_ref()) == signature
}
