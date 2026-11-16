//! The configuration keys that environment variables can override.

use anyhow::{Context as _, bail};
use serde_json::Value;

/// How the text of an environment variable becomes a configuration value.
#[derive(Debug, Clone, Copy)]
pub(super) enum EnvKind {
    Text,
    Number,
    Flag,
    /// A comma-separated list.
    List,
}

/// The configuration keys that an environment variable can override, a key of
/// `filess3config` as `filess3config.<key>`. The variable is [`super::env_name`] of the key, for
/// example `FVOCI_SESSION_IDLE_DAYS` and `FVOCI_FILESS3CONFIG_BUCKET`. The S3 credentials
/// have none: the S3 client reads them from the `AWS_*` variables when `accesskeyid` is
/// empty.
pub(super) const ENV_KEYS: [(&str, EnvKind); 61] = [
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
    ("oidc_extra_ca_file", EnvKind::Text),
    ("oidc_former_provider_days", EnvKind::Number),
    ("org_creation", EnvKind::Text),
    ("websocket_ping_seconds", EnvKind::Number),
    ("public_link_connections_per_container", EnvKind::Number),
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

/// The value the environment's `text` for the variable `name` gives a key of `kind`.
///
/// # Errors
///
/// Returns an error naming the variable if the text is no value of that kind.
pub(super) fn env_value(name: &str, text: &str, kind: EnvKind) -> anyhow::Result<Value> {
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
