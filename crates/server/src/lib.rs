//! The fvoci board server.
//!
//! A port of the original server's standalone mode: the `/api/v2` REST API, the `/ws`
//! websocket, and the static web app, on SQLite or PostgreSQL.

pub mod api;
pub mod app;
pub mod auth;
pub mod config;
pub mod error;
pub mod permissions;
pub mod scheduler;
pub mod storage;
pub mod web;
pub mod webhook;
pub mod ws;

use std::time::Duration;

use anyhow::{Context as _, bail};
use axum::Router;
use axum::body::Body;
use axum::extract::ws::WebSocketUpgrade;
use axum::extract::ws::rejection::WebSocketUpgradeRejection;
use axum::extract::{OriginalUri, Request, State};
use axum::http::header::ALLOW;
use axum::http::{HeaderValue, Method, StatusCode, Uri};
use axum::middleware;
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::get;
use fvoci_store::{DbType, Store};
use tokio_util::sync::CancellationToken;

pub use crate::request_log::Peer;
pub use crate::tls::TlsFiles;

use crate::admin_socket::AdminSocket;
use crate::app::App;
use crate::config::Configuration;
use crate::error::AppError;
use crate::request_log::ClientAddress;
use crate::scheduler::RecurringTask;
use crate::storage::FileBackend;
use crate::tls::TlsListener;

/// The environment variable that names a path prefix under which everything is served.
pub const BASE_PATH_ENV: &str = "FVOCI_HTTP_SERVER_BASEPATH";

/// How long shutdown waits for requests that are still being answered.
const CONNECTION_DRAIN_TIMEOUT: Duration = Duration::from_secs(10);

/// How long shutdown waits for notifications that are still being sent.
const BACKGROUND_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(10);

/// How often sessions that are no longer used are deleted
/// (`server/server/server.go:41`).
const CLEAN_UP_SESSIONS_INTERVAL: Duration = Duration::from_mins(10);

/// The shortest time a session stays in the database after its last use, in seconds: 31
/// days (`server/server/server.go:44`).
const MINIMUM_SESSION_CLEAN_UP_AGE: i64 = 60 * 60 * 24 * 31;

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
    /// `single_user_token` puts the server into single-user mode. `base_path` is a path
    /// prefix, such as `/boards`, under which everything is served; empty for none.
    ///
    /// # Errors
    ///
    /// Returns an error if the configuration asks for something unsupported, or the
    /// database or the file storage cannot be set up.
    pub async fn new(
        config: Configuration,
        single_user_token: Option<String>,
        base_path: &str,
    ) -> anyhow::Result<Self> {
        for warning in config.check()? {
            tracing::warn!("{warning}");
        }
        // An empty token would match every request that carries none.
        if single_user_token.as_deref() == Some("") {
            bail!("the single-user token must not be empty");
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
        let app = App::new(config, store, files, single_user_token)?;
        app.root_team().await.context("unable to get root team")?;
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
        let routes = Router::new()
            .nest("/api/v2", api)
            .merge(api::system_router())
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
        let client_address = if self.app.config().trust_forwarded_for {
            ClientAddress::ForwardedFor
        } else {
            ClientAddress::SocketPeer
        };
        router
            .layer(middleware::from_fn(request_log::contain_panics))
            .layer(middleware::from_fn_with_state(
                client_address,
                request_log::log_request,
            ))
    }

    /// Serves connections from `listener` until `shutdown` completes, then waits for
    /// notifications in flight and closes the database.
    ///
    /// Connections are served over TLS if the configuration has `usessl`. While serving, the administration socket is open
    /// if the configuration has `enablelocalmode`, and sessions that are no longer used
    /// are deleted every ten minutes.
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
        match Store::connect(db_type, &config.db_config, &config.db_table_prefix).await {
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

/// How long a session must have been unused before the clean-up deletes it, in seconds.
///
/// Preserved quirk: never less than 31 days, whatever the configured session lifetime, so
/// with a shorter lifetime sessions stay in the database after they stopped being
/// accepted (`server/server/server.go:258-261`).
fn session_clean_up_age(session_expire_time: i64) -> i64 {
    session_expire_time.max(MINIMUM_SESSION_CLEAN_UP_AGE)
}

/// Starts the task that deletes sessions nobody has used for a long time. The first run
/// is one interval after the start.
fn start_session_clean_up(app: &App) -> RecurringTask {
    let store = app.store().clone();
    let age = session_clean_up_age(app.config().session_expire_time);
    RecurringTask::spawn("cleanUpSessions", CLEAN_UP_SESSIONS_INTERVAL, move || {
        let store = store.clone();
        async move {
            if let Err(error) = store.clean_up_sessions(age).await {
                tracing::error!(%error, "unable to clean up the sessions");
            }
        }
    })
}

async fn handle_websocket(
    State(app): State<App>,
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
    // The commands a connection sends are small; a larger message is a mistake or an
    // attempt to make the server buffer it, and closes the connection.
    upgrade
        .max_message_size(api::WEBSOCKET_MESSAGE_LIMIT)
        .max_frame_size(api::WEBSOCKET_MESSAGE_LIMIT)
        .on_upgrade(move |socket| async move { app.ws().serve(socket).await })
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The clean-up age of `server/server/server.go:258-261`.
    #[test]
    fn sessions_are_cleaned_up_after_the_longer_of_31_days_and_their_lifetime() {
        let day = 60 * 60 * 24;
        assert_eq!(
            session_clean_up_age(30 * day),
            31 * day,
            "the default lifetime"
        );
        assert_eq!(session_clean_up_age(60), 31 * day, "a short lifetime");
        assert_eq!(session_clean_up_age(90 * day), 90 * day, "a long lifetime");
    }
}
