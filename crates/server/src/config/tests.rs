use super::*;

fn no_env(_: &str) -> Option<String> {
    None
}

#[test]
fn empty_object_gives_the_defaults() {
    let config = Configuration::parse("{}", no_env).expect("an empty object is valid");
    assert_eq!(config.server_url, "http://localhost:8000");
    assert_eq!(config.port, 8000);
    assert_eq!(config.database_kind, "sqlite3");
    assert_eq!(config.database_connection, "./fvoci.db");
    assert_eq!(config.web_app_path, "./pack");
    assert_eq!(config.file_storage, "local");
    assert_eq!(config.file_storage_path, "./files");
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
    assert_eq!(config.max_file_bytes, 100 * 1024 * 1024);
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
    assert_eq!(config.name_display, "username");
    assert_eq!(config.admin_socket_path, "/var/tmp/fvoci_local.socket");
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
    let Err(error) = Configuration::parse(r#"{"file_storage_s3": "x"}"#, no_env) else {
        panic!("a string is no S3 settings");
    };
    assert!(error.to_string().contains("file_storage_s3"), "{error}");
    Configuration::parse(r#"{"file_storage_s3": null}"#, no_env).expect("null is no settings");
}

/// The keys of a file are matched as they are written: another case, or a name of the
/// original server's configuration, is no setting, and is named in the warning of the
/// keys that are not read (without its value).
#[test]
fn keys_are_read_only_as_written_and_the_others_are_named_in_a_warning() {
    let text = r#"{
            "PORT": 8088,
            "Listen_Loopback_Only": true,
            "serverRoot": "https://hidden.example",
            "dbconfig": "postgres://u:hunter2@h/db",
            "enablePublicSharedBoards": true,
            "file_storage_s3": {"Bucket": "b", "accesskeyid": "s3cr3t", "bucket": "a"}
        }"#;
    let config = Configuration::parse(text, no_env).expect("valid JSON");
    assert_eq!(config.port, 8000, "PORT is not port");
    assert!(!config.listen_loopback_only, "another case is another key");
    assert_eq!(config.server_url, "http://localhost:8000");
    assert_eq!(config.database_connection, "./fvoci.db");
    assert!(!config.allow_public_links);
    assert_eq!(config.file_storage_s3.bucket, "a");
    assert_eq!(config.file_storage_s3.access_key_id, "");
    assert_eq!(
        config.unread_keys(),
        [
            "Listen_Loopback_Only",
            "PORT",
            "dbconfig",
            "enablePublicSharedBoards",
            "serverRoot",
            "file_storage_s3.Bucket",
            "file_storage_s3.accesskeyid",
        ]
    );
    let warnings = config.check().expect("unread keys are no error");
    assert_eq!(warnings.len(), 1, "one line: {warnings:?}");
    assert_eq!(
        warnings[0],
        "the configuration file has keys that no setting reads: Listen_Loopback_Only, PORT, \
         dbconfig, enablePublicSharedBoards, serverRoot, file_storage_s3.Bucket, \
         file_storage_s3.accesskeyid"
    );
    for value in ["hunter2", "s3cr3t", "hidden.example"] {
        assert!(!warnings[0].contains(value), "{value}: {warnings:?}");
        assert!(!format!("{config:?}").contains(value), "{value}");
    }
}

/// The keys of the settings that are read in groups of their own (mail, search, editing
/// together, wikis) are read, not named as unread; so are the keys the environment
/// sets.
#[test]
fn the_keys_of_every_setting_are_read() {
    let text = r#"{"smtp_host": "mail.example", "search_index": "other", "collab_workers": 2,
        "wiki_pages": 9, "mail_per_second": 3, "feature_flags": {"a": "b"},
        "file_storage_s3": {"bucket": "b", "server_side_encryption": true}}"#;
    let env = |name: &str| (name == "FVOCI_FILE_STORAGE_S3_REGION").then(|| "r".to_owned());
    let config = Configuration::parse(text, env).expect("valid");
    assert_eq!(config.unread_keys(), Vec::<String>::new());
    assert_eq!(config.smtp.smtp_host, "mail.example");
    assert_eq!(config.collab.collab_workers, 2);
    assert!(config.file_storage_s3.server_side_encryption);
    assert_eq!(config.file_storage_s3.region, "r");
    let defaults = code_defaults();
    let every_key: Map<String, Value> = defaults
        .iter()
        .filter(|(key, _)| !key.contains('.'))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    let config = Configuration::parse(&Value::Object(every_key).to_string(), no_env)
        .expect("the defaults, written out, are read");
    assert_eq!(config.unread_keys(), Vec::<String>::new());
}

/// The websockets are pinged every 1 to 59 seconds: a Ping must come back within the
/// quiet limit of a connection, 90 seconds, with 30 to spare. None at all is no setting.
#[test]
fn the_ping_period_is_one_to_fifty_nine_seconds() {
    let check = |seconds: u64| {
        Configuration {
            websocket_ping_seconds: seconds,
            ..Configuration::default()
        }
        .check()
    };
    for refused in [0, 60, 89, 90, u64::MAX] {
        let error = check(refused).expect_err("out of the range").to_string();
        assert!(
            error.contains("websocket_ping_seconds"),
            "{refused}: {error}"
        );
    }
    for accepted in [1, 30, 59] {
        assert_eq!(
            check(accepted).expect("in the range"),
            Vec::<String>::new(),
            "{accepted}"
        );
    }
    let env = |name: &str| (name == "FVOCI_WEBSOCKET_PING_SECONDS").then(|| "0".to_owned());
    let config = Configuration::parse("{}", env).expect("read");
    assert!(config.check().is_err(), "0 from the environment too");
}

#[test]
fn environment_overrides_the_file_and_the_defaults() {
    let env = |name: &str| match name {
        "FVOCI_PORT" => Some("9001".to_owned()),
        "FVOCI_DATABASE_CONNECTION" => Some("postgres://u@h/db".to_owned()),
        "FVOCI_ALLOW_PUBLIC_LINKS" => Some("true".to_owned()),
        "FVOCI_BLOCK_CHANGE_WEBHOOKS" => Some("http://a/hook, http://b/hook".to_owned()),
        "FVOCI_SESSION_IDLE_DAYS" => Some("3".to_owned()),
        _ => None,
    };
    let config = Configuration::parse(r#"{"port": 8000}"#, env).expect("valid");
    assert_eq!(config.port, 9001);
    assert_eq!(config.database_connection, "postgres://u@h/db");
    assert!(
        config.allow_public_links,
        "a flag is read from the environment"
    );
    assert_eq!(
        config.block_change_webhooks,
        ["http://a/hook", "http://b/hook"]
    );
    assert_eq!(config.session_idle_days, 3);
}

/// The settings of S3 that differ between deployments come from the environment too
/// (user decision 2026-11-11 11:05 KST); the credentials do not.
#[test]
fn the_environment_overrides_the_s3_settings_but_not_the_credentials() {
    let env = |name: &str| match name {
        "FVOCI_FILE_STORAGE_S3_BUCKET" => Some("from-env".to_owned()),
        "FVOCI_FILE_STORAGE_S3_HTTPS" => Some("on".to_owned()),
        "FVOCI_FILE_STORAGE_S3_TIMEOUT_MS" => Some("5000".to_owned()),
        _ => None,
    };
    let file = r#"{"file_storage_s3": {"bucket": "from-file", "region": "eu-west-1"}}"#;
    for text in [file, "{}", r#"{"file_storage_s3": null}"#] {
        let s3 = Configuration::parse(text, env)
            .expect("valid")
            .file_storage_s3;
        assert_eq!(s3.bucket, "from-env", "{text}");
        assert!(s3.https, "{text}");
        assert_eq!(s3.timeout_ms, 5000, "{text}");
    }
    let s3 = Configuration::parse(file, env)
        .expect("valid")
        .file_storage_s3;
    assert_eq!(
        s3.region, "eu-west-1",
        "the keys the environment leaves stay"
    );
    let bad = |name: &str| {
        (name == "FVOCI_FILE_STORAGE_S3_SERVER_SIDE_ENCRYPTION").then(|| "maybe".to_owned())
    };
    assert!(
        Configuration::parse("{}", bad).is_err(),
        "an unreadable flag"
    );
    // The credentials have no variable of their own: the S3 client reads AWS_*.
    let credentials =
        |name: &str| (name == "FVOCI_FILE_STORAGE_S3_ACCESS_KEY_ID").then(|| "key".to_owned());
    let s3 = Configuration::parse("{}", credentials)
        .expect("valid")
        .file_storage_s3;
    assert_eq!(s3.access_key_id, "", "not read");
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
fn the_sender_is_mail_from() {
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
            metrics_address: text.to_owned(),
            ..Configuration::default()
        }
        .metrics_listen_address()
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
    let address = |listen_loopback_only: bool| {
        Configuration {
            metrics_address: ":9092".to_owned(),
            listen_loopback_only,
            ..Configuration::default()
        }
        .metrics_listen_address()
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
    assert!(error(r#"{"auth_mode": "other"}"#).contains("auth_mode"));
    for address in [
        "9092",
        "localhost",
        ":",
        "host:port",
        "::1:9092",
        "host:70000",
        "a b:1",
    ] {
        let text = format!(r#"{{"metrics_address": "{address}"}}"#);
        assert!(
            error(&text).contains("metrics_address"),
            "{address} is refused"
        );
    }
    assert!(error(r#"{"file_storage": "ftp"}"#).contains("file_storage"));
    // Zero would not mean "no limit" to anyone who reads it as "nothing".
    assert!(error(r#"{"max_request_bytes": 0}"#).contains("max_request_bytes"));
    assert!(error(r#"{"trash_retention_days": 0}"#).contains("trash_retention_days"));
    assert!(error(r#"{"upload_part_bytes": 5242879}"#).contains("upload_part_bytes"));
    assert!(error(r#"{"upload_part_bytes": 5368709121}"#).contains("upload_part_bytes"));
    assert!(error(r#"{"max_pending_upload_bytes": 0}"#).contains("max_pending_upload_bytes"));
    for size in ["0", "-1", "83886080001"] {
        assert!(
            error(&format!(r#"{{"max_file_bytes": {size}}}"#)).contains("max_file_bytes"),
            "{size}"
        );
    }
    assert!(
        error(r#"{"max_file_bytes": 2000, "max_pending_upload_bytes": 1999}"#)
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
            r#"{"max_file_bytes": 2000, "max_import_bytes": 3000, "max_pending_upload_bytes": 2500}"#
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
    let behind = warnings(r#"{"server_url": "https://boards.example.com"}"#);
    assert_eq!(behind.len(), 1, "{behind:?}");
    assert!(behind[0].contains("trusted_proxies"), "{behind:?}");
    for quiet in [
        r#"{"server_url": "https://boards.example.com", "trusted_proxies": ["10.0.0.0/8"]}"#,
        r#"{"server_url": "https://boards.example.com", "serve_https": true}"#,
        r#"{"server_url": "http://localhost:8000"}"#,
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
    let text = r#"{"database_connection": "postgres://u:hunter2@h/db", "file_storage_s3": {"secret_access_key": "s3cr3t"}}"#;
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
    /// The default of each key, as JSON.
    defaults: BTreeMap<String, Value>,
    /// The environment variable of each key that has one.
    env: BTreeMap<String, String>,
    /// The variables of "그 밖의 환경 변수", as written.
    other_env: Vec<String>,
}

/// The text of a table cell between backticks, if it is all in backticks.
fn code(cell: &str) -> Option<&str> {
    cell.trim().strip_prefix('`')?.strip_suffix('`')
}

/// Reads the tables of the document: the rows of the sections "키: …" are keys, and
/// those of "그 밖의 환경 변수" are variables.
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
        let key = key.to_owned();
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
        }
    }
    documented
}

/// The keys of the configuration and their defaults, the keys of `file_storage_s3`
/// as `file_storage_s3.<key>`.
fn code_defaults() -> BTreeMap<String, Value> {
    let Value::Object(keys) =
        serde_json::to_value(Configuration::default()).expect("the defaults serialize")
    else {
        panic!("the configuration is an object");
    };
    let mut defaults = BTreeMap::new();
    for (key, value) in keys {
        match value {
            Value::Object(inner) if key == NESTED => {
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
        database_kind: db_type.to_owned(),
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

    let in_file = format!(r#"{{"secret_key": "{text}"}}"#);
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
