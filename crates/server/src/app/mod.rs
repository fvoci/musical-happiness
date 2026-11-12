//! The application layer: what the server does, independent of HTTP.
//!
//! [`App`] holds the server's shared state. Its operations are spread over the modules of
//! this directory by area, mirroring the original's `server/app` package.

mod access;
mod audience;
mod auth;
mod blocks;
mod boards;
mod boards_and_blocks;
mod cards;
mod category;
mod category_boards;
mod doomed;
mod download;
mod export;
mod files;
mod housekeeping;
mod import;
mod loose_objects;
mod oidc;
mod onboarding;
mod readiness;
mod reconcile;
mod search;
mod server_lock;
mod slots;

pub(crate) use download::download;
pub(crate) use server_lock::HeldLock;
pub(crate) use server_lock::take_server_lock;
use slots::Busy;
pub use slots::Slot;
pub(crate) use startup::{open_store, prepare_store};
mod shared_reads;
mod sharing;
mod startup;
mod store_metrics;
mod subscriptions;
mod templates;
mod trash;
mod uploads;
mod users;
mod workspaces;

pub use access::{BoardAccess, DeletedBoardAccess, as_managed};
pub use audience::Audience;
pub use auth::Admission;
pub use blocks::ChangeNotification;
pub use cards::CardPreviews;
pub use category_boards::DEFAULT_CATEGORY_BOARDS;
pub use export::ExportedArchive;
pub(crate) use files::FILE;
pub use import::Imported;
pub use loose_objects::{RemovedObjects, WRITING_GRACE_MILLIS};
pub use oidc::{
    CALLBACK_PATH, Callback, FinishedLogin, LOGIN_FLOW_LIFETIME_MILLIS, LoginFailure, StartedLogin,
    secret_context,
};
pub use readiness::{Readiness, ReadinessCheck};
pub use reconcile::{Found, Reconciliation, UNRECORDED_MIN_AGE_MILLIS, Unrecorded};
pub use shared_reads::{MAX_SHARED_READ_FAILURES, SHARED_READ_WINDOW, SharedReads};
pub use uploads::{
    CLOSED_UPLOAD_GRACE_MILLIS, CleanedUp, FileToUpload, MAX_OPEN_UPLOADS, MAX_UPLOAD_NAME_CHARS,
    PARTS_IN_MEMORY, PartDigest, UPLOAD_LIFETIME_MILLIS,
};
pub use users::sanitize;
pub use workspaces::Entered;

use std::sync::Arc;

use anyhow::Context as _;

use fvoci_model::ClientConfig;
use fvoci_store::{Store, Tenant};
use serde::Serialize;
use tokio_util::task::TaskTracker;

use crate::auth::Authenticator;
use crate::auth::hash_queue::HashQueue;
use crate::auth::limiter::{Limits, LoginLimiter};
use crate::auth::oidc::fetch::{Addresses, Fetcher};
use crate::config::Configuration;
use crate::error::{AppError, Reason};
use crate::metrics::{Metrics, OpenUploads, Pool};
use crate::storage::FileBackend;
use crate::web::WebRoot;
use crate::webhook::Webhook;
use crate::ws::Hub;

/// The version reported by `ping`. The original's web app compares it with its own.
pub const SERVER_VERSION: &str = "8.0.0";

/// The shared state of the server. Cloning is cheap; every clone is the same server.
///
/// A handle reaches the rows of one organization, the one its store names
/// ([`Tenant`]). [`App::scoped`] gives a handle to the same server that reaches other
/// rows; work that goes on after a request takes its handle along, and with it the
/// organization.
#[derive(Debug, Clone)]
pub struct App {
    inner: Arc<Inner>,
    store: Store,
    ws: Hub,
}

#[derive(Debug)]
struct Inner {
    config: Configuration,
    auth: Authenticator,
    login_limiter: LoginLimiter,
    password_work: HashQueue,
    files: FileBackend,
    web: WebRoot,
    webhook: Webhook,
    /// The client of the organizations' providers.
    oidc: Fetcher,
    /// The Prometheus metrics, kept if `prometheusaddress` is set.
    metrics: Metrics,
    /// Work that continues after a response was sent: websocket broadcasts and webhooks.
    background: TaskTracker,
    /// Held while a user's default category is looked for and created, so that two
    /// requests of a new user do not each create one.
    default_category: tokio::sync::Mutex<()>,
    /// Held while the limits on open uploads are checked and a session is stored, so
    /// that sessions opened at the same moment do not pass the limits together.
    opening_upload: tokio::sync::Mutex<()>,
    /// The uploads' turns (`uploads::UploadTurns`).
    upload_turns: uploads::UploadTurns,
    /// The memory the parts of uploads to the object store may take while they are
    /// received, in KiB (`uploads::PARTS_IN_MEMORY`).
    part_memory: Arc<tokio::sync::Semaphore>,
    /// The last answer of the readiness checks.
    readiness: readiness::Kept,
    /// The exports that run.
    exports: slots::Slots,
    /// The imports that run.
    imports: slots::Slots,
    /// Whether this process holds the lock of the server (`server_lock`).
    server_lock: server_lock::Holder,
    /// The limit on reads of shared boards whose token opens nothing.
    shared_reads: SharedReads,
}

/// What `ping` reports about the server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, utoipa::ToSchema)]
pub struct ServerMetadata {
    /// The server version.
    pub version: String,
    /// The build number; empty unless set at build time.
    pub build_number: String,
    /// The build date; empty unless set at build time.
    pub build_date: String,
    /// The commit the server was built from; empty unless set at build time.
    pub commit: String,
    /// The edition of the server.
    pub edition: String,
    /// The database kind: `sqlite3` or `postgres`.
    pub db_type: String,
    /// The database's version string.
    pub db_version: String,
    /// The operating system the server runs on.
    pub os_type: String,
    /// The CPU architecture the server runs on.
    pub os_arch: String,
    /// The kind of deployment: always `personal_server`.
    pub sku: String,
}

impl App {
    /// Assembles the application state.
    ///
    /// # Errors
    ///
    /// Returns an error if `serverRoot` or an allowed origin is no URL, if webhooks
    /// are configured and their HTTP client cannot be set up, or if `prometheusaddress`
    /// is no address.
    pub fn new(config: Configuration, store: Store, files: FileBackend) -> anyhow::Result<Self> {
        let metrics = if config.metrics_address()?.is_some() {
            Metrics::kept()?
        } else {
            Metrics::none()
        };
        if metrics.is_kept() {
            metrics.db_known(store_metrics::database(store.db_type()));
            if !store.observe(Box::new(store_metrics::StoreMetrics(metrics.clone()))) {
                tracing::warn!("the store has an observer already; its calls are not measured");
            }
        }
        store.set_comment_versions_kept(config.max_comment_versions);
        // The authenticator reads what is no organization's (sessions), and asks the
        // directory whose a shared board is: it is given no organization to reach.
        let auth = Authenticator::new(store.scoped(Tenant::Unset), &config)?;
        let login_limiter =
            LoginLimiter::new(Limits::from_config(&config)).with_metrics(metrics.clone());
        let password_work = HashQueue::new(
            usize::try_from(config.password_hash_concurrency).unwrap_or(usize::MAX),
            usize::try_from(config.password_hash_queue).unwrap_or(usize::MAX),
            std::time::Duration::from_secs(config.password_hash_queue_seconds),
        );
        let ping_interval = (config.websocket_ping_seconds > 0)
            .then(|| std::time::Duration::from_secs(config.websocket_ping_seconds));
        let ws = Hub::new(
            Audience::new(store.clone()),
            auth.clone(),
            ping_interval,
            metrics.clone(),
        );
        let files = files.measured(metrics.clone());
        let web = WebRoot::new(&config.web_path, &config.server_root);
        let webhook = Webhook::new(config.webhook_update.clone())
            .context("unable to set up the webhook client")?;
        let oidc = Fetcher::new(
            if config.oidc_allow_private_addresses {
                Addresses::Any
            } else {
                Addresses::PublicOnly
            },
            &config.oidc_extra_root_certificates,
        )
        .context("unable to set up the client of the providers")?;
        let part_memory = uploads::part_memory_kib(config.upload_part_bytes);
        let slots =
            |capacity: u32| slots::Slots::new(usize::try_from(capacity).unwrap_or(usize::MAX));
        let exports = slots(config.max_concurrent_exports);
        let imports = slots(config.max_concurrent_imports);
        Ok(Self {
            store,
            ws,
            inner: Arc::new(Inner {
                config,
                auth,
                login_limiter,
                password_work,
                files,
                web,
                webhook,
                oidc,
                metrics,
                background: TaskTracker::new(),
                default_category: tokio::sync::Mutex::new(()),
                opening_upload: tokio::sync::Mutex::new(()),
                upload_turns: uploads::UploadTurns::default(),
                part_memory: Arc::new(tokio::sync::Semaphore::new(part_memory)),
                readiness: readiness::Kept::default(),
                exports,
                imports,
                server_lock: server_lock::Holder::default(),
                shared_reads: SharedReads::default(),
            }),
        })
    }

    /// The Prometheus metrics; they do nothing unless `prometheusaddress` is set.
    pub fn metrics(&self) -> &Metrics {
        &self.inner.metrics
    }

    /// The server configuration.
    pub fn config(&self) -> &Configuration {
        &self.inner.config
    }

    /// The same server, reaching the rows of `tenant`: of the organization a request
    /// names, of the system organization that holds the built-in templates, or of all
    /// ([`Tenant::System`], for the work the server does on nobody's behalf).
    #[must_use]
    pub fn scoped(&self, tenant: Tenant) -> Self {
        let store = self.store.scoped(tenant);
        // The hub is the same, with the same connections; what it asks about the
        // boards, to know whom a change is told to, it asks as this handle reaches them.
        let mut ws = self.ws.clone();
        ws.audience = Audience::new(store.clone());
        Self {
            inner: Arc::clone(&self.inner),
            ws,
            store,
        }
    }

    /// The database, as this handle reaches it.
    pub fn store(&self) -> &Store {
        &self.store
    }

    /// The client of the organizations' providers.
    pub(crate) fn oidc_fetcher(&self) -> &Fetcher {
        &self.inner.oidc
    }

    /// The session and read-token checks.
    pub fn authenticator(&self) -> &Authenticator {
        &self.inner.auth
    }

    /// The limit on reads of shared boards whose token opens nothing.
    pub(crate) fn shared_reads(&self) -> &SharedReads {
        &self.inner.shared_reads
    }

    /// The limits on login attempts.
    pub(crate) fn login_limiter(&self) -> &LoginLimiter {
        &self.inner.login_limiter
    }

    /// The queue passwords are hashed and checked in.
    pub(crate) fn inner_password_work(&self) -> &HashQueue {
        &self.inner.password_work
    }

    /// The websocket hub.
    pub fn ws(&self) -> &Hub {
        &self.ws
    }

    /// The file storage.
    pub fn files(&self) -> &FileBackend {
        &self.inner.files
    }

    /// The web app directory.
    pub fn web(&self) -> &WebRoot {
        &self.inner.web
    }

    /// The webhook client.
    pub fn webhook(&self) -> &Webhook {
        &self.inner.webhook
    }

    /// Waits for the turn to create a user's default category.
    pub(crate) async fn default_category_turn(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.inner.default_category.lock().await
    }

    /// Waits for the turn to check the limits on open uploads and store a session.
    pub(crate) async fn opening_upload_turn(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.inner.opening_upload.lock().await
    }

    /// The turns of the uploads.
    pub(crate) fn upload_turns(&self) -> &uploads::UploadTurns {
        &self.inner.upload_turns
    }

    /// The memory parts of uploads to the object store may take, in KiB.
    pub(crate) fn part_memory(&self) -> &Arc<tokio::sync::Semaphore> {
        &self.inner.part_memory
    }

    /// Runs `work` in the background, after the caller has returned. Used for
    /// notifications that must not delay or fail a request.
    pub fn spawn_background(&self, work: impl Future<Output = ()> + Send + 'static) {
        self.inner.background.spawn(work);
    }

    /// Runs `work` as a task of its own and returns its handle. Used to let a request
    /// handler finish what it started when its client goes away; shutdown waits for these
    /// tasks as it does for background work.
    pub(crate) fn spawn_tracked<T: Send + 'static>(
        &self,
        work: impl Future<Output = T> + Send + 'static,
    ) -> tokio::task::JoinHandle<T> {
        self.inner.background.spawn(work)
    }

    /// Waits until all background work has finished, or `timeout` has passed. Returns
    /// `false` on timeout.
    pub async fn finish_background(&self, timeout: std::time::Duration) -> bool {
        self.inner.background.close();
        let finished = tokio::time::timeout(timeout, self.inner.background.wait())
            .await
            .is_ok();
        self.inner.background.reopen();
        finished
    }

    /// Takes the place of an export of the user's, given back when the [`Slot`] is
    /// dropped.
    ///
    /// # Errors
    ///
    /// Returns a 429 if the user has an export running (reason `export_running`) or the
    /// server runs as many as `max_concurrent_exports` (reason `server_exports_busy`).
    pub fn start_export(&self, user_id: &str) -> Result<Slot, AppError> {
        self.inner.exports.take(user_id).map_err(|busy| match busy {
            Busy::User => AppError::limited(
                "an export of yours is running; try again when it is done",
                Reason::ExportRunning,
            ),
            Busy::Server => AppError::limited(
                "the server runs as many exports as it may; try again later",
                Reason::ServerExportsBusy,
            ),
        })
    }

    /// How many exports and how many imports hold their place, as their limits count
    /// them: for the metrics.
    fn archives_running(&self) -> (usize, usize) {
        (self.inner.exports.held(), self.inner.imports.held())
    }

    /// Reads into the metrics what is measured by asking, for a scrape: the archives
    /// that hold their place, the connection pool, the open uploads and the live
    /// sessions. The last two stay as they were if the database cannot be read in time,
    /// which is counted, or another scrape is reading them.
    pub(crate) async fn read_storage_metrics(&self) {
        let metrics = self.metrics();
        if !metrics.is_kept() {
            return;
        }
        let (exports, imports) = self.archives_running();
        metrics.set_archives_running(exports, imports);
        let pool = self.store().pool_status();
        metrics.set_pool(Pool {
            connections: pool.connections,
            idle: pool.idle,
            max_connections: pool.max_connections,
        });
        let now = crate::auth::now_millis();
        let store = self.store().clone();
        let open_uploads = async move {
            let open = store.open_uploads(now).await?;
            Ok::<_, fvoci_store::StoreError>(OpenUploads {
                sessions: open.sessions,
                bytes: open.bytes,
            })
        };
        let idle_since = now.saturating_sub(self.config().session_idle_millis());
        let store = self.store().clone();
        let live_sessions = async move { store.count_live_sessions(now, idle_since).await };
        tokio::join!(
            metrics.read_open_uploads(open_uploads),
            metrics.read_live_sessions(live_sessions),
        );
    }

    /// Takes the place of an import of the user's, as [`Self::start_export`] does.
    ///
    /// # Errors
    ///
    /// Returns a 429 if the user has an import running (reason `import_running`) or the
    /// server runs as many as `max_concurrent_imports` (reason `server_imports_busy`).
    pub fn start_import(&self, user_id: &str) -> Result<Slot, AppError> {
        self.inner.imports.take(user_id).map_err(|busy| match busy {
            Busy::User => AppError::limited(
                "an import of yours is running; try again when it is done",
                Reason::ImportRunning,
            ),
            Busy::Server => AppError::limited(
                "the server runs as many imports as it may; try again later",
                Reason::ServerImportsBusy,
            ),
        })
    }

    /// Returns whether the user may read the board. Who reads a shared board by its
    /// read token and may not is told no user's identifier (`Board::hide_people`,
    /// `Block::hide_people`).
    ///
    /// # Errors
    ///
    /// Returns the store's error if a fact of the board cannot be read.
    pub async fn is_board_reader(&self, user_id: &str, board_id: &str) -> Result<bool, AppError> {
        Ok(self.board_grant(user_id, board_id).await?.reads())
    }

    /// The part of the configuration the web app needs.
    pub fn client_config(&self) -> ClientConfig {
        let config = self.config();
        ClientConfig {
            enable_public_shared_boards: config.enable_public_shared_boards,
            teammate_name_display: config.teammate_name_display.clone(),
            feature_flags: config.feature_flags.clone(),
            max_file_size: config.max_file_size,
            max_import_size: i64::try_from(config.max_import_bytes).unwrap_or(i64::MAX),
        }
    }

    /// What `ping` reports.
    pub async fn server_metadata(&self) -> ServerMetadata {
        let db_version = match self.store().db_version().await {
            Ok(version) => version,
            Err(error) => {
                tracing::error!(%error, "error checking database version");
                String::new()
            }
        };
        ServerMetadata {
            version: SERVER_VERSION.to_owned(),
            build_number: option_env!("FVOCI_BUILD_NUMBER")
                .unwrap_or_default()
                .to_owned(),
            build_date: option_env!("FVOCI_BUILD_DATE")
                .unwrap_or_default()
                .to_owned(),
            commit: option_env!("FVOCI_BUILD_HASH")
                .unwrap_or_default()
                .to_owned(),
            edition: option_env!("FVOCI_EDITION").unwrap_or("dev").to_owned(),
            db_type: self.store().db_type().as_str().to_owned(),
            db_version,
            os_type: go_os_name().to_owned(),
            os_arch: go_arch_name().to_owned(),
            sku: "personal_server".to_owned(),
        }
    }
}

/// The operating system name as the original reports it.
fn go_os_name() -> &'static str {
    match std::env::consts::OS {
        "macos" => "darwin",
        other => other,
    }
}

/// The architecture name as the original reports it.
fn go_arch_name() -> &'static str {
    match std::env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        "x86" => "386",
        other => other,
    }
}
