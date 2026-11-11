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
mod export;
mod files;
mod import;
mod onboarding;
mod search;
mod sharing;
mod subscriptions;
mod teams;
mod templates;
mod uploads;
mod users;

pub use auth::Admission;
pub use blocks::ChangeNotification;
pub use category_boards::DEFAULT_CATEGORY_BOARDS;
pub(crate) use files::FILE;
pub use uploads::{FileToUpload, MAX_UPLOAD_NAME_CHARS, PartDigest, UPLOAD_LIFETIME_MILLIS};
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
use crate::error::AppError;
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
