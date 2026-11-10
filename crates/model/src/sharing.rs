//! Public sharing of a board through a read token.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::serde_util::{is_zero, null_as_default};

/// The sharing state of a board.
#[derive(Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Sharing {
    /// The identifier of the shared board.
    #[serde(deserialize_with = "null_as_default")]
    pub id: String,
    /// Whether sharing is on.
    #[serde(deserialize_with = "null_as_default")]
    pub enabled: bool,
    /// The read token that grants access to the board.
    #[serde(deserialize_with = "null_as_default")]
    pub token: String,
    /// The identifier of the user who last changed the sharing state.
    #[serde(deserialize_with = "null_as_default")]
    pub modified_by: String,
    /// Last modification time in milliseconds since the Unix epoch.
    #[serde(rename = "update_at", skip_serializing_if = "is_zero")]
    #[serde(deserialize_with = "null_as_default")]
    pub update_at: i64,
}

impl fmt::Debug for Sharing {
    /// Leaves out the read token.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Sharing")
            .field("id", &self.id)
            .field("enabled", &self.enabled)
            .field("modified_by", &self.modified_by)
            .field("update_at", &self.update_at)
            .finish_non_exhaustive()
    }
}
