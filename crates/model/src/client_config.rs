//! The configuration a server hands to its web client.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::serde_util::null_as_default;

/// The part of the server configuration that the web client needs.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ClientConfig {
    /// Whether telemetry is on. This server has none and always sends `false`; the member
    /// exists because existing clients read it.
    pub telemetry: bool,
    /// The telemetry identifier. Always empty, for the same reason.
    #[serde(rename = "telemetryid")]
    pub telemetry_id: String,
    /// Whether boards may be shared publicly through a read token.
    pub enable_public_shared_boards: bool,
    /// How other users' names are displayed.
    pub teammate_name_display: String,
    /// The server's feature flags.
    #[serde(deserialize_with = "null_as_default")]
    pub feature_flags: BTreeMap<String, String>,
    /// The largest upload the server accepts, in bytes.
    pub max_file_size: i64,
}
