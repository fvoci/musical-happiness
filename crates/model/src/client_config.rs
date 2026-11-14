//! The configuration a server hands to its web client.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::serde_util::null_as_default;

/// The part of the server configuration that the web client needs.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(default, rename_all = "camelCase")]
pub struct ClientConfig {
    /// Whether boards may be shared publicly through a read token.
    pub enable_public_shared_boards: bool,
    /// How other users' names are displayed.
    pub teammate_name_display: String,
    /// The server's feature flags.
    #[serde(deserialize_with = "null_as_default")]
    pub feature_flags: BTreeMap<String, String>,
    /// The largest upload the server accepts, in bytes; always more than zero (100 MiB
    /// unless the server is configured otherwise).
    pub max_file_size: i64,
    /// The largest archive an import takes, in bytes; always more than zero (256 MiB
    /// unless the server is configured otherwise).
    pub max_import_size: i64,
    /// Whether the bodies of cards can be searched (`searchWorkspaceContent`): the
    /// server has a search server.
    pub search_enabled: bool,
}
