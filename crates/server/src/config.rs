//! The server configuration: a JSON file, `FVOCI_*` environment variables, and a few
//! command-line overrides.
//!
//! The file has the keys of the original server's `config.json`, so existing files keep
//! working. Keys are matched without regard to case, as the original does. Keys this
//! server does not know are ignored.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context as _, bail};
use serde::Deserialize;
use serde_json::{Map, Value};

/// The prefix of the environment variables that override configuration keys.
pub const ENV_PREFIX: &str = "FVOCI_";

/// The default of `max_request_bytes`: 4 MiB. The largest block the model accepts, a
/// title of 16,383 characters and fields of 800,000, takes up to about 3.3 MB as UTF-8,
/// so every single valid block fits. Requests of the web app measured below 1 kB.
pub const DEFAULT_MAX_REQUEST_BYTES: u64 = 4 * 1024 * 1024;

pub mod collab;
mod keys;
pub mod mail;
mod scrapes;
mod secret_key;
pub mod smtp;
mod upload_limits;

pub use scrapes::{MetricsAddress, MetricsToken};
pub use secret_key::{ENV_ONLY, SECRET_KEY_ENV};

use crate::auth::sealed::SecretKey;

/// The settings of the S3 file backend, the `filess3config` object.
#[derive(Clone, Default, Deserialize)]
#[cfg_attr(test, derive(serde::Serialize))]
#[serde(default)]
pub struct AmazonS3Config {
    /// The access key; empty to use the environment's credentials.
    #[serde(rename = "accesskeyid")]
    pub access_key_id: String,
    /// The secret key.
    #[serde(rename = "secretaccesskey")]
    pub secret_access_key: String,
    /// The bucket files are stored in.
    pub bucket: String,
    /// A prefix put before every object key.
    #[serde(rename = "pathprefix")]
    pub path_prefix: String,
    /// The region of the bucket.
    pub region: String,
    /// The endpoint host, with port if needed; empty for AWS.
    pub endpoint: String,
    /// Whether to talk to the endpoint over TLS.
    pub ssl: bool,
    /// Whether to sign requests with the legacy version 2 scheme. Not supported.
    #[serde(rename = "signv2")]
    pub sign_v2: bool,
    /// Whether to ask for server-side encryption of stored objects.
    pub sse: bool,
    /// The request timeout in milliseconds; zero for the default.
    pub timeout: i64,
}

impl std::fmt::Debug for AmazonS3Config {
    /// Leaves out the secret key.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AmazonS3Config")
            .field("access_key_id", &self.access_key_id)
            .field("bucket", &self.bucket)
            .field("path_prefix", &self.path_prefix)
            .field("region", &self.region)
            .field("endpoint", &self.endpoint)
            .field("ssl", &self.ssl)
            .field("sign_v2", &self.sign_v2)
            .field("sse", &self.sse)
            .field("timeout", &self.timeout)
            .finish_non_exhaustive()
    }
}

/// The server configuration.
///
/// The serde names are the keys of the configuration file in lower case; [`Configuration::read`]
/// lower-cases the file's keys before decoding. `docs/configuration.md` documents every
/// key, and a test holds the two together.
#[derive(Clone, Deserialize)]
#[cfg_attr(test, derive(serde::Serialize))]
#[serde(default)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "each flag is a separate key of the configuration file"
)]
pub struct Configuration {
    /// The public URL of the server. Its path is the base path the web app is served under.
    #[serde(rename = "serverroot")]
    pub server_root: String,
    /// The port to listen on.
    pub port: u16,
    /// The database kind: `sqlite3` or `postgres`.
    #[serde(rename = "dbtype")]
    pub db_type: String,
    /// The database connection string, in the format of the original server.
    #[serde(rename = "dbconfig")]
    pub db_config: String,
    /// How many times to try to reach the database at startup.
    #[serde(rename = "dbpingattempts")]
    pub db_ping_attempts: u32,
    /// How long a server that starts waits for the lock of the server of its database,
    /// which another process holds, before it refuses to start, in seconds.
    pub server_lock_wait_seconds: u64,
    /// Whether to serve HTTPS, with `./cert/cert.pem` and `./cert/key.pem`.
    #[serde(rename = "usessl")]
    pub use_ssl: bool,
    /// The directory holding the web app: `index.html` and `static/`.
    #[serde(rename = "webpath")]
    pub web_path: String,
    /// The file backend: `local` or `amazons3`.
    #[serde(rename = "filesdriver")]
    pub files_driver: String,
    /// The settings of the `amazons3` file backend.
    #[serde(rename = "filess3config")]
    pub files_s3_config: AmazonS3Config,
    /// The directory of the `local` file backend.
    #[serde(rename = "filespath")]
    pub files_path: String,
    /// The largest accepted upload in bytes: 100 MiB unless set. At least 1, and at most
    /// what an upload in parts carries (`upload_part_bytes` × 10,000).
    #[serde(rename = "maxfilesize")]
    pub max_file_size: i64,
    /// Where the Prometheus metrics are served, as `host:port`; empty for none. A
    /// listener of their own, apart from the API's ([`Configuration::metrics_address`]).
    #[serde(rename = "prometheusaddress")]
    pub prometheus_address: String,
    /// The bearer token a scrape of the metrics must send; empty for none. Never
    /// written by `Debug` ([`Configuration::metrics_token`]).
    #[serde(rename = "prometheustoken")]
    pub prometheus_token: String,
    /// URLs that receive a POST with every changed block.
    pub webhook_update: Vec<String>,
    /// Further origins whose pages may send requests that change something and open
    /// websocket connections, next to the origin of `serverRoot`. For development,
    /// where the web app is served by a tool of its own on another port.
    pub allowed_origins: Vec<String>,
    /// How many days a session may go unused before it ends.
    pub session_idle_days: u32,
    /// How many days after signing in a session ends, however much it is used.
    pub session_max_days: u32,
    /// Whether to listen on the loopback interface only.
    #[serde(rename = "localonly")]
    pub local_only: bool,
    /// Whether to serve the administration API on a Unix socket.
    #[serde(rename = "enablelocalmode")]
    pub enable_local_mode: bool,
    /// The path of the administration socket.
    #[serde(rename = "localmodesocketlocation")]
    pub local_mode_socket_location: String,
    /// Whether boards may be shared publicly through a read token.
    #[serde(rename = "enablepublicsharedboards")]
    pub enable_public_shared_boards: bool,
    /// Feature flags handed to the web app.
    #[serde(rename = "featureflags")]
    pub feature_flags: BTreeMap<String, String>,
    /// How the web app displays other users' names.
    #[serde(rename = "teammatenamedisplay")]
    pub teammate_name_display: String,
    /// Whether user lists include email addresses.
    #[serde(rename = "showemailaddress")]
    pub show_email_address: bool,
    /// Whether user lists include first and last names.
    #[serde(rename = "showfullname")]
    pub show_full_name: bool,
    /// The authentication mode. Only `native` is supported. Signing in through an
    /// organization's `OpenID Connect` provider is the organization's to set, beside it.
    #[serde(rename = "authmode")]
    pub auth_mode: String,
    /// Whether the providers of the organizations may be reached at addresses that are
    /// not public (loopback, private networks): for an installation whose provider is
    /// on its own network. Off, such a provider is neither stored nor signed in
    /// through.
    pub oidc_allow_private_addresses: bool,
    /// The key the server seals what it keeps secret in the database with: the client
    /// secrets of the providers. Read from `FVOCI_SECRET_KEY` only, never from the
    /// file, and never written by `Debug`. Without it no provider can be stored.
    #[serde(skip)]
    pub secret_key: Option<SecretKey>,
    /// Certificates (DER) that the client of the providers trusts besides the
    /// system's. Not a configuration file key: the tests set it for a provider of
    /// their own, as they set `skip_template_init`.
    #[serde(skip)]
    pub oidc_extra_root_certificates: Vec<Vec<u8>>,
    /// Whether the server holds one organization (`first_user_only`) or takes in
    /// several (`open`). `open` needs PostgreSQL, and a database role that the wall
    /// between organizations holds.
    pub org_creation: String,
    /// How often the server pings each websocket, in seconds; zero to never ping. Keeps
    /// proxies and CDNs from closing idle connections.
    pub websocket_ping_seconds: u64,
    /// How many connections may read one shared board by its public link at once:
    /// following its blocks by the link's read token, or visiting it with that token.
    /// One more is not taken, and is not counted among the board's anonymous readers.
    pub public_link_connections_per_board: u32,
    /// How many of a board's connections by its public link may come from one address
    /// (an IPv6 address counted by its /64), within `public_link_connections_per_board`.
    pub public_link_connections_per_address: u32,
    /// Whether the JSON answers of the API are compressed (gzip, brotli or zstd, as the
    /// client's `Accept-Encoding` asks). On unless set: a proxy in front that compresses
    /// itself can have it off.
    pub compress_responses: bool,
    /// The largest body, in bytes, of a request to an API route that takes JSON. A
    /// larger one is refused with 413. It does not apply to file uploads, which are
    /// limited to one part (`upload_part_bytes`) as they are read, nor to the parts of
    /// uploads, limited to their size, nor to archive imports, which have no limit on
    /// the request yet.
    pub max_request_bytes: u64,
    /// How many days a deleted board or block stays in the trash, where it can be
    /// restored from, before it is removed for good.
    pub trash_retention_days: u32,
    /// The size in bytes of every part of an upload in parts but the last
    /// (`docs/s6-design.md` 5.6). At least 5 MiB, the smallest part an S3 multipart
    /// upload takes.
    pub upload_part_bytes: u64,
    /// The directory of the server's temporary files: the parts of uploads that are not
    /// complete yet. Empty for `.tmp` under `filespath`. Not the system's temporary
    /// directory, which may be held in memory.
    pub tmp_path: String,
    /// The most bytes all open uploads in parts of the server may hold back at once:
    /// the sum of their declared sizes (`docs/s6-design.md` 5.6, 8).
    pub max_pending_upload_bytes: u64,
    /// The largest archive an import takes, in bytes: 256 MiB unless set. At least 1,
    /// and at most what an upload in parts carries (`upload_part_bytes` × 10,000).
    pub max_import_bytes: u64,
    /// How many earlier versions of a comment's body the server keeps: the first one,
    /// with which the comment was posted, and the newest. At least 1.
    pub max_comment_versions: u32,
    /// How many exports (`exportBoardArchive`, `exportWorkspaceArchive`) the server runs at
    /// once; a user runs one at a time. An export runs until its archive is sent. At
    /// least 1.
    pub max_concurrent_exports: u32,
    /// How long, in seconds, an export's download may send nothing before it is given
    /// up: its place among the exports and its temporary file are given back, and the
    /// connection is broken. At least 1.
    pub export_stall_seconds: u64,
    /// How many imports (`importArchive`) the server runs at once; a user runs one at a
    /// time. At least 1.
    pub max_concurrent_imports: u32,
    /// The addresses, and address ranges such as `10.0.0.0/8`, of the reverse proxies
    /// in front of the server. A request from one of them is taken to come from the
    /// address the proxy names in `X-Forwarded-For`; that header is believed from
    /// nobody else. Empty if clients reach the server directly.
    pub trusted_proxies: Vec<String>,
    /// How many failed logins in a row an address has for free at one account before it
    /// is made to wait.
    pub login_pair_free_failures: u32,
    /// The wait, in seconds, after the first failed login that is not free. It doubles
    /// with each further failure.
    pub login_pair_base_delay_seconds: u64,
    /// The longest wait of an address at one account, in seconds.
    pub login_pair_max_delay_seconds: u64,
    /// How many failed logins in a row, from anywhere, lock an account.
    pub login_account_max_failures: u32,
    /// How long an account stays locked, in seconds.
    pub login_account_lock_seconds: u64,
    /// How many logins and registrations one address may attempt per minute, one at a
    /// time. Zero for no limit by address, as for a server whose users all come from
    /// one address and no trusted proxy tells them apart.
    pub login_address_attempts_per_minute: u32,
    /// How many passwords are hashed or checked at a time. One hash takes 19 MiB and
    /// some milliseconds of a processor.
    pub password_hash_concurrency: u32,
    /// How many requests may wait for their turn to have a password hashed.
    pub password_hash_queue: u32,
    /// How long a request waits for that turn before it is told to try again, in
    /// seconds.
    pub password_hash_queue_seconds: u64,
    /// The keys `collab_*` (editing a body together).
    #[serde(flatten)]
    pub collab: collab::CollabSettings,
    /// SMTP transport settings and the limits of all mail; an empty host disables
    /// outgoing mail unless `mail_transport` names a provider.
    #[serde(flatten)]
    pub smtp: smtp::SmtpSettings,
    /// The way mail leaves (`mail_transport`) and the keys of the providers' APIs.
    #[serde(flatten)]
    pub mail: mail::MailSettings,
    /// Whether to skip importing the built-in templates at startup. Not a configuration
    /// file key; the test harness sets it so a server that does not need templates
    /// starts quickly, as the original's `SkipTemplateInit` does.
    #[serde(skip)]
    pub skip_template_init: bool,
}

impl Default for Configuration {
    fn default() -> Self {
        Self {
            server_root: "http://localhost:8000".to_owned(),
            port: 8000,
            db_type: "sqlite3".to_owned(),
            db_config: "./fvoci.db".to_owned(),
            db_ping_attempts: 5,
            server_lock_wait_seconds: 30,
            use_ssl: false,
            web_path: "./pack".to_owned(),
            files_driver: "local".to_owned(),
            files_s3_config: AmazonS3Config::default(),
            files_path: "./files".to_owned(),
            max_file_size: DEFAULT_MAX_FILE_SIZE,
            prometheus_address: String::new(),
            prometheus_token: String::new(),
            webhook_update: Vec::new(),
            allowed_origins: Vec::new(),
            session_idle_days: 14,
            session_max_days: 90,
            local_only: false,
            enable_local_mode: false,
            local_mode_socket_location: "/var/tmp/fvoci_local.socket".to_owned(),
            enable_public_shared_boards: false,
            feature_flags: BTreeMap::new(),
            teammate_name_display: "username".to_owned(),
            show_email_address: false,
            show_full_name: false,
            auth_mode: "native".to_owned(),
            oidc_allow_private_addresses: false,
            secret_key: None,
            oidc_extra_root_certificates: Vec::new(),
            org_creation: ORG_CREATION_FIRST_USER_ONLY.to_owned(),
            websocket_ping_seconds: 30,
            public_link_connections_per_board: 200,
            public_link_connections_per_address: 10,
            compress_responses: true,
            max_request_bytes: DEFAULT_MAX_REQUEST_BYTES,
            trash_retention_days: 30,
            upload_part_bytes: DEFAULT_UPLOAD_PART_BYTES,
            tmp_path: String::new(),
            max_pending_upload_bytes: DEFAULT_MAX_PENDING_UPLOAD_BYTES,
            max_import_bytes: DEFAULT_MAX_IMPORT_BYTES,
            max_comment_versions: DEFAULT_MAX_COMMENT_VERSIONS,
            max_concurrent_exports: DEFAULT_MAX_CONCURRENT_ARCHIVES,
            export_stall_seconds: 60,
            max_concurrent_imports: DEFAULT_MAX_CONCURRENT_ARCHIVES,
            trusted_proxies: Vec::new(),
            login_pair_free_failures: 5,
            login_pair_base_delay_seconds: 30,
            login_pair_max_delay_seconds: 60 * 60,
            login_account_max_failures: 100,
            login_account_lock_seconds: 60 * 60,
            login_address_attempts_per_minute: 20,
            password_hash_concurrency: 4,
            password_hash_queue: 16,
            password_hash_queue_seconds: 5,
            collab: collab::CollabSettings::default(),
            smtp: smtp::SmtpSettings::default(),
            mail: mail::MailSettings::default(),
            skip_template_init: false,
        }
    }
}

impl std::fmt::Debug for Configuration {
    /// Leaves out the database connection string, which may hold a password.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Configuration")
            .field("smtp", &self.smtp)
            .field("mail", &self.mail)
            .field("server_root", &self.server_root)
            .field("port", &self.port)
            .field("db_type", &self.db_type)
            .field("use_ssl", &self.use_ssl)
            .field("web_path", &self.web_path)
            .field("files_driver", &self.files_driver)
            .field("files_s3_config", &self.files_s3_config)
            .field("files_path", &self.files_path)
            .field("max_file_size", &self.max_file_size)
            .field("webhook_update", &self.webhook_update)
            .field("allowed_origins", &self.allowed_origins)
            .field("session_idle_days", &self.session_idle_days)
            .field("session_max_days", &self.session_max_days)
            .field("local_only", &self.local_only)
            .field("enable_local_mode", &self.enable_local_mode)
            .field(
                "local_mode_socket_location",
                &self.local_mode_socket_location,
            )
            .field(
                "enable_public_shared_boards",
                &self.enable_public_shared_boards,
            )
            .field("feature_flags", &self.feature_flags)
            .field("teammate_name_display", &self.teammate_name_display)
            .field("show_email_address", &self.show_email_address)
            .field("show_full_name", &self.show_full_name)
            .field("auth_mode", &self.auth_mode)
            .field(
                "oidc_allow_private_addresses",
                &self.oidc_allow_private_addresses,
            )
            .field("secret_key", &self.secret_key)
            .field("org_creation", &self.org_creation)
            .field("server_lock_wait_seconds", &self.server_lock_wait_seconds)
            .field("websocket_ping_seconds", &self.websocket_ping_seconds)
            .field(
                "public_link_connections_per_board",
                &self.public_link_connections_per_board,
            )
            .field(
                "public_link_connections_per_address",
                &self.public_link_connections_per_address,
            )
            .field("compress_responses", &self.compress_responses)
            .field("max_request_bytes", &self.max_request_bytes)
            .field("trash_retention_days", &self.trash_retention_days)
            .field("upload_part_bytes", &self.upload_part_bytes)
            .field("tmp_path", &self.tmp_path)
            .field("max_pending_upload_bytes", &self.max_pending_upload_bytes)
            .field("max_import_bytes", &self.max_import_bytes)
            .field("max_comment_versions", &self.max_comment_versions)
            .field("max_concurrent_exports", &self.max_concurrent_exports)
            .field("export_stall_seconds", &self.export_stall_seconds)
            .field("max_concurrent_imports", &self.max_concurrent_imports)
            .field("trusted_proxies", &self.trusted_proxies)
            .field("login_pair_free_failures", &self.login_pair_free_failures)
            .field(
                "login_pair_base_delay_seconds",
                &self.login_pair_base_delay_seconds,
            )
            .field(
                "login_pair_max_delay_seconds",
                &self.login_pair_max_delay_seconds,
            )
            .field(
                "login_account_max_failures",
                &self.login_account_max_failures,
            )
            .field(
                "login_account_lock_seconds",
                &self.login_account_lock_seconds,
            )
            .field(
                "login_address_attempts_per_minute",
                &self.login_address_attempts_per_minute,
            )
            .field("password_hash_concurrency", &self.password_hash_concurrency)
            .field("password_hash_queue", &self.password_hash_queue)
            .field(
                "password_hash_queue_seconds",
                &self.password_hash_queue_seconds,
            )
            .field("collab", &self.collab)
            .finish_non_exhaustive()
    }
}

/// How the text of an environment variable becomes a configuration value.
#[derive(Debug, Clone, Copy)]
enum EnvKind {
    Text,
    Number,
    Flag,
    /// A comma-separated list.
    List,
}

/// The configuration keys that an environment variable can override, a key of
/// `filess3config` as `filess3config.<key>`. The variable is [`env_name`] of the key, for
/// example `FVOCI_SESSION_IDLE_DAYS` and `FVOCI_FILESS3CONFIG_BUCKET`. The S3 credentials
/// have none: the S3 client reads them from the `AWS_*` variables when `accesskeyid` is
/// empty.
const ENV_KEYS: [(&str, EnvKind); 59] = [
    ("serverroot", EnvKind::Text),
    ("port", EnvKind::Number),
    ("dbtype", EnvKind::Text),
    ("dbconfig", EnvKind::Text),
    ("dbpingattempts", EnvKind::Number),
    ("server_lock_wait_seconds", EnvKind::Number),
    ("usessl", EnvKind::Flag),
    ("webpath", EnvKind::Text),
    ("filesdriver", EnvKind::Text),
    ("filespath", EnvKind::Text),
    ("filess3config.bucket", EnvKind::Text),
    ("filess3config.pathprefix", EnvKind::Text),
    ("filess3config.region", EnvKind::Text),
    ("filess3config.endpoint", EnvKind::Text),
    ("filess3config.ssl", EnvKind::Flag),
    ("filess3config.signv2", EnvKind::Flag),
    ("filess3config.sse", EnvKind::Flag),
    ("filess3config.timeout", EnvKind::Number),
    ("maxfilesize", EnvKind::Number),
    ("prometheusaddress", EnvKind::Text),
    ("prometheustoken", EnvKind::Text),
    ("webhook_update", EnvKind::List),
    ("allowed_origins", EnvKind::List),
    ("session_idle_days", EnvKind::Number),
    ("session_max_days", EnvKind::Number),
    ("localonly", EnvKind::Flag),
    ("enablelocalmode", EnvKind::Flag),
    ("localmodesocketlocation", EnvKind::Text),
    ("enablepublicsharedboards", EnvKind::Flag),
    ("teammatenamedisplay", EnvKind::Text),
    ("showemailaddress", EnvKind::Flag),
    ("showfullname", EnvKind::Flag),
    ("authmode", EnvKind::Text),
    ("oidc_allow_private_addresses", EnvKind::Flag),
    ("org_creation", EnvKind::Text),
    ("websocket_ping_seconds", EnvKind::Number),
    ("public_link_connections_per_board", EnvKind::Number),
    ("public_link_connections_per_address", EnvKind::Number),
    ("compress_responses", EnvKind::Flag),
    ("max_request_bytes", EnvKind::Number),
    ("trash_retention_days", EnvKind::Number),
    ("upload_part_bytes", EnvKind::Number),
    ("tmp_path", EnvKind::Text),
    ("max_pending_upload_bytes", EnvKind::Number),
    ("max_import_bytes", EnvKind::Number),
    ("max_comment_versions", EnvKind::Number),
    ("max_concurrent_exports", EnvKind::Number),
    ("export_stall_seconds", EnvKind::Number),
    ("max_concurrent_imports", EnvKind::Number),
    ("trusted_proxies", EnvKind::List),
    ("login_pair_free_failures", EnvKind::Number),
    ("login_pair_base_delay_seconds", EnvKind::Number),
    ("login_pair_max_delay_seconds", EnvKind::Number),
    ("login_account_max_failures", EnvKind::Number),
    ("login_account_lock_seconds", EnvKind::Number),
    ("login_address_attempts_per_minute", EnvKind::Number),
    ("password_hash_concurrency", EnvKind::Number),
    ("password_hash_queue", EnvKind::Number),
    ("password_hash_queue_seconds", EnvKind::Number),
];

const MILLIS_PER_DAY: i64 = 24 * 60 * 60 * 1000;

/// The largest file, unless the configuration says otherwise: 100 MiB (decision 16 of
/// `docs/s6-design.md`).
const DEFAULT_MAX_FILE_SIZE: i64 = 100 * 1024 * 1024;

/// What all open uploads may hold back, unless the configuration says otherwise: 2 GiB
/// (`docs/s6-design.md` 5.6, 8).
const DEFAULT_MAX_PENDING_UPLOAD_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// The largest archive an import takes unless configured (`docs/s6-design.md` 4.2).
const DEFAULT_MAX_IMPORT_BYTES: u64 = 256 * 1024 * 1024;

/// How many earlier versions of a comment the server keeps unless configured.
const DEFAULT_MAX_COMMENT_VERSIONS: u32 = fvoci_store::DEFAULT_COMMENT_VERSIONS_KEPT;

/// How many exports, and how many imports, the server runs at once unless configured.
const DEFAULT_MAX_CONCURRENT_ARCHIVES: u32 = 2;

/// The size of the parts of an upload, unless the configuration says otherwise: 8 MiB
/// (`docs/s6-design.md` 5.6). A file of 100 MiB comes in 13 parts.
const DEFAULT_UPLOAD_PART_BYTES: u64 = 8 * 1024 * 1024;

/// The smallest part an S3 multipart upload takes, and so the smallest part size.
pub const MIN_UPLOAD_PART_BYTES: u64 = 5 * 1024 * 1024;

/// The largest part an S3 multipart upload takes, and so the largest part size.
pub const MAX_UPLOAD_PART_BYTES: u64 = 5 * 1024 * 1024 * 1024;

/// Keys of the original server's configuration that this server no longer has, each with
/// what to do instead; a key of `filess3config` as `filess3config.<key>`. A file or an
/// environment that sets one to something is refused: a server that silently ignored it
/// would run differently from what its operator wrote.
const REMOVED_KEYS: [(&str, &str); 11] = [
    (
        "dbtableprefix",
        "tables have fixed names; to keep them apart from other tables in one PostgreSQL \
         database, give the server a schema of its own with search_path in dbconfig",
    ),
    (
        "trust_forwarded_for",
        "name the reverse proxies in trusted_proxies; X-Forwarded-For is believed from \
         them only",
    ),
    (
        "securecookie",
        "the session cookie is Secure exactly when serverRoot is an https address",
    ),
    (
        "session_expire_time",
        "a session ends after session_idle_days without use and session_max_days after \
         signing in, both in days",
    ),
    (
        "session_refresh_time",
        "when a session was last used is recorded at most every ten minutes; there is \
         nothing to set",
    ),
    (
        "logging_cfg_file",
        "the log is set with FVOCI_LOG_LEVEL and FVOCI_LOG_FORMAT",
    ),
    (
        "logging_cfg_json",
        "the log is set with FVOCI_LOG_LEVEL and FVOCI_LOG_FORMAT",
    ),
    ("audit_cfg_file", "there is no audit log"),
    ("audit_cfg_json", "there is no audit log"),
    (
        "filess3config.trace",
        "requests to the object store are not logged one by one",
    ),
    (
        "smtp_from",
        "the sender of all mail, whatever its way, is mail_from (FVOCI_MAIL_FROM)",
    ),
];

/// The environment variable of a key: [`ENV_PREFIX`] and the key in capitals, a dot as
/// `_`.
fn env_name(key: &str) -> String {
    format!("{ENV_PREFIX}{}", key.to_uppercase().replace('.', "_"))
}

/// The value of a key in the file's keys, a key of `filess3config` named
/// `filess3config.<key>`.
fn value_of<'a>(keys: &'a Map<String, Value>, key: &str) -> Option<&'a Value> {
    match key.split_once('.') {
        Some((outer, inner)) => keys.get(outer).and_then(|outer| outer.get(inner)),
        None => keys.get(key),
    }
}

/// Sets a key in the file's keys, a key of `filess3config` named `filess3config.<key>`
/// (which [`Configuration::parse`] has made an object).
fn set_value(keys: &mut Map<String, Value>, key: &str, value: Value) {
    match key.split_once('.') {
        Some((outer, inner)) => {
            if let Some(Value::Object(outer)) = keys.get_mut(outer) {
                outer.insert(inner.to_owned(), value);
            }
        }
        None => {
            keys.insert(key.to_owned(), value);
        }
    }
}

/// Whether a configuration value says nothing: what a removed key may still hold.
fn is_unset(value: &Value) -> bool {
    match value {
        Value::Null | Value::Bool(false) => true,
        Value::String(text) => text.is_empty(),
        Value::Number(number) => number.as_i64() == Some(0),
        Value::Array(items) => items.is_empty(),
        Value::Bool(true) | Value::Object(_) => false,
    }
}

/// `org_creation`: the server has one organization.
pub const ORG_CREATION_FIRST_USER_ONLY: &str = "first_user_only";
/// `org_creation`: the server takes in several organizations.
pub const ORG_CREATION_OPEN: &str = "open";

impl Configuration {
    /// Whether the server sends mail: `mail_transport` names a way, or `smtp_host` is
    /// set.
    pub fn mail_enabled(&self) -> bool {
        self.mail.enabled(&self.smtp)
    }

    /// Whether the server takes in several organizations (`org_creation` is `open`).
    pub fn takes_several_organizations(&self) -> bool {
        self.org_creation == ORG_CREATION_OPEN
    }

    /// Reads the configuration file at `path` and applies environment overrides from the
    /// process environment.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read, is not a JSON object, holds a value
    /// of the wrong type, or an environment override cannot be parsed.
    pub fn read(path: &Path) -> anyhow::Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading the configuration file {}", path.display()))?;
        Self::parse(&text, |name| std::env::var(name).ok())
            .with_context(|| format!("in the configuration file {}", path.display()))
    }

    /// Decodes configuration JSON, with environment variables looked up through `env`.
    ///
    /// # Errors
    ///
    /// Returns an error if `text` is not a JSON object, gives a key twice (also in
    /// letters of different cases), holds a value of the wrong type, sets a key this
    /// server no longer has, or an environment override cannot be parsed.
    pub fn parse(text: &str, env: impl Fn(&str) -> Option<String>) -> anyhow::Result<Self> {
        let Value::Object(file) =
            serde_json::from_str(text).context("the file is not valid JSON")?
        else {
            bail!("the configuration must be a JSON object");
        };
        // Which of two values of a key would win is the order of the text, or of the
        // letters: a key given twice is refused, by its name alone (its value may be a
        // secret, such as the token of the metrics).
        keys::check_each_once(text)?;
        let mut keys = lower_case_keys(file);
        match keys.remove("filess3config") {
            Some(Value::Object(s3)) => {
                keys.insert(
                    "filess3config".to_owned(),
                    Value::Object(lower_case_keys(s3)),
                );
            }
            None | Some(Value::Null) => {
                keys.insert("filess3config".to_owned(), Value::Object(Map::new()));
            }
            Some(_) => bail!("filess3config must be a JSON object"),
        }

        for (key, kind) in ENV_KEYS
            .into_iter()
            .chain(collab::ENV_KEYS)
            .chain(smtp::ENV_KEYS)
            .chain(mail::ENV_KEYS)
        {
            let name = env_name(key);
            if let Some(text) = env(&name)
                && !scrapes::is_unset_token(key, &text)
            {
                set_value(&mut keys, key, env_value(&name, &text, kind)?);
            }
        }

        for (key, instead) in REMOVED_KEYS {
            let from_env = env(&env_name(key)).is_some_and(|text| !text.trim().is_empty());
            if from_env || value_of(&keys, key).is_some_and(|value| !is_unset(value)) {
                bail!("{key} is no longer a setting: {instead}");
            }
        }
        // The S3 credentials come from the AWS_* variables only; a variable named like
        // the other S3 settings would otherwise be passed over without a word.
        for key in ["filess3config.accesskeyid", "filess3config.secretaccesskey"] {
            let name = env_name(key);
            if env(&name).is_some_and(|text| !text.trim().is_empty()) {
                bail!(
                    "{name} is not read: leave filess3config.accesskeyid empty and give the \
                     credentials as AWS_ACCESS_KEY_ID and AWS_SECRET_ACCESS_KEY"
                );
            }
        }

        scrapes::check_token_is_text(&keys)?;
        smtp::check_password_is_text(&keys)?;
        mail::check_secrets_are_text(&keys)?;
        let secret_key = secret_key::read(&keys, &env, is_unset)?;
        let mut config: Self = serde_json::from_value(Value::Object(keys))
            .context("a key has a value of the wrong type")?;
        config.secret_key = secret_key;
        Ok(config)
    }

    /// How long a session may go unused before it ends, in milliseconds.
    pub fn session_idle_millis(&self) -> i64 {
        i64::from(self.session_idle_days).saturating_mul(MILLIS_PER_DAY)
    }

    /// How long after signing in a session ends, in milliseconds.
    pub fn session_max_millis(&self) -> i64 {
        i64::from(self.session_max_days).saturating_mul(MILLIS_PER_DAY)
    }

    /// How long a deleted board or block stays in the trash, in milliseconds.
    pub fn trash_retention_millis(&self) -> i64 {
        i64::from(self.trash_retention_days).saturating_mul(MILLIS_PER_DAY)
    }

    /// The directory of the server's temporary files: `tmp_path`, or `.tmp` under
    /// `filespath`.
    pub fn tmp_dir(&self) -> std::path::PathBuf {
        if self.tmp_path.is_empty() {
            std::path::Path::new(&self.files_path).join(".tmp")
        } else {
            std::path::PathBuf::from(&self.tmp_path)
        }
    }

    /// How long an export's download may send nothing before it is given up
    /// (`export_stall_seconds`).
    pub fn export_stall(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.export_stall_seconds)
    }

    /// How long a start waits for the lock of the server (`server_lock_wait_seconds`).
    pub fn server_lock_wait(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.server_lock_wait_seconds)
    }

    /// Checks that the configuration only asks for what this server supports.
    ///
    /// Returns the warnings for settings that are accepted but have no effect.
    ///
    /// # Errors
    ///
    /// Returns an error naming the first setting this server cannot honor: an
    /// authentication mode other than `native`, a metrics address that is no `host:port`,
    /// port 0, a limit of zero, an unknown file backend, or version 2 request signing for
    /// S3.
    pub fn check(&self) -> anyhow::Result<Vec<String>> {
        if self.auth_mode != "native" {
            bail!(
                "authMode {:?} is not supported; this server only has native authentication",
                self.auth_mode
            );
        }
        if ![ORG_CREATION_FIRST_USER_ONLY, ORG_CREATION_OPEN].contains(&self.org_creation.as_str())
        {
            bail!(
                "org_creation {:?} is not known; use {ORG_CREATION_FIRST_USER_ONLY:?} or \
                 {ORG_CREATION_OPEN:?}",
                self.org_creation
            );
        }
        // SQLite holds one organization, and nothing would keep several apart. (A
        // dbtype that is none is refused where the database is opened.)
        if self.takes_several_organizations() && self.db_type == "sqlite3" {
            bail!(
                "org_creation is {ORG_CREATION_OPEN:?}, which needs dbtype \"postgres\": an \
                 SQLite database holds one organization"
            );
        }
        // Zero would listen on any free port, not on one the operator can name.
        if self.port == 0 {
            bail!("port must be 1 to 65535, not 0");
        }
        // Zero does not mean "no limit": a server that takes bodies of any size is not
        // something to end up with by leaving a number out.
        if self.max_request_bytes == 0 {
            bail!("max_request_bytes must be a positive number of bytes, not 0");
        }
        // The session cookie and the check of where a request comes from both follow
        // from this address.
        crate::auth::token::Origins::new(&self.server_root, &self.allowed_origins)
            .map_err(anyhow::Error::msg)?;
        crate::client_address::TrustedProxies::parse(&self.trusted_proxies)
            .map_err(anyhow::Error::msg)?;
        self.check_login_limits()?;
        // Zero would end every session as it starts.
        if self.session_idle_days == 0 || self.session_max_days == 0 {
            bail!("session_idle_days and session_max_days must be at least 1");
        }
        // Zero would empty the trash as it fills: deleting could not be undone.
        if self.trash_retention_days == 0 {
            bail!("trash_retention_days must be at least 1");
        }
        self.check_upload_limits()?;
        self.smtp.check()?;
        self.mail.way(&self.smtp)?;
        self.metrics_address()?;
        self.check_metrics_token()?;
        match self.files_driver.as_str() {
            "local" => {}
            "amazons3" => {
                if self.files_s3_config.sign_v2 {
                    bail!(
                        "filess3config.SignV2 is not supported; requests are signed with version 4"
                    );
                }
            }
            other => bail!("filesdriver {other:?} is not supported; use \"local\" or \"amazons3\""),
        }

        let mut warnings = Vec::new();
        // Reached over HTTPS without ending TLS itself: something in front of the server
        // does, and every request arrives from its address. Unless the server is told
        // which address that is, all clients are one client to the login limits (one
        // that guesses holds everybody up) and to the readers of a public link (one
        // address's share of a board is all a board takes).
        if self.server_root.starts_with("https://")
            && !self.use_ssl
            && self.trusted_proxies.is_empty()
        {
            warnings.push(
                "serverRoot is https but the server does not end TLS itself (usessl is off) and \
                 trusted_proxies is empty: behind a reverse proxy every client is seen at the \
                 proxy's address, and the login limits and the readers of a public link \
                 (public_link_connections_per_address) then count all clients as one; set \
                 trusted_proxies to the proxy's addresses"
                    .to_owned(),
            );
        }
        if !self.prometheus_token.is_empty() && self.prometheus_address.is_empty() {
            warnings.push(
                "prometheustoken is set but prometheusaddress is empty: no metrics are served"
                    .to_owned(),
            );
        }
        Ok(warnings)
    }

    /// Checks the settings of the login limits and of the queue of password work.
    fn check_login_limits(&self) -> anyhow::Result<()> {
        // A wait of nothing is no wait, an account that locks at no failure is always
        // locked, and nobody's password is checked by a queue without a place.
        for (name, value) in [
            (
                "login_pair_base_delay_seconds",
                self.login_pair_base_delay_seconds,
            ),
            (
                "login_pair_max_delay_seconds",
                self.login_pair_max_delay_seconds,
            ),
            (
                "login_account_lock_seconds",
                self.login_account_lock_seconds,
            ),
            (
                "login_account_max_failures",
                u64::from(self.login_account_max_failures),
            ),
            (
                "password_hash_concurrency",
                u64::from(self.password_hash_concurrency),
            ),
            (
                "password_hash_queue_seconds",
                self.password_hash_queue_seconds,
            ),
        ] {
            if value == 0 {
                bail!("{name} must be at least 1");
            }
        }
        // A wait is a time on the clock: one that is longer than this means "for ever",
        // and that is not what a number of seconds should be made to say.
        let longest = crate::auth::limiter::LONGEST_WAIT.as_secs();
        for (name, seconds) in [
            (
                "login_pair_base_delay_seconds",
                self.login_pair_base_delay_seconds,
            ),
            (
                "login_pair_max_delay_seconds",
                self.login_pair_max_delay_seconds,
            ),
            (
                "login_account_lock_seconds",
                self.login_account_lock_seconds,
            ),
        ] {
            if seconds > longest {
                bail!("{name} must not be more than {longest} (ten years)");
            }
        }
        if self.login_pair_max_delay_seconds < self.login_pair_base_delay_seconds {
            bail!(
                "login_pair_max_delay_seconds must not be less than login_pair_base_delay_seconds"
            );
        }
        Ok(())
    }
}

/// The keys of `map` in small letters; [`keys::check_each_once`] has seen that no two
/// become one.
fn lower_case_keys(map: Map<String, Value>) -> Map<String, Value> {
    map.into_iter()
        .map(|(key, value)| (key.to_lowercase(), value))
        .collect()
}

fn env_value(name: &str, text: &str, kind: EnvKind) -> anyhow::Result<Value> {
    Ok(match kind {
        EnvKind::Text => Value::from(text),
        EnvKind::Number => {
            let number: i64 = text
                .trim()
                .parse()
                .with_context(|| format!("{name}={text:?} is not a whole number"))?;
            Value::from(number)
        }
        EnvKind::Flag => match text.trim().to_lowercase().as_str() {
            "true" | "1" | "t" | "yes" | "on" => Value::Bool(true),
            "false" | "0" | "f" | "no" | "off" => Value::Bool(false),
            _ => bail!("{name}={text:?} is not true or false"),
        },
        EnvKind::List => Value::from(
            text.split(',')
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .collect::<Vec<&str>>(),
        ),
    })
}

#[cfg(test)]
mod tests;
