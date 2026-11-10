//! The test server and the client that talks to it; the counterpart of the original's
//! `server/integrationtests/clienttestlib.go` and `server/client/client.go`.
#![allow(
    dead_code,
    reason = "a shared helper module; each test module uses a different part of it"
)]

use std::path::{Path, PathBuf};
use std::time::Duration;

use fvoci_model::{
    Block, Board, BoardMember, BoardType, IdKind, LoginRequest, LoginResponse, RegisterRequest,
    Team, User, new_id,
};
use fvoci_server::app::App;
use fvoci_server::config::Configuration;
use fvoci_server::{Server, TlsFiles};
use fvoci_store::testing::TestDatabase;
use reqwest::{Method, StatusCode};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use tokio::sync::oneshot;

pub(crate) const USER1_USERNAME: &str = "user1";
pub(crate) const USER2_USERNAME: &str = "user2";
pub(crate) const PASSWORD: &str = "Pa$$word";
/// The team the original's tests use. A standalone server accepts any team identifier.
pub(crate) const TEST_TEAM_ID: &str = "team-id";
/// The single-user token of `SetupTestHelperWithToken`.
pub(crate) const SINGLE_USER_TOKEN: &str = "TESTTOKEN";

/// A running server with a database and directories of its own.
pub(crate) struct TestServer {
    /// The application state, for arranging and inspecting what the API has no route for.
    pub(crate) app: App,
    pub(crate) base_url: String,
    /// The first client; `user1` after [`TestServer::init_basic`].
    pub(crate) client: Client,
    /// The second client; `user2` after [`TestServer::init_basic`].
    pub(crate) client2: Client,
    shutdown: oneshot::Sender<()>,
    serving: tokio::task::JoinHandle<anyhow::Result<()>>,
    database: TestDatabase,
    directory: PathBuf,
}

/// How a test server differs from the default one.
#[derive(Debug, Default)]
pub(crate) struct Setup {
    single_user_token: Option<String>,
    public_shared_boards: bool,
    base_path: String,
    configure: Vec<fn(&mut Configuration)>,
    admin_socket: Option<PathBuf>,
    tls_files: Option<TlsFiles>,
    /// Whether the built-in templates are imported at startup. Off by default so a
    /// server starts quickly; the counterpart of the original's `SkipTemplateInit`.
    with_templates: bool,
}

impl Setup {
    /// `SetupTestHelper`: native authentication, no users yet.
    pub(crate) fn native() -> Self {
        Self::default()
    }

    /// `SetupTestHelperWithToken`: single-user mode; both clients carry the token.
    pub(crate) fn single_user() -> Self {
        Self {
            single_user_token: Some(SINGLE_USER_TOKEN.to_owned()),
            ..Self::default()
        }
    }

    /// Single-user mode with a token of the caller's choosing.
    pub(crate) fn single_user_with_token(token: &str) -> Self {
        Self {
            single_user_token: Some(token.to_owned()),
            ..Self::default()
        }
    }

    /// `SetupTestHelperLocalMode`: native authentication with public sharing turned on.
    pub(crate) fn local_mode() -> Self {
        Self {
            public_shared_boards: true,
            ..Self::default()
        }
    }

    /// Imports the built-in templates at startup, for tests that need them.
    pub(crate) fn with_templates(mut self) -> Self {
        self.with_templates = true;
        self
    }

    /// Serves everything under a path prefix such as `/boards`.
    pub(crate) fn base_path(mut self, base_path: &str) -> Self {
        base_path.clone_into(&mut self.base_path);
        self
    }

    /// Changes the configuration before the server starts.
    pub(crate) fn configure(mut self, change: fn(&mut Configuration)) -> Self {
        self.configure.push(change);
        self
    }

    /// Opens the administration socket at `path` (`enablelocalmode`).
    pub(crate) fn admin_socket(mut self, path: &Path) -> Self {
        self.admin_socket = Some(path.to_owned());
        self
    }

    /// Reads the certificate and the key for `usessl` from these files. The server only
    /// serves TLS if the configuration turns `usessl` on as well; the clients of
    /// [`TestServer`] do not speak it.
    pub(crate) fn tls_files(mut self, tls_files: TlsFiles) -> Self {
        self.tls_files = Some(tls_files);
        self
    }

    /// Starts the server. The counterpart of `SetupTestHelper(t).Start()`.
    pub(crate) async fn start(self) -> TestServer {
        self.try_start().await.expect("the test server starts")
    }

    /// Starts the server, or returns why it would not start.
    pub(crate) async fn try_start(self) -> anyhow::Result<TestServer> {
        let directory = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
            .join("api-tests")
            .join(new_id(IdKind::None));
        let web_path = directory.join("web");
        std::fs::create_dir_all(web_path.join("static")).expect("the web directory is created");
        std::fs::write(
            web_path.join("index.html"),
            "<html><head><base href=\"{{.BaseURL}}/\"></head><body>test web app</body></html>",
        )
        .expect("the index page is written");
        std::fs::write(
            web_path.join("static").join("main.js"),
            "console.log('test');",
        )
        .expect("a static file is written");

        let database = TestDatabase::create(&directory).await;
        let mut config = Configuration {
            db_type: database.db_type().as_str().to_owned(),
            db_config: database.dbconfig().to_owned(),
            db_table_prefix: "test_".to_owned(),
            web_path: web_path.display().to_string(),
            files_driver: "local".to_owned(),
            files_path: directory.join("uploads").display().to_string(),
            enable_public_shared_boards: self.public_shared_boards,
            // Pings are tested on their own; elsewhere they would only be noise.
            websocket_ping_seconds: 0,
            // Importing the templates is slow; only tests that ask for them pay it.
            skip_template_init: !self.with_templates,
            ..Configuration::default()
        };
        if let Some(path) = &self.admin_socket {
            config.enable_local_mode = true;
            config.local_mode_socket_location = path.display().to_string();
        }
        for change in &self.configure {
            change(&mut config);
        }

        let mut server =
            Server::new(config, self.single_user_token.clone(), &self.base_path).await?;
        if let Some(tls_files) = self.tls_files {
            server = server.with_tls_files(tls_files);
        }
        let app = server.app().clone();
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("a local port is free");
        let address = listener.local_addr().expect("the listener has an address");
        let (shutdown, stopped) = oneshot::channel::<()>();
        let serving = tokio::spawn(server.serve(listener, async move {
            let _ = stopped.await;
        }));

        let base_url = format!("http://{address}{}", self.base_path);
        let token = self.single_user_token.unwrap_or_default();
        Ok(TestServer {
            app,
            client: Client::new(&base_url, &token),
            client2: Client::new(&base_url, &token),
            base_url,
            shutdown,
            serving,
            database,
            directory,
        })
    }
}

impl TestServer {
    /// `InitBasic`: registers and signs in `user1` on the first client and `user2`, with
    /// the team's signup token, on the second.
    pub(crate) async fn init_basic(mut self) -> Self {
        self.client = self
            .register_and_login(USER1_USERNAME, "user1@sample.com", PASSWORD, "")
            .await;
        let team: Team = self.client.get("/teams/0").await.ok_json();
        assert!(!team.signup_token.is_empty(), "the team has a signup token");
        self.client2 = self
            .register_and_login(
                USER2_USERNAME,
                "user2@sample.com",
                PASSWORD,
                &team.signup_token,
            )
            .await;
        self
    }

    /// A client without a session.
    pub(crate) fn anonymous(&self) -> Client {
        Client::new(&self.base_url, "")
    }

    /// A client that carries the given session token.
    pub(crate) fn client_with_token(&self, token: &str) -> Client {
        Client::new(&self.base_url, token)
    }

    /// `RegisterAndLogin`: registers a user and returns a client signed in as them.
    pub(crate) async fn register_and_login(
        &self,
        username: &str,
        email: &str,
        password: &str,
        signup_token: &str,
    ) -> Client {
        let request = RegisterRequest {
            username: username.to_owned(),
            email: email.to_owned(),
            password: password.to_owned(),
            token: signup_token.to_owned(),
        };
        self.anonymous()
            .post("/register", &request)
            .await
            .check_ok();
        self.login(username, password).await
    }

    /// `Login`: returns a client signed in as an existing user.
    pub(crate) async fn login(&self, username: &str, password: &str) -> Client {
        let mut client = self.anonymous();
        client.login(username, password).await.check_ok();
        client
    }

    /// Registers one more user with the team's signup token and signs them in.
    pub(crate) async fn add_user(&self, username: &str) -> Client {
        let team = self.app.root_team().await.expect("the root team exists");
        self.register_and_login(
            username,
            &format!("{username}@sample.com"),
            PASSWORD,
            &team.signup_token,
        )
        .await
    }

    /// `CreateBoard`: creates an empty board as the first client's user.
    pub(crate) async fn create_board(&self, team_id: &str, board_type: BoardType) -> Board {
        let board = Board {
            team_id: team_id.to_owned(),
            board_type,
            ..Board::default()
        };
        self.client.post("/boards", &board).await.ok_json()
    }

    /// Stops the server, removes its database and files, and returns how serving ended.
    /// For servers that are expected to refuse to serve.
    pub(crate) async fn stop(self) -> anyhow::Result<()> {
        let _ = self.shutdown.send(());
        let served = self.serving.await.expect("the server task does not panic");
        self.database.remove().await;
        let _ = std::fs::remove_dir_all(&self.directory);
        served
    }

    /// Stops the server and removes its database and files. The counterpart of
    /// `TearDown`.
    pub(crate) async fn teardown(self) {
        let _ = self.shutdown.send(());
        self.serving
            .await
            .expect("the server task does not panic")
            .expect("the server stops cleanly");
        self.database.remove().await;
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

/// An HTTP client for the API that carries one session token; the counterpart of the
/// original's `client.Client`.
#[derive(Debug, Clone)]
pub(crate) struct Client {
    http: reqwest::Client,
    base_url: String,
    /// The session token, sent as a bearer token when not empty.
    pub(crate) token: String,
}

/// What the server answered.
#[derive(Debug)]
pub(crate) struct ApiResponse {
    pub(crate) status: StatusCode,
    pub(crate) headers: reqwest::header::HeaderMap,
    pub(crate) body: Vec<u8>,
}

impl Client {
    pub(crate) fn new(base_url: &str, token: &str) -> Self {
        Self {
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .expect("an HTTP client can be built"),
            base_url: base_url.to_owned(),
            token: token.to_owned(),
        }
    }

    /// The address of a path outside the API, such as `/ping`.
    pub(crate) fn root_url(&self, path: &str) -> String {
        format!("{}{path}", self.base_url)
    }

    /// The websocket address of the server.
    pub(crate) fn websocket_url(&self) -> String {
        format!("{}/ws", self.base_url.replacen("http", "ws", 1))
    }

    /// A request to an API path such as `/boards`, with the headers every API request
    /// needs. For requests the shorthand methods do not cover.
    pub(crate) fn request(&self, method: Method, path: &str) -> reqwest::RequestBuilder {
        let mut request = self
            .http
            .request(method, format!("{}/api/v2{path}", self.base_url))
            .header("X-Requested-With", "XMLHttpRequest");
        if !self.token.is_empty() {
            request = request.bearer_auth(&self.token);
        }
        request
    }

    /// A request to any path of the server, with no headers added.
    pub(crate) fn raw_request(&self, method: Method, path: &str) -> reqwest::RequestBuilder {
        self.http.request(method, self.root_url(path))
    }

    /// Sends a request and reads the whole answer.
    pub(crate) async fn send(request: reqwest::RequestBuilder) -> ApiResponse {
        let response = request.send().await.expect("the server answers");
        let status = response.status();
        let headers = response.headers().clone();
        let body = response
            .bytes()
            .await
            .expect("the answer has a body")
            .to_vec();
        ApiResponse {
            status,
            headers,
            body,
        }
    }

    pub(crate) async fn get(&self, path: &str) -> ApiResponse {
        Self::send(self.request(Method::GET, path)).await
    }

    pub(crate) async fn delete(&self, path: &str) -> ApiResponse {
        Self::send(self.request(Method::DELETE, path)).await
    }

    pub(crate) async fn delete_json(&self, path: &str, body: &impl Serialize) -> ApiResponse {
        Self::send(self.request(Method::DELETE, path).json(body)).await
    }

    pub(crate) async fn post(&self, path: &str, body: &impl Serialize) -> ApiResponse {
        Self::send(self.request(Method::POST, path).json(body)).await
    }

    /// A `POST` without a body.
    pub(crate) async fn post_empty(&self, path: &str) -> ApiResponse {
        Self::send(self.request(Method::POST, path)).await
    }

    pub(crate) async fn put(&self, path: &str, body: &impl Serialize) -> ApiResponse {
        Self::send(self.request(Method::PUT, path).json(body)).await
    }

    pub(crate) async fn patch(&self, path: &str, body: &impl Serialize) -> ApiResponse {
        Self::send(self.request(Method::PATCH, path).json(body)).await
    }

    /// A request whose body is sent as given, for malformed input.
    pub(crate) async fn send_raw_body(
        &self,
        method: Method,
        path: &str,
        body: &str,
    ) -> ApiResponse {
        Self::send(self.request(method, path).body(body.to_owned())).await
    }

    /// `Login`: signs in and keeps the session token.
    pub(crate) async fn login(&mut self, username: &str, password: &str) -> ApiResponse {
        let request = LoginRequest {
            login_type: "normal".to_owned(),
            username: username.to_owned(),
            email: String::new(),
            password: password.to_owned(),
            mfa_token: String::new(),
        };
        let response = self.post("/login", &request).await;
        if response.status == StatusCode::OK {
            self.token = response.json::<LoginResponse>().token;
        }
        response
    }

    /// `GetMe`.
    pub(crate) async fn me(&self) -> User {
        self.get("/users/me").await.ok_json()
    }

    /// Creates a board with the given fields as this client's user.
    pub(crate) async fn create_board(&self, board: &Board) -> ApiResponse {
        self.post("/boards", board).await
    }

    /// `InsertBlocks`.
    pub(crate) async fn insert_blocks(&self, board_id: &str, blocks: &[Block]) -> ApiResponse {
        self.post(&format!("/boards/{board_id}/blocks"), &blocks)
            .await
    }

    /// `AddMemberToBoard`.
    pub(crate) async fn add_member(&self, member: &BoardMember) -> ApiResponse {
        self.post(&format!("/boards/{}/members", member.board_id), member)
            .await
    }
}

impl ApiResponse {
    /// The body as text.
    pub(crate) fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    /// The body decoded from JSON.
    #[track_caller]
    pub(crate) fn json<T: DeserializeOwned>(&self) -> T {
        serde_json::from_slice(&self.body).unwrap_or_else(|error| {
            panic!(
                "the body is not the expected JSON ({error}): {}",
                self.text()
            )
        })
    }

    /// The body as loosely typed JSON, for checking the exact wire format.
    #[track_caller]
    pub(crate) fn value(&self) -> Value {
        self.json()
    }

    /// The message of an error answer: English text for developers.
    #[track_caller]
    pub(crate) fn error_message(&self) -> String {
        self.error_member("message")
    }

    /// The stable code of an error answer, such as `not_found`.
    #[track_caller]
    pub(crate) fn error_code(&self) -> String {
        self.error_member("error")
    }

    /// The stable reason of an error answer, such as `last_admin`; empty if none.
    #[track_caller]
    pub(crate) fn error_reason(&self) -> String {
        self.error_member("reason")
    }

    #[track_caller]
    fn error_member(&self, member: &str) -> String {
        self.value()[member].as_str().unwrap_or_default().to_owned()
    }

    /// Checks that the answer is the error with this status and message, in the one
    /// format of the API's errors: the code that belongs to the status, the message, and
    /// the identifier of the request, which the response header repeats.
    #[track_caller]
    pub(crate) fn check_error(&self, status: StatusCode, message: &str) {
        self.check_status(status);
        let body = self.value();
        let code = match status.as_u16() {
            400 => "bad_request",
            401 => "unauthorized",
            403 => "forbidden",
            404 => "not_found",
            405 => "method_not_allowed",
            409 => "conflict",
            413 => "payload_too_large",
            500 => "internal",
            other => panic!("status {other} is no error status of the API"),
        };
        assert_eq!(body["error"], code, "the error code: {body}");
        assert_eq!(body["message"], message, "the message: {body}");
        let request_id = body["request_id"].as_str().unwrap_or_default();
        assert!(!request_id.is_empty(), "the request identifier: {body}");
        assert_eq!(
            self.header("x-request-id"),
            request_id,
            "the header repeats the identifier"
        );
    }

    /// Checks that the answer is the server error, which never says what went wrong.
    #[track_caller]
    pub(crate) fn check_internal_error(&self) {
        self.check_error(StatusCode::INTERNAL_SERVER_ERROR, "internal server error");
    }

    /// The value of a response header, or the empty string.
    pub(crate) fn header(&self, name: &str) -> String {
        self.headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_owned()
    }

    #[track_caller]
    pub(crate) fn check_status(&self, expected: StatusCode) {
        assert_eq!(
            self.status,
            expected,
            "unexpected status; body: {}",
            self.text()
        );
    }

    /// `CheckOK`.
    #[track_caller]
    pub(crate) fn check_ok(&self) {
        self.check_status(StatusCode::OK);
    }

    /// `CheckBadRequest`.
    #[track_caller]
    pub(crate) fn check_bad_request(&self) {
        self.check_status(StatusCode::BAD_REQUEST);
    }

    /// `CheckUnauthorized`.
    #[track_caller]
    pub(crate) fn check_unauthorized(&self) {
        self.check_status(StatusCode::UNAUTHORIZED);
    }

    /// `CheckForbidden`.
    #[track_caller]
    pub(crate) fn check_forbidden(&self) {
        self.check_status(StatusCode::FORBIDDEN);
    }

    /// `CheckNotFound`.
    #[track_caller]
    pub(crate) fn check_not_found(&self) {
        self.check_status(StatusCode::NOT_FOUND);
    }

    /// `CheckRequestEntityTooLarge`.
    #[track_caller]
    pub(crate) fn check_request_entity_too_large(&self) {
        self.check_status(StatusCode::PAYLOAD_TOO_LARGE);
    }

    /// `CheckNotImplemented`.
    #[track_caller]
    pub(crate) fn check_not_implemented(&self) {
        self.check_status(StatusCode::NOT_IMPLEMENTED);
    }

    /// Checks for status 200 and decodes the body.
    #[track_caller]
    pub(crate) fn ok_json<T: DeserializeOwned>(&self) -> T {
        self.check_ok();
        self.json()
    }
}
