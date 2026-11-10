//! The board archive format used for import and export.

use crate::serde_util::null_as_default;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The content of `version.json`, the first entry of an archive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ArchiveHeader {
    /// The archive format version.
    #[serde(deserialize_with = "null_as_default")]
    pub version: i64,
    /// The export time in milliseconds since the Unix epoch.
    #[serde(deserialize_with = "null_as_default")]
    pub date: i64,
}

/// One line of a board's `board.jsonl`.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ArchiveLine {
    /// What `data` holds: `board`, `block` or `boardMember`.
    #[serde(rename = "type")]
    pub line_type: String,
    /// The entity itself, decoded according to `line_type`.
    pub data: Value,
}
