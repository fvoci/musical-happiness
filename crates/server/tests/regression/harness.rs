//! Drives a test server through a request sequence and records what it answers.
//!
//! Every recorded request becomes a [`Step`]: a name, the status code and the response
//! body after normalization. Normalization replaces what legitimately differs between two
//! runs, so that a recording can be compared with the one pinned in `golden/`:
//!
//! * identifiers (UUIDs in canonical text) become `<id:N>`, numbered in order of first
//!   appearance, also inside a longer string such as an error message or a path;
//! * secrets (43 characters of URL-safe base64: stored file names, tokens) become
//!   `<secret:N>`, numbered the same way, so that a file name is recognized where it
//!   comes up again;
//! * millisecond timestamps become `<ts>`;
//! * the seconds to wait of a 429 (`retry_after`, `Retry-After`) become `<seconds>`;
//! * values of members named like secrets (`token`, `signupToken`) become `<secret>`, and
//!   the identifier of the request in an error body becomes `<request-id>`.
//!
//! Lists are recorded in the order they come in: every list the API answers with has a
//! defined order, and the recording pins it.

use std::collections::{BTreeMap, HashMap};
use std::future::{Future, IntoFuture};
use std::pin::Pin;
use std::time::Duration;

use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::common::{ROOT_TEAM_ID, Setup, TestServer};
use crate::golden;

/// The prefix of every API route.
pub(crate) const API: &str = "/api/v1";

/// The password of every user the scenarios register.
const PASSWORD: &str = "test-password";

/// Members whose values are secrets and differ from run to run.
const SECRET_KEYS: [&str; 3] = ["token", "signupToken", "signup_token"];

/// The member of an error body that identifies the request; different in every run.
const REQUEST_ID_KEY: &str = "request_id";

/// The member of an error body, and the response header, that say how many seconds to
/// wait: a clock's reading, a second more or less from run to run.
const RETRY_AFTER_KEY: &str = "retry_after";
const RETRY_AFTER_HEADER: &str = "Retry-After";

/// Anything after 2017 in milliseconds; smaller numbers are counts, sort orders and so on.
const TIMESTAMP_FLOOR: i64 = 1_500_000_000_000;

/// The length of a secret: 256 bits in unpadded URL-safe base64.
const SECRET_LENGTH: usize = fvoci_server::auth::secret::SECRET_LENGTH;

/// One recorded request, or an observation derived from one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct Step {
    /// What the step is called in reports and in the golden file.
    pub(crate) name: String,
    /// Method, normalized path and caller; `(observation)` for a derived fact.
    pub(crate) request: String,
    /// The status code; 0 for an observation.
    pub(crate) status: u16,
    /// The media type of the response, without parameters.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub(crate) content_type: String,
    /// The response headers the scenario asked to keep.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) headers: BTreeMap<String, String>,
    /// The normalized response body.
    pub(crate) body: Value,
}

/// The body of a response, read the way its media type says.
#[derive(Debug, Clone)]
pub(crate) enum Body {
    /// A JSON media type whose content parsed.
    Json(Value),
    /// A `text/*` media type, or JSON that did not parse.
    Text(String),
    /// Anything else.
    Bytes(Vec<u8>),
}

/// What a request returned, before normalization.
#[derive(Debug, Clone)]
pub(crate) struct Reply {
    pub(crate) status: u16,
    pub(crate) body: Body,
    /// Header names are spelled one way (`Content-Type`).
    pub(crate) headers: BTreeMap<String, String>,
}

impl Reply {
    /// The parsed JSON body.
    #[track_caller]
    pub(crate) fn json(&self) -> &Value {
        match &self.body {
            Body::Json(value) => value,
            other => panic!(
                "expected a JSON body, got {other:?} (status {})",
                self.status
            ),
        }
    }

    /// The raw body of a response that is neither JSON nor text.
    #[track_caller]
    pub(crate) fn bytes(&self) -> &[u8] {
        match &self.body {
            Body::Bytes(bytes) => bytes,
            other => panic!(
                "expected a binary body, got {other:?} (status {})",
                self.status
            ),
        }
    }

    /// A string member of the JSON body.
    #[track_caller]
    pub(crate) fn string(&self, member: &str) -> String {
        string_member(self.json(), member)
    }

    /// The value of a header, or the empty string.
    pub(crate) fn header(&self, name: &str) -> String {
        self.headers.get(name).cloned().unwrap_or_default()
    }

    /// The session token the answer sets in the session cookie.
    #[track_caller]
    pub(crate) fn session_token(&self) -> String {
        let cookie = self.header("Set-Cookie");
        cookie
            .strip_prefix("fvoci_session=")
            .and_then(|rest| rest.split(';').next())
            .filter(|token| !token.is_empty())
            .unwrap_or_else(|| panic!("no session cookie in {cookie:?}"))
            .to_owned()
    }
}

/// A string member of a JSON object.
#[track_caller]
pub(crate) fn string_member(value: &Value, member: &str) -> String {
    value[member]
        .as_str()
        .unwrap_or_else(|| panic!("no string member {member:?} in {value}"))
        .to_owned()
}

/// The identifier of the object with the given title in a JSON list.
#[track_caller]
pub(crate) fn id_by_title(list: &Value, title: &str) -> String {
    let items = list.as_array().expect("a JSON list");
    let item = items
        .iter()
        .find(|item| item["title"] == title)
        .unwrap_or_else(|| panic!("nothing titled {title:?} in {list}"));
    string_member(item, "id")
}

/// What a request carries.
#[derive(Debug, Clone)]
pub(crate) enum Payload {
    /// No body.
    None,
    /// A JSON body.
    Json(Value),
    /// A body sent as given.
    Raw {
        bytes: Vec<u8>,
        content_type: Option<String>,
    },
}

/// Who a request is sent as.
#[derive(Debug, Clone)]
pub(crate) enum Caller {
    /// No session.
    Anonymous,
    /// This session token in the `Authorization` header, as a client that is no browser
    /// sends it.
    Token(String),
    /// This session token in the session cookie, as a browser sends it.
    Cookie(String),
}

/// A `multipart/form-data` body with one file part.
pub(crate) fn multipart(
    field_name: &str,
    file_name: &str,
    content: &[u8],
    content_type: &str,
) -> Payload {
    const BOUNDARY: &str = "fvoci-regression-7f3a9c51e2b84d06";
    let mut bytes = format!(
        "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{field_name}\"; \
         filename=\"{file_name}\"\r\nContent-Type: {content_type}\r\n\r\n"
    )
    .into_bytes();
    bytes.extend_from_slice(content);
    bytes.extend_from_slice(format!("\r\n--{BOUNDARY}--\r\n").as_bytes());
    Payload::Raw {
        bytes,
        content_type: Some(format!("multipart/form-data; boundary={BOUNDARY}")),
    }
}

/// The media type [`multipart`] gives a file part unless told otherwise.
pub(crate) const OCTET_STREAM: &str = "application/octet-stream";

/// An HTTP client for one test server. Sends requests; records nothing.
#[derive(Debug, Clone)]
pub(crate) struct Http {
    client: reqwest::Client,
    base_url: String,
}

impl Http {
    pub(crate) fn new(base_url: &str) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            // A redirect is an answer to record, not something to follow.
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("an HTTP client can be built");
        Self {
            client,
            base_url: base_url.trim_end_matches('/').to_owned(),
        }
    }

    /// Sends a request. `path` is relative to the server root, e.g. `/api/v1/boards`.
    /// `csrf` adds the header of the API's request-forgery check.
    pub(crate) async fn send(
        &self,
        method: Method,
        path: &str,
        payload: Payload,
        caller: &Caller,
        csrf: Csrf,
        extra_headers: &[(&str, String)],
    ) -> Reply {
        let mut request = self
            .client
            .request(method, format!("{}{path}", self.base_url));
        if csrf == Csrf::Header {
            request = request.header("X-Requested-With", "XMLHttpRequest");
        }
        request = match caller {
            Caller::Anonymous => request,
            Caller::Token(token) => request.bearer_auth(token),
            Caller::Cookie(token) => request.header("Cookie", format!("fvoci_session={token}")),
        };
        request = match payload {
            Payload::None => request,
            Payload::Json(value) => request.json(&value),
            Payload::Raw {
                bytes,
                content_type,
            } => request
                .header(
                    "Content-Type",
                    // What an HTTP client that was told nothing sends for a body.
                    content_type.unwrap_or_else(|| "application/x-www-form-urlencoded".to_owned()),
                )
                .body(bytes),
        };
        for (name, value) in extra_headers {
            request = request.header(*name, value);
        }

        let response = request.send().await.expect("the server answers");
        let status = response.status().as_u16();
        let headers: BTreeMap<String, String> = response
            .headers()
            .iter()
            .map(|(name, value)| {
                (
                    canonical_header_name(name.as_str()),
                    String::from_utf8_lossy(value.as_bytes()).into_owned(),
                )
            })
            .collect();
        let raw = response
            .bytes()
            .await
            .expect("the answer has a body")
            .to_vec();

        let media_type = headers.get("Content-Type").cloned().unwrap_or_default();
        let body = if media_type.contains("json") {
            match serde_json::from_slice(&raw) {
                Ok(value) => Body::Json(value),
                Err(_) => Body::Text(String::from_utf8_lossy(&raw).into_owned()),
            }
        } else if media_type.starts_with("text/") {
            Body::Text(String::from_utf8_lossy(&raw).into_owned())
        } else {
            Body::Bytes(raw)
        };
        Reply {
            status,
            body,
            headers,
        }
    }

    /// A JSON request with the API header, as the given caller.
    pub(crate) async fn api(
        &self,
        method: Method,
        path: &str,
        payload: Payload,
        caller: &Caller,
    ) -> Reply {
        self.send(method, path, payload, caller, Csrf::Header, &[])
            .await
    }

    /// Registers `user` and signs in. Returns the session token.
    pub(crate) async fn register(&self, user: &str, signup_token: &str) -> String {
        let mut body = json!({
            "username": user,
            "email": format!("{user}@example.com"),
            "password": PASSWORD,
        });
        if !signup_token.is_empty() {
            body["token"] = json!(signup_token);
        }
        let reply = self
            .api(
                Method::POST,
                &format!("{API}/register"),
                Payload::Json(body),
                &Caller::Anonymous,
            )
            .await;
        assert_eq!(reply.status, 200, "register {user}: {:?}", reply.body);
        self.login(user).await
    }

    /// Signs `user` in and returns the session token.
    pub(crate) async fn login(&self, user: &str) -> String {
        let body = json!({"username": user, "password": PASSWORD});
        let reply = self
            .api(
                Method::POST,
                &format!("{API}/login"),
                Payload::Json(body),
                &Caller::Anonymous,
            )
            .await;
        assert_eq!(reply.status, 200, "login {user}: {:?}", reply.body);
        reply.session_token()
    }

    /// The team's signup token, read as the holder of `token`.
    pub(crate) async fn signup_token(&self, token: &str) -> String {
        self.api(
            Method::GET,
            &format!("{API}/teams/{ROOT_TEAM_ID}"),
            Payload::None,
            &Caller::Token(token.to_owned()),
        )
        .await
        .string("signupToken")
    }

    /// The identifier of the user who holds `token`.
    pub(crate) async fn user_id(&self, token: &str) -> String {
        self.api(
            Method::GET,
            &format!("{API}/users/me"),
            Payload::None,
            &Caller::Token(token.to_owned()),
        )
        .await
        .string("id")
    }
}

/// Whether a request carries the header the API's cross-site check asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Csrf {
    /// `X-Requested-With: XMLHttpRequest`.
    Header,
    /// No such header.
    Missing,
}

/// How a request presents its session token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Carrier {
    /// In the `Authorization` header.
    Bearer,
    /// In the session cookie.
    Cookie,
}

/// Whether a request becomes a step of the recording.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Recording {
    Step,
    Skip,
}

/// Drives one server and records what it answers.
#[derive(Debug)]
pub(crate) struct Session {
    http: Http,
    steps: Vec<Step>,
    /// Session tokens by user name.
    tokens: HashMap<String, String>,
    placeholders: Placeholders,
}

impl Session {
    fn new(base_url: &str) -> Self {
        Self {
            http: Http::new(base_url),
            steps: Vec::new(),
            tokens: HashMap::new(),
            placeholders: Placeholders::default(),
        }
    }

    pub(crate) fn get(&mut self, name: &str, path: &str) -> Call<'_> {
        self.call(name, Method::GET, path)
    }

    pub(crate) fn post(&mut self, name: &str, path: &str) -> Call<'_> {
        self.call(name, Method::POST, path)
    }

    pub(crate) fn put(&mut self, name: &str, path: &str) -> Call<'_> {
        self.call(name, Method::PUT, path)
    }

    pub(crate) fn patch(&mut self, name: &str, path: &str) -> Call<'_> {
        self.call(name, Method::PATCH, path)
    }

    pub(crate) fn delete(&mut self, name: &str, path: &str) -> Call<'_> {
        self.call(name, Method::DELETE, path)
    }

    /// A request named `name`; awaiting it sends it and records the answer.
    pub(crate) fn call(&mut self, name: &str, method: Method, path: &str) -> Call<'_> {
        Call {
            session: self,
            name: name.to_owned(),
            method,
            path: path.to_owned(),
            payload: Payload::None,
            user: None,
            token: None,
            csrf: Csrf::Header,
            carrier: Carrier::Bearer,
            recording: Recording::Step,
            keep_headers: &[],
            extra_headers: Vec::new(),
        }
    }

    /// Records a derived fact (not a request) as a step.
    pub(crate) fn note(&mut self, name: &str, value: &Value) {
        let body = self.placeholders.value(value, "");
        self.steps.push(Step {
            name: name.to_owned(),
            request: "(observation)".to_owned(),
            status: 0,
            content_type: String::new(),
            headers: BTreeMap::new(),
            body,
        });
    }

    /// Registers one user per name, unique to the scenario, with the team's signup
    /// token, and returns the user names.
    pub(crate) async fn users<const N: usize>(
        &mut self,
        scenario: &str,
        names: [&str; N],
    ) -> [String; N] {
        let signup_token = self.signup_token("root").await;
        let mut created = Vec::with_capacity(N);
        for name in names {
            let user = format!("{scenario}-{name}");
            let token = self.http.register(&user, &signup_token).await;
            self.tokens.insert(user.clone(), token);
            created.push(user);
        }
        created
            .try_into()
            .unwrap_or_else(|_| unreachable!("one user is created per name"))
    }

    /// The team's signup token, read as `user`. Not recorded.
    pub(crate) async fn signup_token(&self, user: &str) -> String {
        self.http.signup_token(self.token_of(user)).await
    }

    /// The identifier of `user`. Not recorded.
    pub(crate) async fn user_id(&self, user: &str) -> String {
        self.http.user_id(self.token_of(user)).await
    }

    #[track_caller]
    /// The session token of a registered user. Not recorded.
    pub(crate) fn session_token(&self, user: &str) -> String {
        self.token_of(user).to_owned()
    }

    /// Takes `token` for the session of `user` from now on: for when the server gave
    /// the session a new one.
    pub(crate) fn set_session_token(&mut self, user: &str, token: String) {
        self.tokens.insert(user.to_owned(), token);
    }

    fn token_of(&self, user: &str) -> &str {
        self.tokens
            .get(user)
            .unwrap_or_else(|| panic!("no session for user {user:?}"))
    }
}

/// A request being put together; see [`Session::call`].
#[derive(Debug)]
pub(crate) struct Call<'s> {
    session: &'s mut Session,
    name: String,
    method: Method,
    path: String,
    payload: Payload,
    user: Option<String>,
    token: Option<String>,
    csrf: Csrf,
    carrier: Carrier,
    recording: Recording,
    keep_headers: &'static [&'static str],
    extra_headers: Vec<(&'static str, String)>,
}

impl Call<'_> {
    /// Sends this JSON body.
    pub(crate) fn json(mut self, body: Value) -> Self {
        self.payload = Payload::Json(body);
        self
    }

    /// Sends these bytes as the body, as an HTTP client does that was given no media
    /// type. For malformed input.
    pub(crate) fn raw(mut self, bytes: &[u8]) -> Self {
        self.payload = Payload::Raw {
            bytes: bytes.to_vec(),
            content_type: None,
        };
        self
    }

    /// Sends a prepared payload, such as [`multipart`]'s.
    pub(crate) fn payload(mut self, payload: Payload) -> Self {
        self.payload = payload;
        self
    }

    /// Sends the request with the session of a registered user.
    pub(crate) fn user(mut self, user: &str) -> Self {
        self.user = Some(user.to_owned());
        self
    }

    /// Sends the request with this bearer token.
    pub(crate) fn token(mut self, token: &str) -> Self {
        self.token = Some(token.to_owned());
        self
    }

    /// Sends the session token in the session cookie, as a browser does, instead of the
    /// `Authorization` header.
    pub(crate) fn in_cookie(mut self) -> Self {
        self.carrier = Carrier::Cookie;
        self
    }

    /// Leaves out the header of the API's cross-site check.
    pub(crate) fn without_csrf_header(mut self) -> Self {
        self.csrf = Csrf::Missing;
        self
    }

    /// Sends the request without making it a step of the recording.
    pub(crate) fn unrecorded(mut self) -> Self {
        self.recording = Recording::Skip;
        self
    }

    /// Records these response headers with the step.
    pub(crate) fn keep_headers(mut self, names: &'static [&'static str]) -> Self {
        self.keep_headers = names;
        self
    }

    /// Adds a request header.
    pub(crate) fn header(mut self, name: &'static str, value: &str) -> Self {
        self.extra_headers.push((name, value.to_owned()));
        self
    }

    async fn send(self) -> Reply {
        let session = self.session;
        let token = match &self.user {
            Some(user) => Some(session.token_of(user).to_owned()),
            None => self.token,
        };
        let who = match (&self.user, &token) {
            (Some(user), _) => user.as_str(),
            (None, Some(_)) => "token",
            (None, None) => "anonymous",
        };
        let who = match self.carrier {
            Carrier::Bearer => who.to_owned(),
            Carrier::Cookie => format!("{who} (cookie)"),
        };
        let caller = match (token.clone(), self.carrier) {
            (None, _) => Caller::Anonymous,
            (Some(token), Carrier::Bearer) => Caller::Token(token),
            (Some(token), Carrier::Cookie) => Caller::Cookie(token),
        };
        let reply = session
            .http
            .send(
                self.method.clone(),
                &self.path,
                self.payload,
                &caller,
                self.csrf,
                &self.extra_headers,
            )
            .await;

        if self.recording == Recording::Step {
            // The order matters: placeholders are numbered by first appearance, in the
            // path first, then in the body, then in the kept headers.
            let path = session.placeholders.text(&self.path);
            let media_type = reply.header("Content-Type");
            let body = session.placeholders.body(&reply.body, &media_type);
            let headers = self
                .keep_headers
                .iter()
                .map(|name| {
                    let value = reply.header(name);
                    let kept = if *name == RETRY_AFTER_HEADER && !value.is_empty() {
                        "<seconds>".to_owned()
                    } else {
                        session.placeholders.text(&value)
                    };
                    ((*name).to_owned(), kept)
                })
                .collect();
            session.steps.push(Step {
                name: self.name,
                request: format!("{} {path} as {who}", self.method),
                status: reply.status,
                content_type: media_type.split(';').next().unwrap_or_default().to_owned(),
                headers,
                body,
            });
        }
        reply
    }
}

impl<'s> IntoFuture for Call<'s> {
    type Output = Reply;
    type IntoFuture = Pin<Box<dyn Future<Output = Reply> + 's>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(self.send())
    }
}

/// Starts a server with the built-in templates and public sharing, registers the first
/// user `root`, runs the scenario and compares its recording with `golden/<name>.json`.
pub(crate) async fn record(name: &str, scenario: impl AsyncFnOnce(&mut Session)) {
    let server = start_server().await;
    let mut session = Session::new(&server.base_url);
    let root = session.http.register("root", "").await;
    session.tokens.insert("root".to_owned(), root);
    scenario(&mut session).await;
    server.teardown().await;
    golden::check_steps(name, &session.steps);
}

/// The server every regression recording runs against.
pub(crate) async fn start_server() -> TestServer {
    Setup::local_mode().with_templates().start().await
}

/// Hands out the numbered placeholders of one scenario.
#[derive(Debug, Default)]
struct Placeholders {
    ids: HashMap<String, usize>,
    secrets: HashMap<String, usize>,
}

impl Placeholders {
    fn text(&mut self, text: &str) -> String {
        replace_run_specifics(text, |found| match found {
            Found::Id(identifier) => {
                let next = self.ids.len() + 1;
                let number = *self.ids.entry(identifier.to_owned()).or_insert(next);
                format!("<id:{number}>")
            }
            Found::Secret(secret) => {
                let next = self.secrets.len() + 1;
                let number = *self.secrets.entry(secret.to_owned()).or_insert(next);
                format!("<secret:{number}>")
            }
        })
    }

    fn body(&mut self, body: &Body, media_type: &str) -> Value {
        match body {
            Body::Bytes(bytes) => json!(format!("<{} bytes>", bytes.len())),
            Body::Text(text) if media_type.contains("html") => {
                if text.to_lowercase().contains("<html") {
                    json!("<html>")
                } else {
                    json!(self.text(text))
                }
            }
            Body::Text(text) => json!(self.text(text)),
            Body::Json(value) => self.value(value, ""),
        }
    }

    /// `key` is the name of the member the value sits under, also for the items of a
    /// list under that member.
    fn value(&mut self, value: &Value, key: &str) -> Value {
        match value {
            Value::Object(members) => {
                let mut sorted: Vec<(&String, &Value)> = members.iter().collect();
                sorted.sort_by(|a, b| a.0.cmp(b.0));
                let mut normalized = Map::new();
                for (name, member) in sorted {
                    normalized.insert(name.clone(), self.value(member, name));
                }
                Value::Object(normalized)
            }
            Value::Array(items) => {
                Value::Array(items.iter().map(|item| self.value(item, key)).collect())
            }
            Value::String(text) => {
                if SECRET_KEYS.contains(&key) && !text.is_empty() {
                    json!("<secret>")
                } else if key == REQUEST_ID_KEY {
                    json!("<request-id>")
                } else {
                    json!(self.text(text))
                }
            }
            Value::Number(number) if is_timestamp(number) => json!("<ts>"),
            Value::Number(_) if key == RETRY_AFTER_KEY => json!("<seconds>"),
            other => other.clone(),
        }
    }
}

fn is_timestamp(number: &serde_json::Number) -> bool {
    // An integer too large for `i64` is past the floor as well; a float never counts.
    number
        .as_i64()
        .map_or_else(|| number.is_u64(), |n| n >= TIMESTAMP_FLOOR)
}

/// What differs from run to run inside a text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Found<'a> {
    /// An identifier: a UUID in canonical text.
    Id(&'a str),
    /// A secret: [`SECRET_LENGTH`] characters of URL-safe base64.
    Secret(&'a str),
}

/// Replaces every identifier and every secret in `text`.
///
/// Both are looked for as whole runs of the characters either can have (letters, digits,
/// `-` and `_`): a run that is an identifier, or one of the length of a secret that has
/// both upper-case and lower-case letters. A word of that length in plain text has no
/// reason to mix the two; 256 random bits in base64 all but always do.
fn replace_run_specifics(text: &str, mut replacement: impl FnMut(Found<'_>) -> String) -> String {
    fn is_run_character(character: char) -> bool {
        character.is_ascii_alphanumeric() || character == '-' || character == '_'
    }

    fn is_secret(run: &str) -> bool {
        run.len() == SECRET_LENGTH
            && run.chars().any(|character| character.is_ascii_uppercase())
            && run.chars().any(|character| character.is_ascii_lowercase())
    }

    let mut result = String::with_capacity(text.len());
    let mut rest = text;
    while !rest.is_empty() {
        let run_length = rest
            .find(|character| !is_run_character(character))
            .unwrap_or(rest.len());
        if run_length == 0 {
            let other_length = rest.find(is_run_character).unwrap_or(rest.len());
            result.push_str(&rest[..other_length]);
            rest = &rest[other_length..];
            continue;
        }
        let run = &rest[..run_length];
        if fvoci_model::is_id(run) {
            result.push_str(&replacement(Found::Id(run)));
        } else if is_secret(run) {
            result.push_str(&replacement(Found::Secret(run)));
        } else {
            result.push_str(run);
        }
        rest = &rest[run_length..];
    }
    result
}

/// Header names are case-insensitive; spell them one way (`Content-Type`).
fn canonical_header_name(name: &str) -> String {
    name.split('-')
        .map(|part| {
            let mut characters = part.chars();
            characters.next().map_or_else(String::new, |first| {
                format!(
                    "{}{}",
                    first.to_ascii_uppercase(),
                    characters.as_str().to_ascii_lowercase()
                )
            })
        })
        .collect::<Vec<_>>()
        .join("-")
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIRST: &str = "01890a5d-ac96-774b-bcce-b302099a8057";
    const SECOND: &str = "01890a5d-ac96-774b-bcce-b302099a8058";
    const SECRET: &str = "qM4nYvE7m0kXc1D_8sWb-3uZtLrA9hGfJ2oPiK5xN6Q";

    #[test]
    fn identifiers_are_numbered_in_order_of_first_appearance() {
        let mut placeholders = Placeholders::default();
        let text = format!("{FIRST} not found, see {SECOND} and {FIRST}");
        assert_eq!(
            placeholders.text(&text),
            "<id:1> not found, see <id:2> and <id:1>",
            "input {text:?}"
        );
    }

    #[test]
    fn secrets_are_numbered_apart_from_identifiers() {
        assert_eq!(SECRET.len(), SECRET_LENGTH, "the sample is a secret");
        let mut placeholders = Placeholders::default();
        let text = format!("/files/teams/{FIRST}/{SECOND}/{SECRET}.png and {SECRET}.png");
        assert_eq!(
            placeholders.text(&text),
            "/files/teams/<id:1>/<id:2>/<secret:1>.png and <secret:1>.png",
            "input {text:?}"
        );
    }

    #[test]
    fn only_whole_runs_are_identifiers_or_secrets() {
        let unchanged = |text: &str| {
            assert_eq!(
                Placeholders::default().text(text),
                text,
                "{text:?} holds nothing that differs between runs"
            );
        };
        // Other spellings of a UUID, the nil UUID, and an identifier inside a longer run.
        unchanged("01890A5D-AC96-774B-BCCE-B302099A8057");
        unchanged("01890a5dac96774bbcceb302099a8057");
        unchanged("00000000-0000-0000-0000-000000000000");
        unchanged("x01890a5d-ac96-774b-bcce-b302099a8057");
        unchanged("01890a5d-ac96-774b-bcce-b302099a8057-more");
        // As long as a secret, but a word: one letter case only.
        unchanged("a_setting_with_a_very_long_snake_case_name_x");
        unchanged("unknown-board");
        assert_eq!(
            Placeholders::default().text(&format!("block ID={FIRST}.")),
            "block ID=<id:1>.",
            "an identifier ends at a character outside the run"
        );
    }

    #[test]
    fn lists_keep_their_order() {
        let mut placeholders = Placeholders::default();
        let value = json!([
            {"id": SECOND, "title": "b", "updateAt": 1_700_000_000_001_i64},
            {"id": FIRST, "title": "a", "token": "x"},
        ]);
        assert_eq!(
            placeholders.value(&value, ""),
            json!([
                {"id": "<id:1>", "title": "b", "updateAt": "<ts>"},
                {"id": "<id:2>", "title": "a", "token": "<secret>"},
            ]),
            "as they came, numbered in that order"
        );
    }

    #[test]
    fn header_names_are_spelled_one_way() {
        assert_eq!(
            canonical_header_name("x-content-type-options"),
            "X-Content-Type-Options",
            "each part is capitalized"
        );
        assert_eq!(canonical_header_name("ETag"), "Etag", "the rest is lowered");
    }
}
