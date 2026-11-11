//! The application layer: what the server does, independent of HTTP.
//!
//! [`App`] holds the server's shared state. Its operations are spread over the modules of
//! this directory by area, mirroring the original's `server/app` package.

mod auth;
mod blocks;
mod boards;
mod boards_and_blocks;
mod cards;
mod category;
mod category_boards;
mod doomed;
mod export;
mod files;
mod import;
mod onboarding;
mod search;
mod slots;

use slots::Busy;
pub use slots::Slot;
mod sharing;
mod subscriptions;
mod teams;
mod templates;
mod uploads;
mod users;

pub use auth::Admission;
pub use blocks::ChangeNotification;
pub use category_boards::DEFAULT_CATEGORY_BOARDS;
pub use export::ExportedArchive;
pub(crate) use files::FILE;
pub use uploads::{
    CLOSED_UPLOAD_GRACE_MILLIS, FileToUpload, MAX_OPEN_UPLOADS, MAX_UPLOAD_NAME_CHARS,
    PARTS_IN_MEMORY, PartDigest, UPLOAD_LIFETIME_MILLIS,
};
pub use users::sanitize;

use std::sync::Arc;

use anyhow::Context as _;

use fvoci_model::{BoardMember, ClientConfig};
use fvoci_store::Store;
use serde::Serialize;
use tokio_util::task::TaskTracker;

use crate::auth::Authenticator;
use crate::auth::hash_queue::HashQueue;
use crate::auth::limiter::{Limits, LoginLimiter};
use crate::config::Configuration;
use crate::error::{AppError, Reason};
use crate::permissions::{
    BoardPermission, TeamPermission, has_permission_to_team, member_has_permission,
};
use crate::storage::FileBackend;
use crate::web::WebRoot;
use crate::webhook::Webhook;
use crate::ws::Hub;

/// The version reported by `ping`. The original's web app compares it with its own.
pub const SERVER_VERSION: &str = "8.0.0";

/// The shared state of the server. Cloning is cheap; every clone is the same server.
#[derive(Debug, Clone)]
pub struct App {
    inner: Arc<Inner>,
}

#[derive(Debug)]
struct Inner {
    config: Configuration,
    store: Store,
    auth: Authenticator,
    login_limiter: LoginLimiter,
    password_work: HashQueue,
    ws: Hub,
    files: FileBackend,
    web: WebRoot,
    webhook: Webhook,
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
    /// The exports that run.
    exports: slots::Slots,
    /// The imports that run.
    imports: slots::Slots,
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
    /// Returns an error if `serverRoot` or an allowed origin is no URL, or if webhooks
    /// are configured and their HTTP client cannot be set up.
    pub fn new(config: Configuration, store: Store, files: FileBackend) -> anyhow::Result<Self> {
        store.set_comment_versions_kept(config.max_comment_versions);
        let auth = Authenticator::new(store.clone(), &config)?;
        let login_limiter = LoginLimiter::new(Limits::from_config(&config));
        let password_work = HashQueue::new(
            usize::try_from(config.password_hash_concurrency).unwrap_or(usize::MAX),
            usize::try_from(config.password_hash_queue).unwrap_or(usize::MAX),
            std::time::Duration::from_secs(config.password_hash_queue_seconds),
        );
        let ping_interval = (config.websocket_ping_seconds > 0)
            .then(|| std::time::Duration::from_secs(config.websocket_ping_seconds));
        let ws = Hub::new(store.clone(), auth.clone(), ping_interval);
        let web = WebRoot::new(&config.web_path, &config.server_root);
        let webhook = Webhook::new(config.webhook_update.clone())
            .context("unable to set up the webhook client")?;
        let part_memory = uploads::part_memory_kib(config.upload_part_bytes);
        let slots =
            |capacity: u32| slots::Slots::new(usize::try_from(capacity).unwrap_or(usize::MAX));
        let exports = slots(config.max_concurrent_exports);
        let imports = slots(config.max_concurrent_imports);
        Ok(Self {
            inner: Arc::new(Inner {
                config,
                store,
                auth,
                login_limiter,
                password_work,
                ws,
                files,
                web,
                webhook,
                background: TaskTracker::new(),
                default_category: tokio::sync::Mutex::new(()),
                opening_upload: tokio::sync::Mutex::new(()),
                upload_turns: uploads::UploadTurns::default(),
                part_memory: Arc::new(tokio::sync::Semaphore::new(part_memory)),
                exports,
                imports,
            }),
        })
    }

    /// The server configuration.
    pub fn config(&self) -> &Configuration {
        &self.inner.config
    }

    /// The database.
    pub fn store(&self) -> &Store {
        &self.inner.store
    }

    /// The session and read-token checks.
    pub fn authenticator(&self) -> &Authenticator {
        &self.inner.auth
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
        &self.inner.ws
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

    /// Returns whether a user has a permission in a team.
    pub fn has_permission_to_team(
        &self,
        user_id: &str,
        team_id: &str,
        permission: TeamPermission,
    ) -> bool {
        has_permission_to_team(user_id, team_id, permission)
    }

    /// Returns whether a user has a permission on a board, which follows from their
    /// membership. No membership is "no".
    ///
    /// # Errors
    ///
    /// Returns the store's error if the membership cannot be read. That is not a "no":
    /// answered as one, a database failure would tell every member that their board
    /// does not exist.
    pub async fn has_permission_to_board(
        &self,
        user_id: &str,
        board_id: &str,
        permission: BoardPermission,
    ) -> Result<bool, AppError> {
        if user_id.is_empty() || board_id.is_empty() {
            return Ok(false);
        }
        match self.store().get_member_for_board(board_id, user_id).await {
            Ok(member) => Ok(member_has_permission(&member, permission)),
            Err(error) if error.is_not_found() => Ok(false),
            Err(error) => Err(AppError::from(anyhow::Error::from(error))
                .context("reading the membership of a board")),
        }
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

    /// Returns whether the user is a member of the board who may read it. Who reads a
    /// shared board by its read token and is not one is told no user's identifier
    /// (`Board::hide_people`, `Block::hide_people`).
    ///
    /// # Errors
    ///
    /// Returns the store's error if the membership cannot be read.
    pub async fn is_board_reader(&self, user_id: &str, board_id: &str) -> Result<bool, AppError> {
        self.has_permission_to_board(user_id, board_id, BoardPermission::ViewBoard)
            .await
    }

    /// Returns the membership of a user on a board, if there is one.
    ///
    /// # Errors
    ///
    /// Returns the store's error if the lookup fails for another reason than the
    /// membership not existing.
    pub async fn find_member_for_board(
        &self,
        board_id: &str,
        user_id: &str,
    ) -> Result<Option<BoardMember>, crate::error::AppError> {
        match self.store().get_member_for_board(board_id, user_id).await {
            Ok(member) => Ok(Some(member)),
            Err(error) if error.is_not_found() => Ok(None),
            Err(error) => Err(error.into()),
        }
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
