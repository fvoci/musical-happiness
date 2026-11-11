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
    /// The address of the metrics endpoint. Not supported; must be empty.
    #[serde(rename = "prometheusaddress")]
    pub prometheus_address: String,
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
    /// The authentication mode. Only `native` is supported.
    #[serde(rename = "authmode")]
    pub auth_mode: String,
    /// How often the server pings each websocket, in seconds; zero to never ping. Keeps
    /// proxies and CDNs from closing idle connections.
    pub websocket_ping_seconds: u64,
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
            use_ssl: false,
            web_path: "./pack".to_owned(),
            files_driver: "local".to_owned(),
            files_s3_config: AmazonS3Config::default(),
            files_path: "./files".to_owned(),
            max_file_size: DEFAULT_MAX_FILE_SIZE,
            prometheus_address: String::new(),
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
            websocket_ping_seconds: 30,
            compress_responses: true,
            max_request_bytes: DEFAULT_MAX_REQUEST_BYTES,
            trash_retention_days: 30,
            upload_part_bytes: DEFAULT_UPLOAD_PART_BYTES,
            tmp_path: String::new(),
            max_pending_upload_bytes: DEFAULT_MAX_PENDING_UPLOAD_BYTES,
            max_import_bytes: DEFAULT_MAX_IMPORT_BYTES,
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
            skip_template_init: false,
        }
    }
}

impl std::fmt::Debug for Configuration {
    /// Leaves out the database connection string, which may hold a password.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Configuration")
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
            .field("websocket_ping_seconds", &self.websocket_ping_seconds)
            .field("compress_responses", &self.compress_responses)
            .field("max_request_bytes", &self.max_request_bytes)
            .field("trash_retention_days", &self.trash_retention_days)
            .field("upload_part_bytes", &self.upload_part_bytes)
            .field("tmp_path", &self.tmp_path)
            .field("max_pending_upload_bytes", &self.max_pending_upload_bytes)
            .field("max_import_bytes", &self.max_import_bytes)
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
const ENV_KEYS: [(&str, EnvKind); 49] = [
    ("serverroot", EnvKind::Text),
    ("port", EnvKind::Number),
    ("dbtype", EnvKind::Text),
    ("dbconfig", EnvKind::Text),
    ("dbpingattempts", EnvKind::Number),
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
    ("websocket_ping_seconds", EnvKind::Number),
    ("compress_responses", EnvKind::Flag),
    ("max_request_bytes", EnvKind::Number),
    ("trash_retention_days", EnvKind::Number),
    ("upload_part_bytes", EnvKind::Number),
    ("tmp_path", EnvKind::Text),
    ("max_pending_upload_bytes", EnvKind::Number),
    ("max_import_bytes", EnvKind::Number),
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
const REMOVED_KEYS: [(&str, &str); 10] = [
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

impl Configuration {
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
    /// Returns an error if `text` is not a JSON object, holds a value of the wrong type,
    /// sets a key this server no longer has, or an environment override cannot be parsed.
    pub fn parse(text: &str, env: impl Fn(&str) -> Option<String>) -> anyhow::Result<Self> {
        let Value::Object(file) =
            serde_json::from_str(text).context("the file is not valid JSON")?
        else {
            bail!("the configuration must be a JSON object");
        };
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

        for (key, kind) in ENV_KEYS {
            let name = env_name(key);
            if let Some(text) = env(&name) {
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

        serde_json::from_value(Value::Object(keys)).context("a key has a value of the wrong type")
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

    /// Checks the limits on uploads: none turns a limit off, and they fit together.
    fn check_upload_limits(&self) -> anyhow::Result<()> {
        // An S3 multipart upload takes no smaller part but the last, and none larger.
        if !(MIN_UPLOAD_PART_BYTES..=MAX_UPLOAD_PART_BYTES).contains(&self.upload_part_bytes) {
            bail!(
                "upload_part_bytes must be {MIN_UPLOAD_PART_BYTES} (5 MiB) to \
                 {MAX_UPLOAD_PART_BYTES} (5 GiB)"
            );
        }
        // Zero once meant "no limit"; a server without one is not something to end up
        // with by leaving a number at what it was (`docs/s6-design.md` 4.2).
        let largest = self
            .upload_part_bytes
            .saturating_mul(u64::try_from(fvoci_model::MAX_UPLOAD_PARTS).unwrap_or(0));
        if u64::try_from(self.max_file_size).map_or(true, |size| size == 0 || size > largest) {
            bail!(
                "maxfilesize must be 1 to {largest} bytes (upload_part_bytes times 10,000 \
                 parts), not {}",
                self.max_file_size
            );
        }
        // Less would refuse the largest file with 429 every time, as if it might fit later.
        // Positive, as checked above.
        let max_file_size = u64::try_from(self.max_file_size).unwrap_or(0);
        if self.max_pending_upload_bytes < max_file_size {
            bail!(
                "max_pending_upload_bytes must be at least maxfilesize ({max_file_size}), \
                 not {}",
                self.max_pending_upload_bytes
            );
        }
        // An archive comes as an upload in parts too.
        if self.max_import_bytes == 0 || self.max_import_bytes > largest {
            bail!(
                "max_import_bytes must be 1 to {largest} bytes (upload_part_bytes times \
                 10,000 parts), not {}",
                self.max_import_bytes
            );
        }
        if self.max_pending_upload_bytes < self.max_import_bytes {
            bail!(
                "max_pending_upload_bytes must be at least max_import_bytes ({}), not {}",
                self.max_import_bytes,
                self.max_pending_upload_bytes
            );
        }
        Ok(())
    }

    /// Checks that the configuration only asks for what this server supports.
    ///
    /// Returns the warnings for settings that are accepted but have no effect.
    ///
    /// # Errors
    ///
    /// Returns an error naming the first setting this server cannot honor: an
    /// authentication mode other than `native`, a metrics address, port 0, a limit of
    /// zero, an unknown file backend, or version 2 request signing for S3.
    pub fn check(&self) -> anyhow::Result<Vec<String>> {
        if self.auth_mode != "native" {
            bail!(
                "authMode {:?} is not supported; this server only has native authentication",
                self.auth_mode
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
        if !self.prometheus_address.is_empty() {
            bail!("prometheusaddress is set, but this server has no metrics endpoint");
        }
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
        // which address that is, all clients are one client to the login limits: one
        // that guesses holds everybody up.
        if self.server_root.starts_with("https://")
            && !self.use_ssl
            && self.trusted_proxies.is_empty()
        {
            warnings.push(
                "serverRoot is https but the server does not end TLS itself (usessl is off) and \
                 trusted_proxies is empty: behind a reverse proxy every client is seen at the \
                 proxy's address, and the login limits then count all clients as one; set \
                 trusted_proxies to the proxy's addresses"
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
mod tests {
    use super::*;

    fn no_env(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn empty_object_gives_the_defaults() {
        let config = Configuration::parse("{}", no_env).expect("an empty object is valid");
        assert_eq!(config.server_root, "http://localhost:8000");
        assert_eq!(config.port, 8000);
        assert_eq!(config.db_type, "sqlite3");
        assert_eq!(config.db_config, "./fvoci.db");
        assert_eq!(config.web_path, "./pack");
        assert_eq!(config.files_driver, "local");
        assert_eq!(config.files_path, "./files");
        assert_eq!(config.session_idle_days, 14);
        assert_eq!(config.session_max_days, 90);
        assert_eq!(config.session_idle_millis(), 14 * 24 * 60 * 60 * 1000);
        assert_eq!(config.session_max_millis(), 90 * 24 * 60 * 60 * 1000);
        assert_eq!(config.trash_retention_days, 30);
        assert_eq!(config.upload_part_bytes, 8 * 1024 * 1024);
        assert_eq!(config.max_file_size, 100 * 1024 * 1024);
        assert_eq!(config.max_pending_upload_bytes, 2 * 1024 * 1024 * 1024);
        assert_eq!(config.max_import_bytes, 256 * 1024 * 1024);
        assert_eq!(config.tmp_dir(), std::path::Path::new("./files/.tmp"));
        assert_eq!(config.auth_mode, "native");
        assert_eq!(config.teammate_name_display, "username");
        assert_eq!(
            config.local_mode_socket_location,
            "/var/tmp/fvoci_local.socket"
        );
        assert_eq!(config.websocket_ping_seconds, 30);
        assert!(
            config.compress_responses,
            "API answers are compressed by default"
        );
        let warnings = config.check().expect("the defaults are supported");
        assert_eq!(
            warnings,
            Vec::<String>::new(),
            "the defaults cause no warning"
        );
    }

    #[test]
    fn a_port_of_zero_and_s3_settings_that_are_no_object_are_refused() {
        let config = Configuration::parse(r#"{"port": 0}"#, no_env).expect("read");
        let error = config.check().expect_err("port 0 is refused");
        assert!(error.to_string().contains("port"), "{error}");
        let Err(error) = Configuration::parse(r#"{"filesS3Config": "x"}"#, no_env) else {
            panic!("a string is no S3 settings");
        };
        assert!(error.to_string().contains("filess3config"), "{error}");
        Configuration::parse(r#"{"filesS3Config": null}"#, no_env).expect("null is no settings");
    }

    /// A file written for the original server, with its unknown keys, but for the
    /// settings this server replaced: those are refused, see
    /// `a_removed_key_is_refused_when_it_is_set_to_something`.
    #[test]
    fn reads_a_configuration_file_of_the_original() {
        let text = r#"{
            "serverRoot": "http://localhost:8000",
            "port": 8000,
            "dbtype": "sqlite3",
            "dbconfig": "./boards.db?_busy_timeout=5000",
            "postgres_dbconfig": "dbname=boards sslmode=disable",
            "useSSL": false,
            "webpath": "./webapp/pack",
            "filespath": "./files",
            "telemetry": true,
            "prometheusaddress": "",
            "localOnly": false,
            "enableLocalMode": true,
            "localModeSocketLocation": "/var/tmp/boards_local.socket",
            "authMode": "native",
            "enablePublicSharedBoards": false
        }"#;
        let config = Configuration::parse(text, no_env).expect("the original file is valid");
        assert_eq!(config.db_config, "./boards.db?_busy_timeout=5000");
        assert_eq!(config.web_path, "./webapp/pack");
        assert!(config.enable_local_mode, "enableLocalMode is read");
        assert_eq!(
            config.local_mode_socket_location,
            "/var/tmp/boards_local.socket"
        );
    }

    #[test]
    fn keys_match_without_regard_to_case() {
        let text = r#"{"PORT": 8088, "LocalOnly": true, "filesS3Config": {"Bucket": "b", "SSL": true, "AccessKeyID": "k"}}"#;
        let config = Configuration::parse(text, no_env).expect("valid");
        assert_eq!(config.port, 8088);
        assert!(config.local_only, "localOnly is read whatever its case");
        assert_eq!(config.files_s3_config.bucket, "b");
        assert_eq!(config.files_s3_config.access_key_id, "k");
        assert!(
            config.files_s3_config.ssl,
            "nested keys are case-insensitive too"
        );
    }

    #[test]
    fn environment_overrides_the_file_and_the_defaults() {
        let env = |name: &str| match name {
            "FVOCI_PORT" => Some("9001".to_owned()),
            "FVOCI_DBCONFIG" => Some("postgres://u@h/db".to_owned()),
            "FVOCI_ENABLEPUBLICSHAREDBOARDS" => Some("true".to_owned()),
            "FVOCI_WEBHOOK_UPDATE" => Some("http://a/hook, http://b/hook".to_owned()),
            "FVOCI_SESSION_IDLE_DAYS" => Some("3".to_owned()),
            _ => None,
        };
        let config = Configuration::parse(r#"{"port": 8000}"#, env).expect("valid");
        assert_eq!(config.port, 9001);
        assert_eq!(config.db_config, "postgres://u@h/db");
        assert!(
            config.enable_public_shared_boards,
            "a flag is read from the environment"
        );
        assert_eq!(config.webhook_update, ["http://a/hook", "http://b/hook"]);
        assert_eq!(config.session_idle_days, 3);
    }

    /// The settings of S3 that differ between deployments come from the environment too
    /// (user decision 2026-11-11 11:05 KST); the credentials do not.
    #[test]
    fn the_environment_overrides_the_s3_settings_but_not_the_credentials() {
        let env = |name: &str| match name {
            "FVOCI_FILESS3CONFIG_BUCKET" => Some("from-env".to_owned()),
            "FVOCI_FILESS3CONFIG_SSL" => Some("on".to_owned()),
            "FVOCI_FILESS3CONFIG_TIMEOUT" => Some("5000".to_owned()),
            "FVOCI_FILESS3CONFIG_SIGNV2" => Some("true".to_owned()),
            _ => None,
        };
        let file = r#"{"FilesS3Config": {"Bucket": "from-file", "Region": "eu-west-1"}}"#;
        for text in [file, "{}", r#"{"filess3config": null}"#] {
            let s3 = Configuration::parse(text, env)
                .expect("valid")
                .files_s3_config;
            assert_eq!(s3.bucket, "from-env", "{text}");
            assert!(s3.ssl, "{text}");
            assert_eq!(s3.timeout, 5000, "{text}");
        }
        let s3 = Configuration::parse(file, env)
            .expect("valid")
            .files_s3_config;
        assert_eq!(
            s3.region, "eu-west-1",
            "the keys the environment leaves stay"
        );
        let bad = |name: &str| (name == "FVOCI_FILESS3CONFIG_SSE").then(|| "maybe".to_owned());
        assert!(
            Configuration::parse("{}", bad).is_err(),
            "an unreadable flag"
        );
        let amazon = Configuration::parse(r#"{"filesdriver": "amazons3"}"#, env).expect("valid");
        assert!(
            amazon.check().is_err(),
            "the signing scheme from the environment is checked"
        );
        for name in [
            "FVOCI_FILESS3CONFIG_ACCESSKEYID",
            "FVOCI_FILESS3CONFIG_SECRETACCESSKEY",
        ] {
            let credentials = |asked: &str| (asked == name).then(|| "key".to_owned());
            let error = Configuration::parse("{}", credentials).expect_err("not read, refused");
            assert!(error.to_string().contains("AWS_ACCESS_KEY_ID"), "{name}");
        }
        let blank =
            |asked: &str| (asked == "FVOCI_FILESS3CONFIG_SECRETACCESSKEY").then(String::new);
        Configuration::parse("{}", blank).expect("a blank credential variable is not in the way");
    }

    #[test]
    fn bad_input_is_an_error() {
        assert!(Configuration::parse("[]", no_env).is_err(), "not an object");
        assert!(Configuration::parse("{", no_env).is_err(), "not JSON");
        assert!(
            Configuration::parse(r#"{"port": "eight"}"#, no_env).is_err(),
            "wrong type"
        );
        let env = |name: &str| (name == "FVOCI_PORT").then(|| "eight".to_owned());
        assert!(
            Configuration::parse("{}", env).is_err(),
            "unparsable override"
        );
    }

    #[test]
    fn a_removed_key_is_refused_when_it_is_set_to_something() {
        let error = |text: &str| {
            Configuration::parse(text, no_env)
                .expect_err("a removed key that says something is refused")
                .to_string()
        };
        assert!(error(r#"{"dbtableprefix": "fb_"}"#).contains("search_path"));
        assert!(error(r#"{"DBTablePrefix": "fb_"}"#).contains("dbtableprefix"));
        let env = |name: &str| (name == "FVOCI_DBTABLEPREFIX").then(|| "fb_".to_owned());
        assert!(
            Configuration::parse("{}", env).is_err(),
            "the environment variable of a removed key is refused too"
        );
        // The session settings of the original are seconds with another meaning; a file
        // that sets them is told what there is instead.
        assert!(error(r#"{"session_expire_time": 2592000}"#).contains("session_idle_days"));
        assert!(error(r#"{"session_refresh_time": 18000}"#).contains("ten minutes"));
        // The settings that did nothing (user decision 2026-11-11 11:05 KST).
        assert!(error(r#"{"logging_cfg_json": "{}"}"#).contains("FVOCI_LOG_LEVEL"));
        assert!(error(r#"{"audit_cfg_file": "a.json"}"#).contains("audit"));
        assert!(error(r#"{"FilesS3Config": {"Trace": true}}"#).contains("filess3config.trace"));
        let env = |name: &str| (name == "FVOCI_FILESS3CONFIG_TRACE").then(|| "false".to_owned());
        let error = Configuration::parse("{}", env).expect_err("its variable, even false");
        assert!(error.to_string().contains("filess3config.trace"));
        // A file written for the original server usually carries the key, empty.
        Configuration::parse(
            r#"{"dbtableprefix": "", "logging_cfg_file": "", "audit_cfg_json": "",
                "filess3config": {"Trace": false}}"#,
            no_env,
        )
        .expect("a removed key that says nothing is not in the way");
    }

    #[test]
    fn check_rejects_what_the_server_cannot_honor() {
        let parse = |text: &str| Configuration::parse(text, no_env).expect("valid JSON");
        let error = |text: &str| {
            parse(text)
                .check()
                .expect_err("the setting is not supported")
                .to_string()
        };
        assert!(error(r#"{"authMode": "other"}"#).contains("authMode"));
        assert!(error(r#"{"prometheusaddress": ":9092"}"#).contains("prometheusaddress"));
        assert!(error(r#"{"filesdriver": "ftp"}"#).contains("filesdriver"));
        // Zero would not mean "no limit" to anyone who reads it as "nothing".
        assert!(error(r#"{"max_request_bytes": 0}"#).contains("max_request_bytes"));
        assert!(error(r#"{"trash_retention_days": 0}"#).contains("trash_retention_days"));
        assert!(error(r#"{"upload_part_bytes": 5242879}"#).contains("upload_part_bytes"));
        assert!(error(r#"{"upload_part_bytes": 5368709121}"#).contains("upload_part_bytes"));
        assert!(error(r#"{"max_pending_upload_bytes": 0}"#).contains("max_pending_upload_bytes"));
        for size in ["0", "-1", "83886080001"] {
            assert!(
                error(&format!(r#"{{"maxfilesize": {size}}}"#)).contains("maxfilesize"),
                "{size}"
            );
        }
        assert!(
            error(r#"{"maxfilesize": 2000, "max_pending_upload_bytes": 1999}"#)
                .contains("max_pending_upload_bytes")
        );
        for size in ["0", "83886080001"] {
            assert!(
                error(&format!(r#"{{"max_import_bytes": {size}}}"#)).contains("max_import_bytes"),
                "{size}"
            );
        }
        assert!(
            error(r#"{"maxfilesize": 2000, "max_import_bytes": 3000, "max_pending_upload_bytes": 2500}"#)
                .contains("max_import_bytes")
        );
        assert!(error(r#"{"session_idle_days": 0}"#).contains("session_idle_days"));
        assert!(error(r#"{"session_max_days": 0}"#).contains("session_max_days"));
        assert!(
            error(r#"{"filesdriver": "amazons3", "filess3config": {"SignV2": true}}"#)
                .contains("SignV2")
        );
    }

    #[test]
    fn a_wait_is_at_least_a_second_and_at_most_ten_years() {
        let error = |text: &str| {
            Configuration::parse(text, no_env)
                .expect("valid JSON")
                .check()
                .expect_err("the wait is refused")
                .to_string()
        };
        for key in [
            "login_pair_base_delay_seconds",
            "login_pair_max_delay_seconds",
            "login_account_lock_seconds",
        ] {
            assert!(error(&format!(r#"{{"{key}": 0}}"#)).contains(key));
            // More than the clock can be moved ahead by: the limiter would have
            // panicked on the first failure it counted.
            let message = error(&format!(
                r#"{{"login_pair_max_delay_seconds": {max}, "{key}": {max}}}"#,
                max = u64::MAX
            ));
            assert!(message.contains("ten years"), "{key}: {message}");
        }
    }

    #[test]
    fn https_behind_a_proxy_nobody_named_is_warned_about() {
        let warnings = |text: &str| {
            Configuration::parse(text, no_env)
                .expect("valid JSON")
                .check()
                .expect("accepted")
        };
        let behind = warnings(r#"{"serverRoot": "https://boards.example.com"}"#);
        assert_eq!(behind.len(), 1, "{behind:?}");
        assert!(behind[0].contains("trusted_proxies"), "{behind:?}");
        for quiet in [
            r#"{"serverRoot": "https://boards.example.com", "trusted_proxies": ["10.0.0.0/8"]}"#,
            r#"{"serverRoot": "https://boards.example.com", "usessl": true}"#,
            r#"{"serverRoot": "http://localhost:8000"}"#,
        ] {
            assert!(warnings(quiet).is_empty(), "{quiet}");
        }
    }

    #[test]
    fn the_request_limit_defaults_to_four_mebibytes_and_can_be_set() {
        let parse = |text: &str| Configuration::parse(text, no_env).expect("valid JSON");
        assert_eq!(
            parse("{}").max_request_bytes,
            4 * 1024 * 1024,
            "the default"
        );
        assert_eq!(
            parse(r#"{"max_request_bytes": 1000}"#).max_request_bytes,
            1000,
            "from the file"
        );
        let from_env = |key: &str| (key == "FVOCI_MAX_REQUEST_BYTES").then(|| "2000".to_owned());
        assert_eq!(
            Configuration::parse("{}", from_env)
                .expect("valid")
                .max_request_bytes,
            2000,
            "from the environment"
        );
    }

    #[test]
    fn debug_output_hides_secrets() {
        let text = r#"{"dbconfig": "postgres://u:hunter2@h/db", "filess3config": {"SecretAccessKey": "s3cr3t"}}"#;
        let debug = format!("{:?}", Configuration::parse(text, no_env).expect("valid"));
        assert!(
            !debug.contains("hunter2") && !debug.contains("s3cr3t"),
            "{debug}"
        );
    }

    /// A file of the repository, read whole.
    fn repository_file(path: &str) -> String {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(path);
        std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("reading {}: {error}", path.display()))
    }

    /// What `docs/configuration.md` says about the keys.
    #[derive(Debug, Default)]
    struct Documented {
        /// The default of each key, lower-cased, as JSON.
        defaults: BTreeMap<String, Value>,
        /// The environment variable of each key that has one.
        env: BTreeMap<String, String>,
        /// The removed keys.
        removed: Vec<String>,
    }

    /// The text of a table cell between backticks, if it is all in backticks.
    fn code(cell: &str) -> Option<&str> {
        cell.trim().strip_prefix('`')?.strip_suffix('`')
    }

    /// Reads the tables of the document: the rows of the sections "키: …" are keys, and
    /// those of "없앤 키" are removed keys.
    fn documented(text: &str) -> Documented {
        let mut documented = Documented::default();
        let mut section = "";
        for line in text.lines() {
            if let Some(heading) = line.strip_prefix("## ") {
                section = heading;
                continue;
            }
            let Some(row) = line.strip_prefix('|') else {
                continue;
            };
            let cells: Vec<&str> = row.split('|').collect();
            let Some(key) = cells.first().and_then(|cell| code(cell)) else {
                continue;
            };
            let key = key.to_lowercase();
            if section.starts_with("키:") {
                assert!(cells.len() >= 5, "a row of five cells: {line}");
                let default = code(cells[3]).unwrap_or_else(|| panic!("a default: {line}"));
                let default: Value = serde_json::from_str(default)
                    .unwrap_or_else(|error| panic!("the default of {key} is JSON ({error})"));
                if let Some(env) = code(cells[1]) {
                    documented.env.insert(key.clone(), env.to_owned());
                }
                assert!(
                    documented.defaults.insert(key.clone(), default).is_none(),
                    "{key} is documented once"
                );
            } else if section == "없앤 키" {
                documented.removed.push(key);
            }
        }
        documented
    }

    /// The keys of the configuration and their defaults, the keys of `filess3config`
    /// as `filess3config.<key>`.
    fn code_defaults() -> BTreeMap<String, Value> {
        let Value::Object(keys) =
            serde_json::to_value(Configuration::default()).expect("the defaults serialize")
        else {
            panic!("the configuration is an object");
        };
        let mut defaults = BTreeMap::new();
        for (key, value) in keys {
            match value {
                Value::Object(inner) if key == "filess3config" => {
                    for (inner_key, inner_value) in inner {
                        defaults.insert(format!("{key}.{inner_key}"), inner_value);
                    }
                }
                other => {
                    defaults.insert(key, other);
                }
            }
        }
        defaults
    }

    #[test]
    fn the_configuration_document_lists_every_key() {
        let documented = documented(&repository_file("docs/configuration.md"));
        let defaults = code_defaults();
        assert_eq!(
            documented.defaults.keys().collect::<Vec<_>>(),
            defaults.keys().collect::<Vec<_>>(),
            "docs/configuration.md lists the keys the server reads"
        );
        for (key, default) in &defaults {
            assert_eq!(
                &documented.defaults[key], default,
                "the default of {key} in docs/configuration.md"
            );
        }
        let env: BTreeMap<String, String> = ENV_KEYS
            .iter()
            .map(|(key, _)| ((*key).to_owned(), env_name(key)))
            .collect();
        assert_eq!(
            documented.env, env,
            "docs/configuration.md names the environment variables the server reads"
        );
        let removed: Vec<String> = REMOVED_KEYS
            .iter()
            .map(|(key, _)| (*key).to_owned())
            .collect();
        assert_eq!(
            documented.removed, removed,
            "docs/configuration.md lists the removed keys"
        );
    }

    #[test]
    fn the_example_configuration_files_are_read() {
        // The configuration files of the repository: what is copied into the image.
        let examples = ["docker/fvoci/config.json"];
        for path in examples {
            let config = Configuration::parse(&repository_file(path), no_env)
                .unwrap_or_else(|error| panic!("{path} is read: {error:#}"));
            let warnings = config
                .check()
                .unwrap_or_else(|error| panic!("{path} is accepted: {error:#}"));
            assert!(warnings.is_empty(), "{path} gives no warning: {warnings:?}");
        }
    }
}
