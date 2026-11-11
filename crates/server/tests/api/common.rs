//! The test server and the client that talks to it; the counterpart of the original's
//! `server/integrationtests/clienttestlib.go` and `server/client/client.go`.
#![allow(
    dead_code,
    reason = "a shared helper module; each test module uses a different part of it"
)]

use std::path::{Path, PathBuf};
use std::time::Duration;

use fvoci_model::{
    Block, Board, BoardMember, BoardType, LoginRequest, RegisterRequest, User, Workspace, new_id,
};
use fvoci_server::app::App;
use fvoci_server::auth::secret::new_secret;
use fvoci_server::config::Configuration;
use fvoci_server::{Server, TlsFiles};
use fvoci_store::testing::TestDatabase;
use fvoci_store::{SharingChange, Store};
use reqwest::{Method, StatusCode};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use tokio::net::TcpStream;
use tokio::sync::oneshot;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

/// A websocket connection to a test server.
pub(crate) type WebSocket = WebSocketStream<MaybeTlsStream<TcpStream>>;

pub(crate) const USER1_USERNAME: &str = "user1";
pub(crate) const USER2_USERNAME: &str = "user2";
pub(crate) const PASSWORD: &str = "Pa$$word";
/// The root workspace of every test server: the oldest workspace, the one the server would have
/// made at its first start. The harness makes it before the server starts, so that
/// tests can name it.
pub(crate) const ROOT_WORKSPACE_ID: &str = "00000000-0000-7000-8000-000000000000";
/// A second workspace, the one most tests make their boards in.
pub(crate) const TEST_WORKSPACE_ID: &str = "00000000-0000-7000-8000-000000000001";
/// A third workspace of the organization, which nobody is let into: a workspace that is
/// there and hidden from every user of a test.
pub(crate) const HIDDEN_WORKSPACE_ID: &str = "00000000-0000-7000-8000-000000000002";
/// The identifier of nothing: well-formed, and never given to anything.
pub(crate) const UNKNOWN_ID: &str = "00000000-0000-7000-8000-00000000dead";

/// A running server with a database and directories of its own.
pub(crate) struct TestServer {
    /// The application state, for arranging and inspecting what the API has no route for.
    pub(crate) app: App,
    pub(crate) base_url: String,
    /// Where the metrics are served, if the server keeps them.
    pub(crate) metrics_address: Option<std::net::SocketAddr>,
    /// The first client; `user1` after [`TestServer::init_basic`].
    pub(crate) client: Client,
    /// The second client; `user2` after [`TestServer::init_basic`].
    pub(crate) client2: Client,
    /// The identifier of the stranger, once there is one.
    stranger: tokio::sync::OnceCell<String>,
    shutdown: oneshot::Sender<()>,
    serving: tokio::task::JoinHandle<anyhow::Result<()>>,
    database: TestDatabase,
    directory: PathBuf,
}

/// As whom a test server connects to PostgreSQL.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum DatabaseRole {
    /// The role of the tests, which the wall between organizations holds.
    #[default]
    HeldByTheWall,
    /// A superuser, whom no policy holds.
    Superuser,
}

/// What is in the database of a test server when it starts.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum Database {
    /// The organization of the tests with its two workspaces, under the identifiers the
    /// tests name them by.
    #[default]
    Seeded,
    /// Nothing: as a server finds it at its first start.
    Empty,
}

/// How a test server differs from the default one.
#[derive(Debug, Default)]
pub(crate) struct Setup {
    /// Whether `user1` is registered and signed in on both clients at the start.
    one_user: bool,
    public_shared_boards: bool,
    base_path: String,
    configure: Vec<fn(&mut Configuration)>,
    admin_socket: Option<PathBuf>,
    tls_files: Option<TlsFiles>,
    database_role: DatabaseRole,
    database: Database,
    /// Whether the built-in templates are imported at startup. Off by default so a
    /// server starts quickly; the counterpart of the original's `SkipTemplateInit`.
    with_templates: bool,
    /// `prometheusaddress`.
    metrics_at: Option<String>,
}

impl Setup {
    /// `SetupTestHelper`: native authentication, no users yet.
    pub(crate) fn native() -> Self {
        Self::default()
    }

    /// One user, `user1`, registered and signed in on both clients: for tests that are
    /// about what a user does and not about who may do it.
    pub(crate) fn one_user() -> Self {
        Self {
            one_user: true,
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

    /// Connects to PostgreSQL as a superuser: for the tests of what the server makes
    /// of a role that the wall between organizations does not hold.
    pub(crate) fn connecting_as_superuser(mut self) -> Self {
        self.database_role = DatabaseRole::Superuser;
        self
    }

    /// Starts on an empty database, as a server does the first time: the organization
    /// and its first workspace are the server's to make, and [`ROOT_WORKSPACE_ID`] and
    /// [`TEST_WORKSPACE_ID`] name nothing.
    pub(crate) fn on_an_empty_database(mut self) -> Self {
        self.database = Database::Empty;
        self
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

    /// Keeps the metrics and serves them at `address` (`prometheusaddress`).
    pub(crate) fn metrics_at(mut self, address: &str) -> Self {
        self.metrics_at = Some(address.to_owned());
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
            .join(new_id());
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

        let database = match self.database_role {
            DatabaseRole::HeldByTheWall => TestDatabase::create(&directory).await,
            DatabaseRole::Superuser => TestDatabase::create_as_superuser(&directory).await,
        };
        let mut config = Configuration {
            db_type: database.db_type().as_str().to_owned(),
            db_config: database.dbconfig().to_owned(),
            web_path: web_path.display().to_string(),
            files_driver: "local".to_owned(),
            files_path: directory.join("uploads").display().to_string(),
            enable_public_shared_boards: self.public_shared_boards,
            // Pings are tested on their own; elsewhere they would only be noise.
            websocket_ping_seconds: 0,
            // Importing the templates is slow; only tests that ask for them pay it.
            skip_template_init: !self.with_templates,
            // Every client of a test comes from one address, and many sign in at once.
            // The limit by address is tested where a test turns it on.
            login_address_attempts_per_minute: 0,
            ..Configuration::default()
        };
        if let Some(path) = &self.admin_socket {
            config.enable_local_mode = true;
            config.local_mode_socket_location = path.display().to_string();
        }
        if let Some(address) = &self.metrics_at {
            address.clone_into(&mut config.prometheus_address);
        }
        for change in &self.configure {
            change(&mut config);
        }

        if self.database == Database::Seeded {
            seed_workspaces(&config).await;
        }
        let mut server = match Server::new(config, &self.base_path).await {
            Ok(server) => server,
            Err(error) => {
                // The database of a server that did not start is of no further use.
                database.remove().await;
                return Err(error);
            }
        };
        if let Some(tls_files) = self.tls_files {
            server = server.with_tls_files(tls_files);
        }
        let app = server.app().clone();
        let metrics_address = server.metrics_address();
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("a local port is free");
        let address = listener.local_addr().expect("the listener has an address");
        let (shutdown, stopped) = oneshot::channel::<()>();
        let serving = tokio::spawn(server.serve(listener, async move {
            let _ = stopped.await;
        }));

        let base_url = format!("http://{address}{}", self.base_path);
        let mut server = TestServer {
            app,
            metrics_address,
            client: Client::new(&base_url, ""),
            client2: Client::new(&base_url, ""),
            base_url,
            stranger: tokio::sync::OnceCell::new(),
            shutdown,
            serving,
            database,
            directory,
        };
        if self.one_user {
            server.client = server
                .register_and_login(USER1_USERNAME, "user1@sample.com", PASSWORD, "")
                .await;
            server.client2 = server.client.clone();
        }
        Ok(server)
    }
}

/// Stores the organization of a test server and its two workspaces, the root workspace first.
async fn seed_workspaces(config: &Configuration) {
    let db_type = config
        .db_type
        .parse()
        .expect("the test database type is one the store knows");
    let store = Store::connect(db_type, &config.db_config)
        .await
        .expect("the test database takes the schema");
    let workspace = |workspace_id: &str| Workspace {
        id: workspace_id.to_owned(),
        signup_token: new_secret(),
        ..Workspace::default()
    };
    let org_id = store
        .create_organization("", &workspace(ROOT_WORKSPACE_ID))
        .await
        .expect("the organization of the test server is stored");
    let ours = store.for_org(&org_id).expect("an identifier");
    for workspace_id in [TEST_WORKSPACE_ID, HIDDEN_WORKSPACE_ID] {
        ours.create_workspace(&workspace(workspace_id))
            .await
            .expect("a further workspace of the test server is stored");
    }
    store.close().await;
}

impl TestServer {
    /// `InitBasic`: registers and signs in `user1` on the first client and `user2`, with
    /// the workspace's signup token, on the second.
    pub(crate) async fn init_basic(mut self) -> Self {
        self.client = self
            .register_and_login(USER1_USERNAME, "user1@sample.com", PASSWORD, "")
            .await;
        let workspace: Workspace = self
            .client
            .get(&format!("/w/{ROOT_WORKSPACE_ID}"))
            .await
            .ok_json();
        assert!(
            !workspace.signup_token.is_empty(),
            "the workspace has a signup token"
        );
        self.client2 = self
            .register_and_login(
                USER2_USERNAME,
                "user2@sample.com",
                PASSWORD,
                &workspace.signup_token,
            )
            .await;
        self
    }

    /// The identifier of a user who exists and never signs in: somebody to make boards
    /// and blocks on behalf of where the test's own users must not be involved. The
    /// user is stored on first use, past the registration route.
    pub(crate) async fn stranger(&self) -> String {
        self.stranger
            .get_or_init(|| async {
                let mut user = User {
                    id: new_id(),
                    username: "stranger".to_owned(),
                    email: "stranger@sample.com".to_owned(),
                    ..User::default()
                };
                self.app
                    .store()
                    .create_user(&mut user)
                    .await
                    .expect("the stranger is stored");
                user.id
            })
            .await
            .clone()
    }

    /// The store as the system organization reaches it: where the built-in templates
    /// are, which no other organization changes.
    pub(crate) fn system_store(&self) -> Store {
        self.app
            .store()
            .for_org(fvoci_store::SYSTEM_ORG_ID)
            .expect("the system organization's identifier")
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
            display_name: String::new(),
        };
        let user: User = self
            .anonymous()
            .post("/register", &request)
            .await
            .created_json();
        self.let_into_the_second_workspace(&user.id).await;
        self.login(username, password).await
    }

    /// Lets a user into the second workspace of the test server, [`TEST_WORKSPACE_ID`], where
    /// most tests make their boards. Whoever registers is in the root workspace, the
    /// organization's default workspace; the users of a test work in both. A server
    /// started on an empty database has no second workspace to be let into.
    pub(crate) async fn let_into_the_second_workspace(&self, user_id: &str) {
        match self
            .app
            .store()
            .add_workspace_member(
                TEST_WORKSPACE_ID,
                user_id,
                fvoci_model::WorkspaceRole::Member,
            )
            .await
        {
            Ok(()) => {}
            Err(error) if error.is_not_found() => {}
            Err(error) => panic!("the user is let into the second workspace: {error}"),
        }
    }

    /// `Login`: returns a client signed in as an existing user.
    pub(crate) async fn login(&self, username: &str, password: &str) -> Client {
        let mut client = self.anonymous();
        client.login(username, password).await.check_no_content();
        client
    }

    /// Registers one more user with the workspace's signup token and signs them in.
    pub(crate) async fn add_user(&self, username: &str) -> Client {
        let workspace = self
            .app
            .root_workspace()
            .await
            .expect("the root workspace exists");
        self.register_and_login(
            username,
            &format!("{username}@sample.com"),
            PASSWORD,
            &workspace.signup_token,
        )
        .await
    }

    /// Shares a board, past the route and on behalf of nobody, and returns the read
    /// token the server made for it.
    pub(crate) async fn share(&self, board_id: &str) -> String {
        self.app
            .set_sharing(board_id, SharingChange::On, "")
            .await
            .expect("the board is shared")
            .token
    }

    /// `CreateBoard`: creates an empty board as the first client's user.
    pub(crate) async fn create_board(&self, workspace_id: &str, board_type: BoardType) -> Board {
        let board = Board {
            workspace_id: workspace_id.to_owned(),
            board_type,
            ..Board::default()
        };
        self.client.post("/boards", &board).await.created_json()
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
    /// The session token; nothing is sent while it is empty.
    pub(crate) token: String,
    /// How the token is sent.
    carrier: Carrier,
}

/// How a client presents its session token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Carrier {
    /// In the `Authorization` header, as a client that is no browser does.
    Bearer,
    /// In the session cookie, as a browser does.
    Cookie,
}

/// The name of the session cookie of a server reached over plain HTTP, as the test
/// servers are.
pub(crate) const SESSION_COOKIE: &str = "fvoci_session";

/// What the server answered.
#[derive(Debug)]
pub(crate) struct ApiResponse {
    /// The path the request was sent to, the server's base path first.
    pub(crate) path: String,
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
            carrier: Carrier::Bearer,
        }
    }

    /// This client's session as a browser has it: the token goes out in the session
    /// cookie instead of the `Authorization` header.
    pub(crate) fn as_browser(&self) -> Self {
        Self {
            carrier: Carrier::Cookie,
            ..self.clone()
        }
    }

    /// Adds the client's token to a request, the way this client presents it.
    fn authorized(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        if self.token.is_empty() {
            return request;
        }
        match self.carrier {
            Carrier::Bearer => request.bearer_auth(&self.token),
            Carrier::Cookie => request.header("Cookie", format!("{SESSION_COOKIE}={}", self.token)),
        }
    }

    /// Opens a websocket connection as this client: the handshake carries the token as
    /// this client presents it, and an `Origin` header if it is a browser's.
    pub(crate) async fn connect_websocket(&self) -> WebSocket {
        let headers: Vec<(&str, String)> = match self.carrier {
            Carrier::Bearer => Vec::new(),
            // A browser names the page's origin; the test servers take localhost:8000
            // for their own address.
            Carrier::Cookie => vec![("Origin", "http://localhost:8000".to_owned())],
        };
        self.try_connect_websocket(&headers)
            .await
            .expect("the websocket endpoint accepts the connection")
    }

    /// Tries to open a websocket connection as this client, with further headers on the
    /// handshake. The error is the status the handshake was refused with.
    pub(crate) async fn try_connect_websocket(
        &self,
        headers: &[(&str, String)],
    ) -> Result<WebSocket, StatusCode> {
        use tokio_tungstenite::tungstenite::Error;
        use tokio_tungstenite::tungstenite::client::IntoClientRequest as _;
        use tokio_tungstenite::tungstenite::http::{HeaderName, HeaderValue};

        let mut request = self
            .websocket_url()
            .into_client_request()
            .expect("the websocket address is a URL");
        let mut add = |name: &str, value: &str| {
            request.headers_mut().insert(
                HeaderName::from_bytes(name.as_bytes()).expect("a header name"),
                HeaderValue::from_str(value).expect("a header value"),
            );
        };
        if !self.token.is_empty() {
            match self.carrier {
                Carrier::Bearer => add("Authorization", &format!("Bearer {}", self.token)),
                Carrier::Cookie => add("Cookie", &format!("{SESSION_COOKIE}={}", self.token)),
            }
        }
        for (name, value) in headers {
            add(name, value);
        }
        match tokio_tungstenite::connect_async(request).await {
            Ok((stream, _)) => Ok(stream),
            Err(Error::Http(response)) => {
                Err(StatusCode::from_u16(response.status().as_u16()).expect("a status code"))
            }
            Err(other) => panic!("the websocket handshake failed without an answer: {other}"),
        }
    }

    /// The address of a path as it is written, with nothing put before it but the
    /// server's own address: for the paths outside the API, and for those that are
    /// written out in full.
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
            .request(method, format!("{}/api/v1{path}", self.base_url))
            .header("X-Requested-With", "XMLHttpRequest");
        request = self.authorized(request);
        request
    }

    /// A request to an API path with the client's token and nothing else: without the
    /// header of the request-forgery check, as a link or an image tag asks.
    pub(crate) fn bare_request(&self, method: Method, path: &str) -> reqwest::RequestBuilder {
        self.authorized(
            self.http
                .request(method, format!("{}/api/v1{path}", self.base_url)),
        )
    }

    /// A request to any path of the server, with no headers added.
    pub(crate) fn raw_request(&self, method: Method, path: &str) -> reqwest::RequestBuilder {
        self.http.request(method, self.root_url(path))
    }

    /// Sends a request and reads the whole answer.
    pub(crate) async fn send(request: reqwest::RequestBuilder) -> ApiResponse {
        let response = request.send().await.expect("the server answers");
        let path = response.url().path().to_owned();
        let status = response.status();
        let headers = response.headers().clone();
        let body = response
            .bytes()
            .await
            .expect("the answer has a body")
            .to_vec();
        ApiResponse {
            path,
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
            username: username.to_owned(),
            email: String::new(),
            password: password.to_owned(),
        };
        let response = self.post("/login", &request).await;
        if response.status == StatusCode::NO_CONTENT {
            self.token = response
                .session_cookie()
                .expect("a login answers with the session cookie");
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
    /// The values of the `Set-Cookie` headers of the answer.
    pub(crate) fn set_cookies(&self) -> Vec<&str> {
        self.headers
            .get_all(reqwest::header::SET_COOKIE)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .collect()
    }

    /// The session token the answer sets in the session cookie, under either of its
    /// names; `None` if the answer sets none. The token is empty where the cookie is
    /// cleared.
    pub(crate) fn session_cookie(&self) -> Option<String> {
        self.set_cookies().into_iter().find_map(|cookie| {
            let (pair, _attributes) = cookie.split_once(';').unwrap_or((cookie, ""));
            let (name, value) = pair.split_once('=')?;
            matches!(name, "fvoci_session" | "__Host-fvoci_session").then(|| value.to_owned())
        })
    }

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

    /// The detail of an error answer: English text for developers.
    #[track_caller]
    pub(crate) fn error_message(&self) -> String {
        self.error_member("detail")
    }

    /// The type of an error answer, such as `/problems/not-found`.
    #[track_caller]
    pub(crate) fn problem_type(&self) -> String {
        self.error_member("type")
    }

    /// Where in the request the one thing is that an error answer refused: the JSON
    /// Pointer of a member of the body as it is, `parameter <name>` or `header <name>`.
    /// `None` if the answer has no `errors`.
    ///
    /// # Panics
    ///
    /// Panics if `errors` has more than one entry, or the entry not exactly one place.
    #[track_caller]
    pub(crate) fn refused_place(&self) -> Option<String> {
        let mut places = self.refused_places();
        assert!(places.len() <= 1, "one thing is refused: {}", self.text());
        places.pop()
    }

    /// As [`ApiResponse::refused_place`], for an answer that refuses something.
    #[track_caller]
    pub(crate) fn refused_at(&self) -> String {
        self.refused_place()
            .unwrap_or_else(|| panic!("the answer names nothing it refused: {}", self.text()))
    }

    /// Where each thing is that an error answer refused, in the order of `errors`, as
    /// [`ApiResponse::refused_place`] writes it.
    #[track_caller]
    pub(crate) fn refused_places(&self) -> Vec<String> {
        let body = self.value();
        let Some(errors) = body.get("errors") else {
            return Vec::new();
        };
        let errors = errors.as_array().expect("errors is a list");
        assert!(!errors.is_empty(), "errors is left out when it is empty");
        errors
            .iter()
            .map(|entry| {
                for member in ["detail", "reason"] {
                    assert!(
                        entry[member].as_str().is_some_and(|text| !text.is_empty()),
                        "an entry has its {member}: {entry}"
                    );
                }
                let places: Vec<String> = [
                    ("pointer", ""),
                    ("parameter", "parameter "),
                    ("header", "header "),
                ]
                .into_iter()
                .filter_map(|(member, prefix)| {
                    entry.get(member).map(|place| {
                        format!("{prefix}{}", place.as_str().expect("a place is text"))
                    })
                })
                .collect();
                assert_eq!(places.len(), 1, "an entry is in one place: {entry}");
                places.into_iter().next().unwrap_or_default()
            })
            .collect()
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

    /// Checks that the answer is the error with this status and detail, in the one
    /// format of the API's errors ([`ApiResponse::check_problem`]).
    #[track_caller]
    pub(crate) fn check_error(&self, status: StatusCode, message: &str) {
        self.check_status(status);
        let body = self.check_problem();
        assert_eq!(body["detail"], message, "the detail: {body}");
    }

    /// Checks that the answer is a problem of RFC 9457 as the API sends it, and returns
    /// it: `application/problem+json`, the status repeated in the body, the `type` of
    /// the `reason` or else of the status, its title, and the identifier of the
    /// request, which `instance` and the response header repeat.
    #[track_caller]
    pub(crate) fn check_problem(&self) -> Value {
        assert_eq!(
            self.header("content-type"),
            "application/problem+json",
            "the content type of an error: {}",
            self.text()
        );
        let body = self.value();
        assert_eq!(body["status"], self.status.as_u16(), "the status: {body}");
        let of_status = match self.status.as_u16() {
            400 => "bad-request",
            401 => "unauthorized",
            403 => "forbidden",
            404 => "not-found",
            405 => "method-not-allowed",
            409 => "conflict",
            412 => "precondition-failed",
            413 => "payload-too-large",
            416 => "range-not-satisfiable",
            429 => "rate-limited",
            500 => "internal",
            503 => "service-unavailable",
            other => panic!("status {other} is no error status of the API"),
        };
        let name = match body.get("reason") {
            Some(reason) => reason.as_str().expect("a reason is text").replace('_', "-"),
            None => of_status.to_owned(),
        };
        // Under the path the server is served at, which a test may have set: what
        // comes before the API's prefix, or the pages of the problems, in the path asked.
        let base_path = ["/api/v1", "/problems"]
            .iter()
            .find_map(|marker| self.path.find(marker))
            .map_or("", |at| &self.path[..at]);
        assert_eq!(
            body["type"],
            format!("{base_path}/problems/{name}"),
            "the type is of the reason, or else of the status: {body}"
        );
        let kind = fvoci_server::problems::find(&name);
        assert!(kind.is_some(), "a kind of problem the server lists: {body}");
        assert_eq!(
            body["title"],
            kind.map(|kind| kind.title).unwrap_or_default(),
            "the title of the type: {body}"
        );
        assert!(body["detail"].is_string(), "the detail: {body}");
        let request_id = body["requestId"].as_str().unwrap_or_default();
        assert!(!request_id.is_empty(), "the request identifier: {body}");
        assert_eq!(
            body["instance"],
            format!("urn:uuid:{request_id}"),
            "the instance is the request: {body}"
        );
        assert_eq!(
            self.header("x-request-id"),
            request_id,
            "the header repeats the identifier"
        );
        for old in ["error", "message", "field", "request_id", "retry_after"] {
            assert!(body.get(old).is_none(), "no member {old}: {body}");
        }
        self.refused_places();
        body
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
        // Every error the API answers with is a problem, whatever the test is about.
        if expected.is_client_error() || expected.is_server_error() {
            self.check_problem();
        }
    }

    /// `CheckOK`.
    #[track_caller]
    pub(crate) fn check_ok(&self) {
        self.check_status(StatusCode::OK);
    }

    /// Checks for status 201, the answer of an operation that made something new.
    #[track_caller]
    pub(crate) fn check_created(&self) {
        self.check_status(StatusCode::CREATED);
    }

    /// Checks for status 204, the answer of an operation without a result, and for what
    /// belongs to it: no body, and no `Content-Type` for the body that is not there.
    #[track_caller]
    pub(crate) fn check_no_content(&self) {
        self.check_status(StatusCode::NO_CONTENT);
        assert!(self.body.is_empty(), "a 204 has no body: {:?}", self.text());
        assert!(
            !self.headers.contains_key(reqwest::header::CONTENT_TYPE),
            "a 204 names no content type: {:?}",
            self.header("content-type")
        );
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

    /// Checks for status 201 and decodes the body: what the operation made.
    #[track_caller]
    pub(crate) fn created_json<T: DeserializeOwned>(&self) -> T {
        self.check_created();
        self.json()
    }
}
