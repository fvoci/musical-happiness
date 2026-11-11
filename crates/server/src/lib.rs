//! The fvoci board server.
//!
//! A port of the original server's standalone mode: the `/api/v1` REST API, the `/ws`
//! websocket, and the static web app, on SQLite or PostgreSQL.

pub mod api;
pub mod app;
pub mod auth;
pub mod client_address;
pub mod config;
pub mod error;
pub mod permissions;
pub mod scheduler;
pub mod storage;
pub mod web;
pub mod webhook;
pub mod ws;

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context as _, bail};
use axum::Router;
use axum::body::Body;
use axum::extract::ws::WebSocketUpgrade;
use axum::extract::ws::rejection::WebSocketUpgradeRejection;
use axum::extract::{OriginalUri, Request, State};
use axum::http::header::{ALLOW, CONTENT_TYPE, ORIGIN, REFERRER_POLICY};
use axum::http::{Extensions, HeaderMap, HeaderValue, Method, StatusCode, Uri, Version};
use axum::middleware;
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::get;
use fvoci_store::{DbType, Store};
use tokio_util::sync::CancellationToken;
use tower_http::compression::CompressionLayer;
use tower_http::compression::predicate::{DefaultPredicate, Predicate};

pub use crate::request_log::Peer;
pub use crate::tls::TlsFiles;

use crate::admin_socket::AdminSocket;
use crate::app::App;
use crate::auth::token::Credential;
use crate::client_address::TrustedProxies;
use crate::config::Configuration;
use crate::error::{AppError, ErrorKind};
use crate::scheduler::RecurringTask;
use crate::storage::FileBackend;
use crate::tls::TlsListener;

/// The environment variable that names a path prefix under which everything is served.
pub const BASE_PATH_ENV: &str = "FVOCI_HTTP_SERVER_BASEPATH";

/// How long shutdown waits for requests that are still being answered.
const CONNECTION_DRAIN_TIMEOUT: Duration = Duration::from_secs(10);

/// How long shutdown waits for notifications that are still being sent.
const BACKGROUND_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(10);

/// How often sessions that have ended are deleted. They are not accepted from the moment
/// they end; removing them is housekeeping.
const CLEAN_UP_SESSIONS_INTERVAL: Duration = Duration::from_hours(1);

/// How often the trash is looked through for what has been in it for too long.
const PURGE_TRASH_INTERVAL: Duration = Duration::from_hours(1);

/// How often upload sessions that have ended are thrown away, with their parts. They
/// are refused from the moment they end; this frees the disk.
const CLEAN_UP_UPLOADS_INTERVAL: Duration = Duration::from_hours(1);

/// A server that is set up and ready to accept connections.
#[derive(Debug)]
pub struct Server {
    app: App,
    base_path: String,
    tls_files: TlsFiles,
}

impl Server {
    /// Connects to the database and the file storage and prepares the application.
    ///
    /// `base_path` is a path prefix, such as `/boards`, under which everything is served;
    /// empty for none.
    ///
    /// # Errors
    ///
    /// Returns an error if the configuration asks for something unsupported, or the
    /// database or the file storage cannot be set up.
    pub async fn new(config: Configuration, base_path: &str) -> anyhow::Result<Self> {
        for warning in config.check()? {
            tracing::warn!("{warning}");
        }
        let base_path = base_path.trim_end_matches('/');
        // What the router would read as a pattern or could never match is refused here,
        // with words, and not by a panic of the router or by a server that answers 404
        // to everything.
        let is_malformed = !base_path.starts_with('/')
            || base_path.contains(['{', '}', '*', '?', '#', '\\'])
            || base_path.contains("//")
            || base_path.contains("/:")
            || base_path.chars().any(char::is_whitespace);
        if !base_path.is_empty() && is_malformed {
            bail!(
                "{BASE_PATH_ENV} must be empty or an absolute path such as /boards, not {base_path:?}"
            );
        }
        let store = connect_store(&config).await?;
        let files =
            FileBackend::from_config(&config).context("unable to initialize the files storage")?;
        let skip_template_init = config.skip_template_init;
        let app = App::new(config, store, files)?;
        app.root_team().await.context("unable to get root team")?;
        // Before any request: what is left over from a run that stopped is nobody's.
        clean_up_uploads(&app).await;
        match app.sweep_temporary_files().await {
            Ok(0) => {}
            Ok(removed) => tracing::info!(removed, "removed left-over temporary files"),
            Err(error) => tracing::error!(%error, "unable to sweep the temporary files"),
        }
        if !skip_template_init {
            app.init_templates()
                .await
                .context("unable to initialize templates")?;
        }
        Ok(Self {
            app,
            base_path: base_path.to_owned(),
            tls_files: TlsFiles::default(),
        })
    }

    /// Reads the certificate and the key for `usessl` from these files instead of
    /// `./cert/cert.pem` and `./cert/key.pem`.
    #[must_use]
    pub fn with_tls_files(mut self, tls_files: TlsFiles) -> Self {
        self.tls_files = tls_files;
        self
    }

    /// The application state.
    pub fn app(&self) -> &App {
        &self.app
    }

    /// The router that serves the API, the websocket and the web app.
    ///
    /// Every request is logged, and a handler that panics answers with a 500.
    pub fn router(&self) -> Router {
        let api = api::router(&self.app)
            .route_layer(middleware::from_fn_with_state(
                self.app.clone(),
                api::run_to_completion,
            ))
            // Under the API prefix nothing is ever answered with the web app's page.
            .fallback(api_not_found)
            .method_not_allowed_fallback(method_not_allowed);
        let api = if self.app.config().compress_responses {
            api.layer(compression())
        } else {
            api
        };
        let routes = Router::new()
            .nest(api::PREFIX, api)
            .route("/ws", get(handle_websocket))
            .route("/static/{*path}", get(handle_static))
            .fallback(handle_fallback)
            .method_not_allowed_fallback(method_not_allowed)
            .with_state(self.app.clone());
        let routes = if self.base_path.is_empty() {
            routes
        } else {
            // The base path with a slash is the web app's root as well; the nested router
            // gets the base path itself and what is below it.
            let root = Router::new()
                .route(&format!("{}/", self.base_path), get(handle_root))
                .method_not_allowed_fallback(method_not_allowed)
                .with_state(self.app.clone());
            Router::new()
                .nest(&self.base_path, routes)
                .merge(root)
                .fallback(|| async { web::not_found() })
        };
        self.with_request_layers(routes)
    }

    /// The router of the administration socket. It must never be served on a TCP
    /// listener; its routes refuse requests that did not arrive through a Unix socket.
    pub fn admin_router(&self) -> Router {
        self.with_request_layers(api::admin_router().with_state(self.app.clone()))
    }

    /// Wraps `router` in the layers every listener has: outermost the request log, then
    /// the containment of handler panics, so that a contained panic is logged as a 500.
    fn with_request_layers(&self, router: Router) -> Router {
        // The configuration was checked when the application was put together.
        let proxies = TrustedProxies::parse(&self.app.config().trusted_proxies).unwrap_or_default();
        router
            .layer(middleware::from_fn(send_no_referrer))
            .layer(middleware::from_fn(request_log::contain_panics))
            .layer(middleware::from_fn_with_state(
                Arc::new(proxies),
                request_log::log_request,
            ))
    }

    /// Serves connections from `listener` until `shutdown` completes, then waits for
    /// notifications in flight and closes the database.
    ///
    /// Connections are served over TLS if the configuration has `usessl`. While serving,
    /// the administration socket is open if the configuration has `enablelocalmode`,
    /// and once at the start and then every hour the sessions that have ended are
    /// deleted and the trash is purged of what has been in it for too long.
    ///
    /// # Errors
    ///
    /// Returns an error, before any connection is accepted, if `usessl` is set and the
    /// certificate or the key is missing or cannot be used, or the administration socket
    /// cannot be opened. Returns an error if accepting connections fails.
    pub async fn serve(
        self,
        listener: tokio::net::TcpListener,
        shutdown: impl Future<Output = ()> + Send + 'static,
    ) -> anyhow::Result<()> {
        let result = self.run(listener, shutdown).await;
        self.shutdown().await;
        result
    }

    /// Everything between start and shutdown, in the order of the original's `Start` and
    /// `Shutdown` (`server/server/server.go:242-322`).
    async fn run(
        &self,
        listener: tokio::net::TcpListener,
        shutdown: impl Future<Output = ()> + Send + 'static,
    ) -> anyhow::Result<()> {
        let tls = self.tls_config()?;
        let address = listener
            .local_addr()
            .context("reading the listener's address")?;
        let admin_socket = self.start_admin_socket()?;
        let clean_up = start_session_clean_up(&self.app);
        let purge = start_trash_purge(&self.app);
        let uploads = start_upload_clean_up(&self.app);

        let service = self.router().into_make_service_with_connect_info::<Peer>();
        let stop = CancellationToken::new();
        let stopped = stop.clone().cancelled_owned();
        let serving = async move {
            if let Some(tls) = tls {
                tracing::info!(%address, "https server started");
                axum::serve(TlsListener::new(listener, tls), service)
                    .with_graceful_shutdown(stopped)
                    .await
            } else {
                tracing::info!(%address, "http server started");
                axum::serve(listener, service)
                    .with_graceful_shutdown(stopped)
                    .await
            }
        };
        tokio::pin!(serving);
        let served = tokio::select! {
            served = &mut serving => served,
            () = shutdown => {
                stop.cancel();
                // Requests under way may finish, but a client that stalls must not hold
                // the server.
                tokio::time::timeout(CONNECTION_DRAIN_TIMEOUT, &mut serving)
                    .await
                    .unwrap_or_else(|_| {
                        tracing::warn!("connections still open after the drain timeout; closing them");
                        Ok(())
                    })
            }
        };
        tracing::info!("the server stopped accepting connections");

        if let Some(admin_socket) = admin_socket {
            admin_socket.stop().await;
        }
        clean_up.cancel().await;
        purge.cancel().await;
        uploads.cancel().await;
        served.context("serving HTTP")
    }

    /// The TLS settings to serve with, or `None` to serve plain HTTP.
    ///
    /// With `usessl` a missing certificate or key is an error. The original serves plain
    /// HTTP then, without a word (`server/web/webserver.go:113`): an operator who asked
    /// for TLS would be sending sessions and passwords in the clear without knowing.
    fn tls_config(&self) -> anyhow::Result<Option<tls::ServerConfig>> {
        if !self.app.config().use_ssl {
            return Ok(None);
        }
        let missing = self.tls_files.missing();
        if !missing.is_empty() {
            let missing: Vec<String> = missing
                .iter()
                .map(|path| path.display().to_string())
                .collect();
            bail!(
                "usessl is set, but there is no file {}; provide the certificate and the key, \
                 or turn usessl off to serve plain HTTP",
                missing.join(" and no file ")
            );
        }
        self.tls_files.load().map(Some)
    }

    /// Opens the administration socket if the configuration asks for it.
    fn start_admin_socket(&self) -> anyhow::Result<Option<AdminSocket>> {
        let config = self.app.config();
        if !config.enable_local_mode {
            return Ok(None);
        }
        let path = std::path::Path::new(&config.local_mode_socket_location);
        AdminSocket::start(path, self.admin_router())
            .with_context(|| format!("opening the administration socket {}", path.display()))
            .map(Some)
    }

    /// Waits for notifications in flight and closes the database.
    pub async fn shutdown(self) {
        // Websocket connections are not part of the HTTP server's own connections once
        // they are upgraded; they end here.
        self.app.ws().close();
        if !self
            .app
            .finish_background(BACKGROUND_SHUTDOWN_TIMEOUT)
            .await
        {
            tracing::warn!("background notifications did not finish before shutdown");
        }
        self.app.store().close().await;
        tracing::info!("server shut down");
    }
}

/// Returns the address the server listens on, in the original's notation: `localhost:8000`
/// for a local-only server, `:8000` for all interfaces.
pub fn listen_address(config: &Configuration) -> String {
    if config.local_only {
        format!("localhost:{}", config.port)
    } else {
        format!(":{}", config.port)
    }
}

/// Connects to the database, trying up to `dbpingattempts` times a second apart, so that
/// a server started together with its database waits for it.
async fn connect_store(config: &Configuration) -> anyhow::Result<Store> {
    let db_type: DbType = config.db_type.parse()?;
    let attempts = config.db_ping_attempts.max(1);
    let mut attempt = 1;
    loop {
        match Store::connect(db_type, &config.db_config).await {
            Ok(store) => return Ok(store),
            Err(fvoci_store::StoreError::Database(error)) if attempt < attempts => {
                tracing::warn!(%error, attempt, attempts, "the database is not reachable yet");
                attempt += 1;
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
            Err(error) => return Err(error).context("connecting to the database"),
        }
    }
}

/// Tells browsers not to say where a request comes from when a page of this server
/// leads elsewhere: the address of a shared board carries its read token.
async fn send_no_referrer(request: Request, next: middleware::Next) -> Response {
    let mut response = next.run(request).await;
    response
        .headers_mut()
        .entry(REFERRER_POLICY)
        .or_insert(HeaderValue::from_static("no-referrer"));
    response
}

/// Deletes the sessions that have ended: unused for the idle limit, or past their
/// absolute limit. What such a session still has open is closed.
async fn clean_up_sessions(app: &App) {
    let now = auth::now_millis();
    let idle_since = now.saturating_sub(app.config().session_idle_millis());
    match app.store().clean_up_sessions(now, idle_since).await {
        Ok(ended) => {
            for session_id in &ended {
                app.ws().close_session(session_id);
            }
            if !ended.is_empty() {
                tracing::debug!(ended = ended.len(), "removed sessions that had ended");
            }
        }
        Err(error) => tracing::error!(%error, "unable to clean up the sessions"),
    }
}

/// Removes for good what has been in the trash for longer than `trash_retention_days`,
/// and the files of the boards that go: nothing says where they are afterwards.
async fn purge_trash(app: &App) {
    let deleted_before = auth::now_millis().saturating_sub(app.config().trash_retention_millis());
    let purged = match app.store().purge_trash(deleted_before).await {
        Ok(purged) => purged,
        Err(error) => {
            tracing::error!(%error, "unable to purge the trash");
            return;
        }
    };
    let mut files_left = 0_usize;
    for file in &purged.files {
        if let Err(error) = app.files().delete(&file.path).await {
            files_left += 1;
            tracing::warn!(%error, path = file.path, "a file of a purged board could not be removed");
        }
    }
    if purged.records > 0 || !purged.files.is_empty() {
        tracing::info!(
            records = purged.records,
            files = purged.files.len(),
            files_left,
            "removed old boards and blocks from the trash"
        );
    }
}

/// Removes the files that no block, live or in the trash, has shown for as long as the
/// trash keeps what is deleted (`docs/s6-design.md` 5.7). A file whose record went but
/// that cannot be removed from the storage is logged: nothing names it any more.
async fn collect_unreferenced_files(app: &App) {
    let before = auth::now_millis().saturating_sub(app.config().trash_retention_millis());
    let files = match app.store().collect_unreferenced_files(before).await {
        Ok(files) => files,
        Err(error) => {
            tracing::error!(%error, "unable to collect the files no block shows");
            return;
        }
    };
    let mut files_left = 0_usize;
    for file in &files {
        if let Err(error) = app.files().delete(&file.path).await {
            files_left += 1;
            tracing::warn!(%error, path = file.path, "a file no block shows could not be removed");
        }
    }
    if !files.is_empty() {
        tracing::info!(
            files = files.len(),
            files_left,
            "removed the files no block has shown for the grace period"
        );
    }
}

/// Starts the task that deletes the sessions that have ended. It runs once at the
/// start, so that a server that is restarted often still gets to it, and then every
/// interval.
fn start_session_clean_up(app: &App) -> RecurringTask {
    let app = app.clone();
    RecurringTask::spawn_starting_now("cleanUpSessions", CLEAN_UP_SESSIONS_INTERVAL, move || {
        let app = app.clone();
        async move { clean_up_sessions(&app).await }
    })
}

/// Starts the task that purges the trash and then collects the files no block shows,
/// as [`start_session_clean_up`] does.
fn start_trash_purge(app: &App) -> RecurringTask {
    let app = app.clone();
    RecurringTask::spawn_starting_now("purgeTrash", PURGE_TRASH_INTERVAL, move || {
        let app = app.clone();
        async move {
            purge_trash(&app).await;
            // After the purge: what it removed for good shows its files no more.
            collect_unreferenced_files(&app).await;
        }
    })
}

/// Throws away the upload sessions that have ended, with their parts.
async fn clean_up_uploads(app: &App) {
    match app.clean_up_uploads(auth::now_millis()).await {
        Ok(0) => {}
        Ok(removed) => tracing::info!(removed, "threw away the upload sessions that ended"),
        Err(error) => tracing::error!(%error, "unable to clean up the upload sessions"),
    }
}

/// Starts the task that throws away the upload sessions that have ended. The server
/// does it once before it serves, so the first run is an interval later.
fn start_upload_clean_up(app: &App) -> RecurringTask {
    let app = app.clone();
    RecurringTask::spawn("cleanUpUploads", CLEAN_UP_UPLOADS_INTERVAL, move || {
        let app = app.clone();
        async move { clean_up_uploads(&app).await }
    })
}

/// The websocket endpoint. Whose connection it becomes is settled here, at the
/// handshake, and so is whether it is accepted at all:
///
/// * A handshake with an `Origin` header comes from a browser. The origin must be the
///   server's own (or one of `allowed_origins`): a browser lets any page open a
///   websocket to any host and sends the cookies of that host along, so nothing else
///   keeps another site's page from listening as the user. The session is the one of
///   the cookie; without a valid one the connection is anonymous and can only follow
///   shared boards with their read token.
/// * A handshake without an `Origin` header comes from something that is no browser,
///   and needs a session token in its `Authorization` header.
async fn handle_websocket(
    State(app): State<App>,
    headers: HeaderMap,
    upgrade: Result<WebSocketUpgrade, WebSocketUpgradeRejection>,
) -> Response {
    let upgrade = match upgrade {
        Ok(upgrade) => upgrade,
        // The endpoint is for a websocket handshake and nothing else; what is none is
        // refused in the API's own format.
        Err(rejection) => {
            let error = if rejection.status() == StatusCode::METHOD_NOT_ALLOWED {
                AppError::method_not_allowed()
            } else {
                AppError::bad_request("this endpoint takes a websocket handshake only")
            };
            let mut response = error.into_response();
            if response.status() == StatusCode::METHOD_NOT_ALLOWED {
                response
                    .headers_mut()
                    .insert(ALLOW, HeaderValue::from_static("GET"));
            }
            return response;
        }
    };
    let identity = match websocket_identity(&app, &headers).await {
        Ok(identity) => identity,
        Err(error) => return error.into_response(),
    };
    // The commands a connection sends are small; a larger message is a mistake or an
    // attempt to make the server buffer it, and closes the connection.
    upgrade
        .max_message_size(api::WEBSOCKET_MESSAGE_LIMIT)
        .max_frame_size(api::WEBSOCKET_MESSAGE_LIMIT)
        .on_upgrade(move |socket| async move { app.ws().serve(socket, identity).await })
}

/// Whose connection a websocket handshake opens; `None` for an anonymous one.
///
/// # Errors
///
/// Returns a 403 for a handshake from another origin, a 401 for one without an
/// `Origin` header that has no valid token in its `Authorization` header, and for any
/// handshake whose `Authorization` header carries a token that is no session's.
async fn websocket_identity(
    app: &App,
    headers: &HeaderMap,
) -> Result<Option<ws::Identity>, AppError> {
    let auth = app.authenticator();
    if auth.origins().foreign_origin(headers).is_some() {
        tracing::debug!("a websocket handshake from another origin was refused");
        return Err(api::foreign_origin());
    }
    let from_a_browser = headers.contains_key(ORIGIN);
    match auth.session_cookie().credential(headers) {
        // Sent on purpose: it is a session's, or the handshake is refused.
        Some(Credential::Bearer(token)) => {
            let session = auth.find_session(&token).await?;
            Ok(Some(ws::Identity { session, token }))
        }
        Some(Credential::Cookie(token)) if from_a_browser => {
            match auth.find_session(&token).await {
                Ok(session) => Ok(Some(ws::Identity { session, token })),
                // A cookie that outlived its session: the page may still follow a shared
                // board.
                Err(error) if error.kind() == ErrorKind::Unauthorized => Ok(None),
                Err(error) => Err(error),
            }
        }
        None if from_a_browser => Ok(None),
        Some(Credential::Cookie(_)) | None => Err(AppError::unauthorized(
            "a websocket handshake without an Origin header needs a session token in the \
             Authorization header",
        )),
    }
}

async fn handle_static(State(app): State<App>, request: Request<Body>) -> Response {
    // The path as the client sent it, still percent-encoded: the file service decodes
    // it, once.
    let path = request
        .uri()
        .path()
        .strip_prefix("/static/")
        .unwrap_or_default()
        .to_owned();
    app.web().serve_static(&path, request).await
}

/// The page that starts the web app, for its root.
async fn handle_root(State(app): State<App>) -> Response {
    app.web().serve_index().await
}

/// The answer to a path under the API prefix that is no route: with a trailing slash,
/// misspelled, or of another version.
async fn api_not_found() -> AppError {
    AppError::not_found("route")
}

/// The answer to a known path asked with a method it does not have. The router adds
/// the `Allow` header with the methods the path does have.
async fn method_not_allowed() -> AppError {
    AppError::method_not_allowed()
}

/// Compresses the JSON answers of the API as the client's `Accept-Encoding` asks: zstd,
/// brotli or gzip, the one it prefers (zstd on a tie), at the codecs' default levels.
/// Nothing else is compressed: an archive or a file is sent with its length, as it is.
fn compression() -> CompressionLayer<impl Predicate> {
    CompressionLayer::new()
        .no_deflate()
        .compress_when(DefaultPredicate::new().and(is_json))
}

/// Whether an answer is JSON, by its `Content-Type`.
fn is_json(_: StatusCode, _: Version, headers: &HeaderMap, _: &Extensions) -> bool {
    headers
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.starts_with("application/json"))
}

/// Everything that is neither an API route nor a static file is a route of the web app:
/// the page that starts it is the answer to a `GET`.
///
/// A path that ends in a slash is redirected to the one without, so that every screen
/// has one address. The root is the exception; with a base path it is the base path
/// itself, with or without the slash.
async fn handle_fallback(
    State(app): State<App>,
    method: Method,
    uri: Uri,
    OriginalUri(original): OriginalUri,
) -> Response {
    let path = uri.path();
    if path == "/api" || path.starts_with("/api/") {
        return AppError::not_found("route").into_response();
    }
    if path == "/static" || path.starts_with("/static/") {
        return web::not_found().into_response();
    }
    if method != Method::GET && method != Method::HEAD {
        let mut response = AppError::method_not_allowed().into_response();
        response
            .headers_mut()
            .insert(ALLOW, HeaderValue::from_static("GET, HEAD"));
        return response;
    }
    if path.len() > 1 && path.ends_with('/') {
        return match without_trailing_slash(&original) {
            Some(location) => Redirect::permanent(&location).into_response(),
            None => web::not_found().into_response(),
        };
    }
    app.web().serve_index().await
}

/// The address a path with a trailing slash is redirected to: the same path without
/// the slash, with its query.
///
/// `None` for a path with an empty segment or a backslash, such as `//host/` and
/// `/\host/`: without its last slash it would read as an address on another host.
fn without_trailing_slash(uri: &Uri) -> Option<String> {
    let path = uri.path().trim_end_matches('/');
    // Browsers read a backslash as a slash, so `/\host` is `//host` to them.
    if path.is_empty() || path.contains("//") || path.contains('\\') {
        return None;
    }
    Some(match uri.query() {
        Some(query) => format!("{path}?{query}"),
        None => path.to_owned(),
    })
}

mod request_log;

mod tls;

#[cfg(unix)]
mod admin_socket;

#[cfg(not(unix))]
#[path = "admin_socket_unsupported.rs"]
mod admin_socket;
