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
    assert_eq!(config.oidc_former_provider_days, 30);
    assert_eq!(
        config.oidc_former_provider_kept(),
        std::time::Duration::from_hours(30 * 24)
    );
    assert_eq!(config.upload_part_bytes, 8 * 1024 * 1024);
    assert_eq!(config.max_file_size, 100 * 1024 * 1024);
    assert_eq!(config.max_pending_upload_bytes, 2 * 1024 * 1024 * 1024);
    assert_eq!(config.max_import_bytes, 256 * 1024 * 1024);
    assert_eq!(config.max_comment_versions, 50);
    assert_eq!(config.max_concurrent_exports, 2);
    assert_eq!(config.export_stall_seconds, 60);
    assert_eq!(config.export_stall(), std::time::Duration::from_secs(60));
    assert_eq!(config.max_concurrent_imports, 2);
    assert_eq!(config.server_lock_wait_seconds, 30);
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
    let blank = |asked: &str| (asked == "FVOCI_FILESS3CONFIG_SECRETACCESSKEY").then(String::new);
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
fn the_old_name_of_the_sender_is_refused_with_the_new_one() {
    let error = Configuration::parse(r#"{"smtp_from": "sender@example.test"}"#, no_env)
        .expect_err("the sender under its old name is not passed over")
        .to_string();
    assert!(error.contains("smtp_from"), "{error}");
    assert!(error.contains("mail_from"), "{error}");
    assert!(error.contains("FVOCI_MAIL_FROM"), "{error}");
    assert!(!error.contains("sender@example.test"), "{error}");
    let env = |name: &str| (name == "FVOCI_SMTP_FROM").then(|| "sender@example.test".to_owned());
    let error = Configuration::parse("{}", env)
        .expect_err("its variable too")
        .to_string();
    assert!(
        error.contains("FVOCI_SMTP_FROM"),
        "the variable is named: {error}"
    );
    assert!(error.contains("mail_from"), "{error}");
    let config = Configuration::parse(r#"{"mail_from": "sender@example.test"}"#, no_env)
        .expect("the sender under its name");
    assert_eq!(config.smtp.mail_from, "sender@example.test");
    let env = |name: &str| (name == "FVOCI_MAIL_FROM").then(|| "env@example.test".to_owned());
    let config = Configuration::parse("{}", env).expect("its variable");
    assert_eq!(config.smtp.mail_from, "env@example.test");
}

#[test]
fn the_metrics_address_is_host_and_port() {
    let address = |text: &str| {
        Configuration {
            prometheus_address: text.to_owned(),
            ..Configuration::default()
        }
        .metrics_address()
        .expect("a valid address")
    };
    assert_eq!(address(""), None, "no metrics");
    let expected = |host: Option<&str>, port| {
        Some(MetricsAddress {
            host: host.map(str::to_owned),
            port,
        })
    };
    assert_eq!(address(":9092"), expected(None, 9092), "every interface");
    assert_eq!(address("127.0.0.1:9092"), expected(Some("127.0.0.1"), 9092));
    assert_eq!(address("[::1]:9092"), expected(Some("::1"), 9092));
    assert_eq!(
        address("metrics.internal:0"),
        expected(Some("metrics.internal"), 0)
    );
}

#[test]
fn metrics_without_a_host_listen_on_the_loopback_of_a_local_only_server() {
    let address = |local_only: bool| {
        Configuration {
            prometheus_address: ":9092".to_owned(),
            local_only,
            ..Configuration::default()
        }
        .metrics_address()
        .expect("a valid address")
        .expect("metrics are kept")
    };
    assert_eq!(address(false).host, None, "every interface");
    assert_eq!(
        address(true).host.as_deref(),
        Some("localhost"),
        "the loopback only, as the API"
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
    for address in [
        "9092",
        "localhost",
        ":",
        "host:port",
        "::1:9092",
        "host:70000",
        "a b:1",
    ] {
        let text = format!(r#"{{"prometheusaddress": "{address}"}}"#);
        assert!(
            error(&text).contains("prometheusaddress"),
            "{address} is refused"
        );
    }
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
        error(
            r#"{"maxfilesize": 2000, "max_import_bytes": 3000, "max_pending_upload_bytes": 2500}"#
        )
        .contains("max_import_bytes")
    );
    assert!(
        error(r#"{"max_comment_versions": 0}"#).contains("max_comment_versions"),
        "a comment keeps at least the text it was posted with"
    );
    for key in [
        "max_concurrent_exports",
        "max_concurrent_imports",
        "export_stall_seconds",
        "public_link_connections_per_container",
        "public_link_connections_per_address",
    ] {
        assert!(
            error(&format!(r#"{{"{key}": 0}}"#)).contains(key),
            "{key}: none at all would refuse every one"
        );
    }
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
        let max = u64::MAX;
        let text = if key == "login_pair_max_delay_seconds" {
            format!(r#"{{"{key}": {max}}}"#)
        } else {
            format!(r#"{{"login_pair_max_delay_seconds": {max}, "{key}": {max}}}"#)
        };
        let message = error(&text);
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
    /// The variables of "그 밖의 환경 변수", as written.
    other_env: Vec<String>,
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
        if section == "그 밖의 환경 변수" {
            documented.other_env.push(key.to_owned());
            continue;
        }
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

/// `org_creation` is one of two words, and `open` is for PostgreSQL.
#[test]
fn org_creation_is_first_user_only_or_open_and_open_needs_postgresql() {
    let with = |org_creation: &str, db_type: &str| Configuration {
        org_creation: org_creation.to_owned(),
        db_type: db_type.to_owned(),
        ..Configuration::default()
    };
    assert_eq!(Configuration::default().org_creation, "first_user_only");
    for db_type in ["sqlite3", "postgres"] {
        with("first_user_only", db_type)
            .check()
            .expect("one organization on either database");
    }
    with("open", "postgres")
        .check()
        .expect("several organizations on PostgreSQL");
    let refused = with("open", "sqlite3").check().expect_err("not on SQLite");
    assert!(refused.to_string().contains("postgres"), "{refused}");
    for unknown in ["", "Open", "everybody"] {
        let refused = with(unknown, "postgres")
            .check()
            .expect_err("a word that is none of the two");
        assert!(refused.to_string().contains("org_creation"), "{refused}");
    }
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
        .chain(&super::collab::ENV_KEYS)
        .chain(&super::smtp::ENV_KEYS)
        .chain(&super::mail::ENV_KEYS)
        .chain(&super::search::ENV_KEYS)
        .chain(&super::wiki::ENV_KEYS)
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
    for (key, variable) in ENV_ONLY {
        assert!(
            documented.other_env.iter().any(|name| name == variable),
            "docs/configuration.md lists {variable}, the variable of {key}, among the other \
             environment variables"
        );
        assert!(!defaults.contains_key(key), "{key} is no key of the file");
    }
}

/// The key the server seals secrets with comes from `FVOCI_SECRET_KEY` and from
/// nowhere else, and what is wrong with it is said without its value.
#[test]
fn the_secret_key_is_read_from_the_environment_only() {
    use base64::Engine as _;
    let text = base64::engine::general_purpose::STANDARD.encode([9_u8; 32]);
    let env =
        |value: &'static str| move |name: &str| (name == SECRET_KEY_ENV).then(|| value.to_owned());
    let leaked: &'static str = Box::leak(text.clone().into_boxed_str());

    assert!(
        Configuration::parse("{}", no_env)
            .expect("no key")
            .secret_key
            .is_none()
    );
    assert!(
        Configuration::parse("{}", env("  "))
            .expect("a blank key")
            .secret_key
            .is_none()
    );
    let config = Configuration::parse("{}", env(leaked)).expect("a key");
    assert_eq!(
        config.secret_key,
        Some(crate::auth::sealed::SecretKey::from_base64(&text).expect("a key"))
    );
    assert!(
        !format!("{config:?}").contains(&text),
        "the key is not written by Debug"
    );

    let short = "c2hvcnQgYW5kIHdyb25nIGtleQ==";
    let refused = Configuration::parse("{}", env(short)).expect_err("too short");
    let said = format!("{refused:#}");
    assert!(
        said.contains(SECRET_KEY_ENV) && !said.contains(short),
        "{said}"
    );

    let in_file = format!(r#"{{"Secret_Key": "{text}"}}"#);
    let refused = Configuration::parse(&in_file, no_env).expect_err("not from the file");
    let said = format!("{refused:#}");
    assert!(
        said.contains(SECRET_KEY_ENV) && !said.contains(&text),
        "{said}"
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

/// How long a former provider is kept is read from the file and the environment; no
/// days keep none, and a number that is no count of days is refused.
#[test]
fn the_days_a_former_provider_is_kept_are_read() {
    let from_file = Configuration::parse(r#"{"oidc_former_provider_days": 7}"#, no_env);
    let env = |name: &str| (name == "FVOCI_OIDC_FORMER_PROVIDER_DAYS").then(|| "0".to_owned());
    let from_env = Configuration::parse(r#"{"oidc_former_provider_days": 7}"#, env);
    let negative = Configuration::parse(r#"{"oidc_former_provider_days": -1}"#, no_env);

    assert_eq!(from_file.expect("valid").oidc_former_provider_days, 7);
    assert!(
        from_env
            .expect("valid")
            .oidc_former_provider_kept()
            .is_zero(),
        "the environment's zero keeps none"
    );
    assert!(negative.is_err(), "a negative number of days is refused");
}
