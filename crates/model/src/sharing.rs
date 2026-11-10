//! Public sharing of a board through a read token.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::serde_util::{is_zero, null_as_default};

/// The sharing state of a board.
///
/// The JSON form is what is answered to those who may share the board: whether it is
/// shared, the read token while it is, and when the state last changed. A board that
/// was never shared is `{"enabled": false}`.
#[derive(Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Sharing {
    /// The identifier of the board.
    #[serde(skip)]
    pub board_id: String,
    /// Whether the board is shared.
    #[serde(deserialize_with = "null_as_default")]
    pub enabled: bool,
    /// The read token: whoever has it reads the board. The server makes it. Empty, and
    /// left out of the JSON form, while the board is not shared.
    #[serde(
        deserialize_with = "null_as_default",
        skip_serializing_if = "String::is_empty"
    )]
    pub token: String,
    /// The identifier of the user who last changed the sharing state.
    #[serde(skip)]
    pub modified_by: String,
    /// When the state last changed, in milliseconds since the Unix epoch. Zero, and
    /// left out of the JSON form, for a board that was never shared.
    #[serde(deserialize_with = "null_as_default", skip_serializing_if = "is_zero")]
    pub update_at: i64,
}

impl fmt::Debug for Sharing {
    /// Leaves out the read token.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Sharing")
            .field("board_id", &self.board_id)
            .field("enabled", &self.enabled)
            .field("modified_by", &self.modified_by)
            .field("update_at", &self.update_at)
            .finish_non_exhaustive()
    }
}
