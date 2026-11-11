//! What a list of the trash says about a thing in it.

use serde::Serialize;

/// A card or a board in the trash, with as much as it takes to decide whether to bring
/// it back (`undeleteBlock`, `undeleteBoard`).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeletedItem {
    /// The identifier of the card or the board.
    pub id: String,
    /// Its title when it was deleted.
    pub title: String,
    /// When it was deleted, in milliseconds since the Unix epoch.
    pub deleted_at: i64,
    /// Who deleted it; empty if nobody known.
    pub deleted_by: String,
}
