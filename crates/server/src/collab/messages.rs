//! The JSON messages the server sends about a body edited together
//! (`contracts/websocket.md`, `contracts/websocket.json`).

use serde_json::json;

use crate::error::AppError;
use crate::permissions::DocMode;

/// The value of `reason` for an error: the reason of its problem, or else the name of
/// the problem of its status, as its `type` ends (`/problems/not-found`), written
/// with underscores as reasons are.
pub(super) fn reason_of(error: &AppError) -> String {
    error.reason().map_or_else(
        || error.kind().name().replace('-', "_"),
        |reason| reason.as_str().to_owned(),
    )
}

/// `DOC_JOINED`: the connection is in the room of the card's body, known there as
/// `doc`; the whole document follows as a sync step 2.
pub(super) fn joined(
    workspace_id: &str,
    card_id: &str,
    doc: u32,
    epoch: i64,
    mode: DocMode,
) -> String {
    json!({
        "action": "DOC_JOINED",
        "workspaceId": workspace_id,
        "cardId": card_id,
        "doc": doc,
        "epoch": epoch,
        "mode": mode.as_str(),
    })
    .to_string()
}

/// `DOC_REFUSED`: the connection is not let into the room.
pub(super) fn refused(workspace_id: &str, card_id: &str, reason: &str) -> String {
    json!({
        "action": "DOC_REFUSED",
        "workspaceId": workspace_id,
        "cardId": card_id,
        "reason": reason,
    })
    .to_string()
}

/// `DOC_ACK`: the update the connection sent is in the document's log as `seq`.
pub(super) fn ack(doc: u32, epoch: i64, seq: i64) -> String {
    json!({"action": "DOC_ACK", "doc": doc, "epoch": epoch, "seq": seq}).to_string()
}

/// `DOC_NACK`: the update the connection sent was not taken.
pub(super) fn nack(doc: u32, epoch: i64, reason: &str) -> String {
    json!({"action": "DOC_NACK", "doc": doc, "epoch": epoch, "reason": reason}).to_string()
}

/// `DOC_MODE`: how the connection is in the room from now on.
pub(super) fn mode(doc: u32, mode: DocMode) -> String {
    json!({"action": "DOC_MODE", "doc": doc, "mode": mode.as_str()}).to_string()
}

/// `DOC_RESET`: the document follows anew, as a sync step 2; of another `epoch` than
/// before, it is another document.
pub(super) fn reset(doc: u32, epoch: i64) -> String {
    json!({"action": "DOC_RESET", "doc": doc, "epoch": epoch}).to_string()
}

/// `DOC_LEFT`: the server put the connection out of the room.
pub(super) fn left(doc: u32, reason: &str) -> String {
    json!({"action": "DOC_LEFT", "doc": doc, "reason": reason}).to_string()
}
