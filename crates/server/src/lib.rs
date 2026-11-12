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
pub mod metrics;
pub mod permissions;
pub mod problems;
pub mod scheduler;
pub mod storage;
pub mod web;
pub mod webhook;
pub mod ws;

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context as _, bail};
use axum::Router;
use axum::body::Body;
use axum::extract::ws::WebSocketUpgrade;
use axum::extract::ws::rejection::WebSocketUpgradeRejection;
use axum::extract::{OriginalUri, Request, State};
use axum::http::header::{ACCEPT_ENCODING, ALLOW, CONTENT_TYPE, ORIGIN, REFERRER_POLICY};
use axum::http::{Extensions, HeaderMap, HeaderValue, Method, StatusCode, Uri, Version};
use axum::middleware;
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::get;
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
use crate::connection::{BreakableListener, Connection};
use crate::error::{AppError, ErrorKind};
use crate::error_response::ReportedError;
use crate::metrics::{Job, JobOutcome};
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
    /// The listener of the metrics, if `prometheusaddress` is set.
    metrics_listener: Option<tokio::net::TcpListener>,
    /// The lock of the server of the database, held from the start to the shutdown.
    lock: Option<app::HeldLock>,
}

impl Server {
    /// Connects to the database and the file storage and prepares the application.
    ///
    /// `base_path` is a path prefix, such as `/boards`, under which everything is served;
    /// empty for none.
    ///
    /// # Errors
    ///
    /// Returns an error if the configuration asks for something unsupported, the
    /// database or the file storage cannot be set up, another process holds the lock of
    /// the server of the database for longer than `server_lock_wait_seconds`, or the
    /// metrics listener cannot listen at `prometheusaddress`.
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
        let store = app::open_store(&config).await?;
        // Before anything is changed or cleaned up: what is left over is nobody's only
        // if no other process serves the database.
        let server_lock = app::take_server_lock(&store, &config).await?;
        let store = app::prepare_store(store).await?;
        let files =
            FileBackend::from_config(&config).context("unable to initialize the files storage")?;
        let skip_template_init = config.skip_template_init;
        let app = App::new(config, store, files)?;
        // Before the slow work below: an address that is taken is reported at once.
        let metrics_listener = match app.config().metrics_address()? {
            Some(address) => Some(metrics_listener::bind(&address).await?),
            None => None,
        };
        app.root_workspace()
            .await
            .context("unable to get root workspace")?;
        let server_lock = app.hold_server_lock(server_lock);
        // Before any request: what is left over from a run that stopped is nobody's.
        clean_up_uploads(&app).await;
        match app.housekeeping().sweep_temporary_files().await {
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
            metrics_listener,
            lock: Some(server_lock),
        })
    }

    /// Reads the certificate and the key for `usessl` from these files instead of
    /// `./cert/cert.pem` and `./cert/key.pem`.
    #[must_use]
    pub fn with_tls_files(mut self, tls_files: TlsFiles) -> Self {
        self.tls_files = tls_files;
        self
    }

    /// The address the metrics are served at; `None` if `prometheusaddress` is empty,
    /// and nothing listens for them.
    pub fn metrics_address(&self) -> Option<SocketAddr> {
        self.metrics_listener
            .as_ref()
            .and_then(|listener| listener.local_addr().ok())
    }

    /// The application state.
    pub fn app(&self) -> &App {
        &self.app
    }

    /// The router that serves the API, the websocket and the web app.
    ///
    /// Every request is logged, and a handler that panics answers with a 500. If the
    /// metrics are kept, every request is measured.
    pub fn router(&self) -> Router {
        let measured = self.app.metrics().is_kept();
        let api = api::router(&self.app)
            .route_layer(middleware::from_fn_with_state(
                self.app.clone(),
                api::run_to_completion,
            ))
            // Under the API prefix nothing is ever answered with the web app's page.
            .fallback(api_not_found)
            .method_not_allowed_fallback(method_not_allowed)
            // Nor with an error in the framework's words.
            .layer(middleware::from_fn(error_response::problems_only));
        let api = if self.app.config().compress_responses {
            api.layer(compression())
                .layer(middleware::from_fn(identity_is_always_acceptable))
        } else {
            api
        };
        let routes = Router::new()
            .nest(api::PREFIX, api)
            .route("/ws", get(handle_websocket))
            // What the `type` of a problem points at.
            .merge(problems::routes())
            .route("/static/{*path}", get(handle_static))
            .fallback(handle_fallback)
            .method_not_allowed_fallback(method_not_allowed)
            .with_state(self.app.clone());
        // Nesting puts the API's routes into this router, so they get the layer too; its
        // fallbacks do not, and their answers are measured as unmatched.
        let routes = if measured {
            routes.route_layer(middleware::from_fn(request_metrics::note_route))
        } else {
            routes
        };
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
        let routes = self.with_request_layers(routes, &self.base_path);
        if measured {
            request_metrics::know_problems(self.app.metrics());
            routes.layer(middleware::from_fn_with_state(
                request_metrics::Measured {
                    metrics: self.app.metrics().clone(),
                    base_path: self.base_path.as_str().into(),
                },
                request_metrics::measure_requests,
            ))
        } else {
            routes
        }
    }

    /// The router of the administration socket. It must never be served on a TCP
    /// listener; its routes refuse requests that did not arrive through a Unix socket.
    pub fn admin_router(&self) -> Router {
        // The socket has no base path: its routes are at their full paths, and so are
        // the pages its error answers point at.
        self.with_request_layers(
            api::admin_router()
                .layer(middleware::from_fn(error_response::problems_only))
                .with_state(self.app.clone())
                .merge(problems::routes()),
            "",
        )
    }

    /// Wraps `router` in the layers every listener has: outermost the request log, then
    /// the containment of handler panics, so that a contained panic is logged as a 500.
    fn with_request_layers(&self, router: Router, base_path: &str) -> Router {
        // The configuration was checked when the application was put together.
        let proxies = TrustedProxies::parse(&self.app.config().trusted_proxies).unwrap_or_default();
        router
            .layer(middleware::from_fn(send_no_referrer))
            .layer(middleware::from_fn(request_log::contain_panics))
            .layer(middleware::from_fn_with_state(
                Arc::new(request_log::Served {
                    proxies,
                    base_path: base_path.to_owned(),
                }),
                request_log::log_request,
            ))
    }

    /// Serves connections from `listener` until `shutdown` completes, then waits for
    /// notifications in flight and closes the database.
    ///
    /// Connections are served over TLS if the configuration has `usessl`. While serving,
    /// the administration socket is open if the configuration has `enablelocalmode`,
    /// the metrics are served, over plain HTTP, if it has `prometheusaddress`, and once
    /// at the start and then every hour the sessions that have ended are deleted and
    /// the trash is purged of what has been in it for too long.
    ///
    /// # Errors
    ///
    /// Returns an error, before any connection is accepted, if `usessl` is set and the
    /// certificate or the key is missing or cannot be used, or the administration socket
    /// cannot be opened. Returns an error if accepting connections fails.
    pub async fn serve(
        mut self,
        listener: tokio::net::TcpListener,
        shutdown: impl Future<Output = ()> + Send + 'static,
    ) -> anyhow::Result<()> {
        let metrics_listener = self.metrics_listener.take();
        let result = self.run(listener, metrics_listener, shutdown).await;
        self.shutdown().await;
        result
    }

    /// Everything between start and shutdown, in the order of the original's `Start` and
    /// `Shutdown` (`server/server/server.go:242-322`).
    async fn run(
        &self,
        listener: tokio::net::TcpListener,
        metrics_listener: Option<tokio::net::TcpListener>,
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

        let service = self
            .router()
            .into_make_service_with_connect_info::<Connection>();
        let stop = CancellationToken::new();
        // When the connections have to be closed by, once the server stops: one
        // deadline for the API's listener and the metrics listener together.
        let drain_by = std::sync::OnceLock::new();
        let drain_deadline =
            || *drain_by.get_or_init(|| tokio::time::Instant::now() + CONNECTION_DRAIN_TIMEOUT);
        let metrics_serving = metrics_listener
            .map(|listener| spawn_metrics_server(listener, &self.app, stop.clone()));
        let stopped = stop.clone().cancelled_owned();
        let serving = async move {
            if let Some(tls) = tls {
                tracing::info!(%address, "https server started");
                axum::serve(BreakableListener(TlsListener::new(listener, tls)), service)
                    .with_graceful_shutdown(stopped)
                    .await
            } else {
                tracing::info!(%address, "http server started");
                axum::serve(BreakableListener(listener), service)
                    .with_graceful_shutdown(stopped)
                    .await
            }
        };
        tokio::pin!(serving);
        let lost = self.app.server_lock_lost();
        tokio::pin!(lost);
        let served = tokio::select! {
            served = &mut serving => served,
            () = &mut lost => {
                stop.cancel();
                tokio::time::timeout_at(drain_deadline(), &mut serving)
                    .await
                    .unwrap_or(Ok(()))
                    .and(Err(std::io::Error::other("the lock of the server was lost")))
            }
            () = shutdown => {
                stop.cancel();
                // Requests under way may finish, but a client that stalls must not hold
                // the server. A scrape of the metrics drains in the same time.
                tokio::time::timeout_at(drain_deadline(), &mut serving)
                    .await
                    .unwrap_or_else(|_| {
                        tracing::warn!("connections still open after the drain timeout; closing them");
                        Ok(())
                    })
            }
        };
        // Also when serving failed, without a shutdown.
        stop.cancel();
        if let Some(mut metrics_serving) = metrics_serving
            && tokio::time::timeout_at(drain_deadline(), &mut metrics_serving)
                .await
                .is_err()
        {
            tracing::warn!("a metrics scrape still open after the drain timeout; closing it");
            metrics_serving.abort();
        }
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

    /// Waits for notifications in flight, lets the lock of the server go, and closes
    /// the database.
    pub async fn shutdown(mut self) {
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
        if let Some(server_lock) = self.lock.take() {
            self.app.release_server_lock(server_lock).await;
        }
        self.app.store().close().await;
        tracing::info!("server shut down");
    }
}

/// Serves the metrics from `listener` until `stop` is cancelled, in a task of its own.
/// Plain HTTP, whatever `usessl` says, and without the request log: a scrape every few
/// seconds would fill it.
fn spawn_metrics_server(
    listener: tokio::net::TcpListener,
    app: &App,
    stop: CancellationToken,
) -> tokio::task::JoinHandle<()> {
    let router = metrics_listener::router(app.clone());
    if let Ok(address) = listener.local_addr() {
        tracing::info!(%address, "metrics server started");
    }
    tokio::spawn(async move {
        if let Err(error) = axum::serve(listener, router)
            .with_graceful_shutdown(stop.cancelled_owned())
            .await
        {
            tracing::error!(%error, "the metrics server stopped");
        }
    })
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

/// Does the server's housekeeping once, now: what its periodic tasks do, each with the
/// reach it takes for itself, over every organization. The server does it by itself
/// while it serves; this is for its tests.
pub async fn keep_house(app: &App) {
    clean_up_sessions(app).await;
    // What became of each job is the metrics' to count, where the tasks run them.
    let _ = purge_trash(app).await;
    let _ = collect_unreferenced_files(app).await;
    let _ = remove_doomed_objects(app).await;
    clean_up_uploads(app).await;
}

/// Deletes the sessions that have ended: unused for the idle limit, or past their
/// absolute limit. What such a session still has open is closed.
async fn clean_up_sessions(app: &App) {
    let app = &app.housekeeping();
    if let Err(error) = app.require_server_lock() {
        tracing::warn!(%error, "the sessions that ended are not cleaned up");
        return;
    }
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

/// Removes for good what has been in the trash for longer than `trash_retention_days`.
/// The objects of the files of the boards that go are doomed with their records, and
/// removed by [`remove_doomed_objects`].
async fn purge_trash(app: &App) -> JobOutcome {
    let app = &app.housekeeping();
    if let Err(error) = app.require_server_lock() {
        tracing::warn!(%error, "the trash is not purged");
        return JobOutcome::Failed;
    }
    let deleted_before = auth::now_millis().saturating_sub(app.config().trash_retention_millis());
    match app.store().purge_trash(deleted_before).await {
        Ok(purged) => {
            if purged.records > 0 || !purged.files.is_empty() {
                tracing::info!(
                    records = purged.records,
                    files = purged.files.len(),
                    "removed old boards and blocks from the trash"
                );
            }
            JobOutcome::Done
        }
        Err(error) => {
            tracing::error!(%error, "unable to purge the trash");
            JobOutcome::Failed
        }
    }
}

/// Removes the records of the files that no block, live or in the trash, has shown for
/// as long as the trash keeps what is deleted (`docs/s6-design.md` 5.7). Their objects
/// are doomed with them, and removed by [`remove_doomed_objects`].
async fn collect_unreferenced_files(app: &App) -> JobOutcome {
    let app = &app.housekeeping();
    if let Err(error) = app.require_server_lock() {
        tracing::warn!(%error, "the files no block shows are not collected");
        return JobOutcome::Failed;
    }
    let before = auth::now_millis().saturating_sub(app.config().trash_retention_millis());
    match app.store().collect_unreferenced_files(before).await {
        Ok(files) => {
            if !files.is_empty() {
                tracing::info!(
                    files = files.len(),
                    "removed the records of the files no block has shown for the grace period"
                );
            }
            JobOutcome::Done
        }
        Err(error) => {
            tracing::error!(%error, "unable to collect the files no block shows");
            JobOutcome::Failed
        }
    }
}

/// Removes the objects of the file storage that are doomed, trying again later what
/// cannot be removed now.
async fn remove_doomed_objects(app: &App) -> JobOutcome {
    let app = &app.housekeeping();
    match app.remove_doomed_objects(auth::now_millis()).await {
        Ok(done) => {
            if done.removed > 0 || done.failed > 0 || done.kept > 0 {
                tracing::info!(
                    removed = done.removed,
                    failed = done.failed,
                    kept = done.kept,
                    "removed the doomed objects of the file storage"
                );
            }
            JobOutcome::from_left(done.failed + done.kept)
        }
        Err(error) => {
            tracing::error!(%error, "unable to remove the doomed objects");
            JobOutcome::Failed
        }
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

/// Starts the task that purges the trash, collects the files no block shows and removes
/// the objects that are doomed, as [`start_session_clean_up`] does.
fn start_trash_purge(app: &App) -> RecurringTask {
    let app = app.clone();
    RecurringTask::spawn_starting_now("purgeTrash", PURGE_TRASH_INTERVAL, move || {
        let app = app.clone();
        async move {
            let purged = purge_trash(&app).await;
            // After the purge: what it removed for good shows its files no more.
            let collected = collect_unreferenced_files(&app).await;
            // After both: what they doomed is removed in the same run.
            let removed = remove_doomed_objects(&app).await;
            app.metrics()
                .job_ran(Job::PurgeTrash, purged.and(collected).and(removed));
        }
    })
}

/// Throws away the upload sessions that have ended, with their parts.
async fn clean_up_uploads(app: &App) {
    let app = &app.housekeeping();
    let outcome = match app.clean_up_uploads(auth::now_millis()).await {
        Ok(cleaned) => {
            if cleaned.removed > 0 {
                tracing::info!(
                    removed = cleaned.removed,
                    "threw away the upload sessions that ended"
                );
            }
            JobOutcome::from_left(cleaned.left)
        }
        Err(error) => {
            tracing::error!(%error, "unable to clean up the upload sessions");
            JobOutcome::Failed
        }
    };
    app.metrics().job_ran(Job::CleanUpUploads, outcome);
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
/// Nothing else is compressed: an archive or a file is sent with its length, as it is,
/// and an error is not, as the request log rewrites its body to add the request's
/// identifier.
fn compression() -> CompressionLayer<impl Predicate> {
    CompressionLayer::new()
        .no_deflate()
        .compress_when(DefaultPredicate::new().and(is_compressed))
}

/// Whether an answer is compressed: JSON, by its `Content-Type`, that is no error.
fn is_compressed(_: StatusCode, _: Version, headers: &HeaderMap, extensions: &Extensions) -> bool {
    extensions.get::<ReportedError>().is_none()
        && headers
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.starts_with("application/json"))
}

/// Drops from `Accept-Encoding` the entries that refuse the uncompressed answer
/// (`identity;q=0`, `*;q=0`). The compression layer would answer such a request whose
/// answer it does not compress with 406 after the handler ran, a change made and
/// reported as refused; an answer as it is serves the client better.
async fn identity_is_always_acceptable(mut request: Request, next: middleware::Next) -> Response {
    if let Some(accepted) = request
        .headers()
        .get(ACCEPT_ENCODING)
        .and_then(|value| value.to_str().ok())
        .and_then(without_refused_identity)
    {
        let headers = request.headers_mut();
        match HeaderValue::from_str(&accepted) {
            Ok(value) if !accepted.is_empty() => {
                headers.insert(ACCEPT_ENCODING, value);
            }
            _ => {
                headers.remove(ACCEPT_ENCODING);
            }
        }
    }
    next.run(request).await
}

/// `Accept-Encoding` without its entries that refuse `identity`, or `None` if it has
/// none of them.
fn without_refused_identity(accepted: &str) -> Option<String> {
    let refuses = |entry: &str| {
        let mut parts = entry.split(';').map(str::trim);
        let name = parts.next().unwrap_or_default();
        let refused = parts.any(|parameter| {
            parameter
                .strip_prefix("q=")
                .and_then(|q| q.parse::<f32>().ok())
                .is_some_and(|q| q <= 0.0)
        });
        refused && (name == "*" || name.eq_ignore_ascii_case("identity"))
    };
    if !accepted.split(',').any(refuses) {
        return None;
    }
    let kept: Vec<&str> = accepted
        .split(',')
        .filter(|entry| !refuses(entry))
        .map(str::trim)
        .collect();
    Some(kept.join(", "))
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

pub mod connection;
mod error_response;
mod metrics_listener;
mod request_log;
mod request_metrics;

mod tls;

#[cfg(unix)]
mod admin_socket;

#[cfg(not(unix))]
#[path = "admin_socket_unsupported.rs"]
mod admin_socket;

#[cfg(test)]
mod tests {
    use super::without_refused_identity;

    #[test]
    fn only_the_entries_that_refuse_identity_are_dropped() {
        assert_eq!(
            without_refused_identity("gzip, br"),
            None,
            "nothing refused"
        );
        assert_eq!(
            without_refused_identity("gzip, identity;q=0"),
            Some("gzip".to_owned())
        );
        assert_eq!(
            without_refused_identity("deflate, *;q=0.000, br;q=0.5"),
            Some("deflate, br;q=0.5".to_owned())
        );
        assert_eq!(
            without_refused_identity("Identity; q=0"),
            Some(String::new()),
            "nothing left"
        );
        assert_eq!(without_refused_identity("identity;q=0.1"), None, "accepted");
    }
}
