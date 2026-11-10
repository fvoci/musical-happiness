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
#[serde(default)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "each flag is a separate key of the configuration file"
)]
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
    /// Whether to log every request to the object store.
    pub trace: bool,
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
            .field("trace", &self.trace)
            .field("timeout", &self.timeout)
            .finish_non_exhaustive()
    }
}

/// The server configuration.
///
/// The serde names are the keys of the configuration file in lower case; [`Configuration::read`]
/// lower-cases the file's keys before decoding.
#[derive(Clone, Deserialize)]
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
    /// A prefix put before every table name.
    #[serde(rename = "dbtableprefix")]
    pub db_table_prefix: String,
    /// Whether to serve HTTPS, with `./cert/cert.pem` and `./cert/key.pem`.
    #[serde(rename = "usessl")]
    pub use_ssl: bool,
    /// Accepted for compatibility; the server sets no cookie.
    #[serde(rename = "securecookie")]
    pub secure_cookie: bool,
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
    /// The largest accepted upload in bytes; zero for no limit.
    #[serde(rename = "maxfilesize")]
    pub max_file_size: i64,
    /// The address of the metrics endpoint. Not supported; must be empty.
    #[serde(rename = "prometheusaddress")]
    pub prometheus_address: String,
    /// URLs that receive a POST with every changed block.
    pub webhook_update: Vec<String>,
    /// How long an unused session stays valid, in seconds.
    pub session_expire_time: i64,
    /// How old a session's last refresh may be before it is refreshed again, in seconds.
    pub session_refresh_time: i64,
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
    /// A logging configuration file of the original server. Not supported.
    pub logging_cfg_file: String,
    /// A logging configuration of the original server. Not supported.
    pub logging_cfg_json: String,
    /// An audit configuration file of the original server. Not supported.
    pub audit_cfg_file: String,
    /// An audit configuration of the original server. Not supported.
    pub audit_cfg_json: String,
    /// How often the server pings each websocket, in seconds; zero to never ping. Keeps
    /// proxies and CDNs from closing idle connections.
    pub websocket_ping_seconds: u64,
    /// The largest body, in bytes, of a request to an API route that takes JSON. A
    /// larger one is refused with 413. It does not apply to file uploads, which are
    /// bounded by `maxfilesize` (and not at all at its default of 0), nor to archive
    /// imports, which have no limit on the request yet.
    pub max_request_bytes: u64,
    /// Whether to take the client address for logs from `X-Forwarded-For`. Turn on only
    /// behind a proxy that sets the header.
    pub trust_forwarded_for: bool,
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
            db_table_prefix: String::new(),
            use_ssl: false,
            secure_cookie: false,
            web_path: "./pack".to_owned(),
            files_driver: "local".to_owned(),
            files_s3_config: AmazonS3Config::default(),
            files_path: "./files".to_owned(),
            max_file_size: 0,
            prometheus_address: String::new(),
            webhook_update: Vec::new(),
            session_expire_time: 60 * 60 * 24 * 30,
            session_refresh_time: 60 * 60 * 5,
            local_only: false,
            enable_local_mode: false,
            local_mode_socket_location: "/var/tmp/fvoci_local.socket".to_owned(),
            enable_public_shared_boards: false,
            feature_flags: BTreeMap::new(),
            teammate_name_display: "username".to_owned(),
            show_email_address: false,
            show_full_name: false,
            auth_mode: "native".to_owned(),
            logging_cfg_file: String::new(),
            logging_cfg_json: String::new(),
            audit_cfg_file: String::new(),
            audit_cfg_json: String::new(),
            websocket_ping_seconds: 30,
            max_request_bytes: DEFAULT_MAX_REQUEST_BYTES,
            trust_forwarded_for: false,
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
            .field("db_table_prefix", &self.db_table_prefix)
            .field("use_ssl", &self.use_ssl)
            .field("web_path", &self.web_path)
            .field("files_driver", &self.files_driver)
            .field("files_s3_config", &self.files_s3_config)
            .field("files_path", &self.files_path)
            .field("max_file_size", &self.max_file_size)
            .field("webhook_update", &self.webhook_update)
            .field("session_expire_time", &self.session_expire_time)
            .field("session_refresh_time", &self.session_refresh_time)
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
            .field("max_request_bytes", &self.max_request_bytes)
            .field("trust_forwarded_for", &self.trust_forwarded_for)
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

/// The configuration keys that an environment variable can override. The variable is the
/// key in upper case after [`ENV_PREFIX`], for example `FVOCI_SESSION_EXPIRE_TIME`.
const ENV_KEYS: [(&str, EnvKind); 31] = [
    ("serverroot", EnvKind::Text),
    ("port", EnvKind::Number),
    ("dbtype", EnvKind::Text),
    ("dbconfig", EnvKind::Text),
    ("dbpingattempts", EnvKind::Number),
    ("dbtableprefix", EnvKind::Text),
    ("usessl", EnvKind::Flag),
    ("securecookie", EnvKind::Flag),
    ("webpath", EnvKind::Text),
    ("filesdriver", EnvKind::Text),
    ("filespath", EnvKind::Text),
    ("maxfilesize", EnvKind::Number),
    ("prometheusaddress", EnvKind::Text),
    ("webhook_update", EnvKind::List),
    ("session_expire_time", EnvKind::Number),
    ("session_refresh_time", EnvKind::Number),
    ("localonly", EnvKind::Flag),
    ("enablelocalmode", EnvKind::Flag),
    ("localmodesocketlocation", EnvKind::Text),
    ("enablepublicsharedboards", EnvKind::Flag),
    ("teammatenamedisplay", EnvKind::Text),
    ("showemailaddress", EnvKind::Flag),
    ("showfullname", EnvKind::Flag),
    ("authmode", EnvKind::Text),
    ("logging_cfg_file", EnvKind::Text),
    ("logging_cfg_json", EnvKind::Text),
    ("audit_cfg_file", EnvKind::Text),
    ("audit_cfg_json", EnvKind::Text),
    ("websocket_ping_seconds", EnvKind::Number),
    ("max_request_bytes", EnvKind::Number),
    ("trust_forwarded_for", EnvKind::Flag),
];

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
    /// or an environment override cannot be parsed.
    pub fn parse(text: &str, env: impl Fn(&str) -> Option<String>) -> anyhow::Result<Self> {
        let Value::Object(file) =
            serde_json::from_str(text).context("the file is not valid JSON")?
        else {
            bail!("the configuration must be a JSON object");
        };
        let mut keys = lower_case_keys(file);
        if let Some(Value::Object(s3)) = keys.remove("filess3config") {
            keys.insert(
                "filess3config".to_owned(),
                Value::Object(lower_case_keys(s3)),
            );
        }

        for (key, kind) in ENV_KEYS {
            let name = format!("{ENV_PREFIX}{}", key.to_uppercase());
            if let Some(text) = env(&name) {
                keys.insert(key.to_owned(), env_value(&name, &text, kind)?);
            }
        }

        serde_json::from_value(Value::Object(keys)).context("a key has a value of the wrong type")
    }

    /// Checks that the configuration only asks for what this server supports.
    ///
    /// Returns the warnings for settings that are accepted but have no effect.
    ///
    /// # Errors
    ///
    /// Returns an error naming the first setting this server cannot honor: an
    /// authentication mode other than `native`, a metrics address, an unknown file
    /// backend, or version 2 request signing for S3.
    pub fn check(&self) -> anyhow::Result<Vec<String>> {
        if self.auth_mode != "native" {
            bail!(
                "authMode {:?} is not supported; this server only has native authentication",
                self.auth_mode
            );
        }
        // Zero does not mean "no limit": a server that takes bodies of any size is not
        // something to end up with by leaving a number out.
        if self.max_request_bytes == 0 {
            bail!("max_request_bytes must be a positive number of bytes, not 0");
        }
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

        let ignored = [
            ("logging_cfg_file", &self.logging_cfg_file),
            ("logging_cfg_json", &self.logging_cfg_json),
            ("audit_cfg_file", &self.audit_cfg_file),
            ("audit_cfg_json", &self.audit_cfg_json),
        ];
        Ok(ignored
            .into_iter()
            .filter(|(_, value)| !value.is_empty())
            .map(|(key, _)| format!("{key} is set but not supported and has no effect"))
            .collect())
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
        assert_eq!(config.session_expire_time, 2_592_000);
        assert_eq!(config.session_refresh_time, 18_000);
        assert_eq!(config.auth_mode, "native");
        assert_eq!(config.teammate_name_display, "username");
        assert_eq!(
            config.local_mode_socket_location,
            "/var/tmp/fvoci_local.socket"
        );
        assert_eq!(config.websocket_ping_seconds, 30);
        let warnings = config.check().expect("the defaults are supported");
        assert_eq!(
            warnings,
            Vec::<String>::new(),
            "the defaults cause no warning"
        );
    }

    /// The repository's own `config.json`, with its unknown keys.
    #[test]
    fn reads_the_original_configuration_file() {
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
            "session_expire_time": 2592000,
            "session_refresh_time": 18000,
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
            "FVOCI_SESSION_EXPIRE_TIME" => Some("60".to_owned()),
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
        assert_eq!(config.session_expire_time, 60);
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
        assert!(
            error(r#"{"filesdriver": "amazons3", "filess3config": {"SignV2": true}}"#)
                .contains("SignV2")
        );

        let warnings = parse(r#"{"logging_cfg_json": "{}", "audit_cfg_file": "a.json"}"#)
            .check()
            .expect("unsupported logging settings only warn");
        assert_eq!(warnings.len(), 2, "{warnings:?}");
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
}
