//! Compliance exports, which a standalone server does not offer.
//!
//! The exports are for system administrators, and a standalone server has none: its
//! check for system-wide permissions always says no
//! (`server/services/permissions/localpermissions/localpermissions.go:26-28`). So every
//! route ends at the permission check, or at a parameter check that comes before it.

use axum::Router;
use axum::extract::RawQuery;
use axum::routing::get;

use super::{ApiResult, RequiredSession, query_param};
use crate::app::App;
use crate::error::AppError;

pub(super) fn routes() -> Router<App> {
    Router::new()
        .route("/admin/boards", get(handle_get_boards_for_compliance))
        .route(
            "/admin/boards_history",
            get(handle_get_boards_compliance_history),
        )
        .route(
            "/admin/blocks_history",
            get(handle_get_blocks_compliance_history),
        )
}

/// Preserved quirk: a signed-in user without the permission gets 401, not 403
/// (`server/api/compliance.go:75-78`, `210-213`, `359-362`).
fn access_denied(operation: &str) -> AppError {
    AppError::unauthorized(format_args!("access denied Compliance Export {operation}"))
}

/// The history routes check for their one required parameter before anything else
/// (`server/api/compliance.go:203-206`, `352-355`).
fn require_modified_since(query: Option<&str>) -> Result<(), AppError> {
    if query_param(query, "modified_since").is_empty() {
        return Err(AppError::bad_request("`modified_since` parameter required"));
    }
    Ok(())
}

async fn handle_get_boards_for_compliance(RequiredSession(_session): RequiredSession) -> ApiResult {
    Err(access_denied("getAllBoards"))
}

async fn handle_get_boards_compliance_history(
    RequiredSession(_session): RequiredSession,
    RawQuery(query): RawQuery,
) -> ApiResult {
    require_modified_since(query.as_deref())?;
    Err(access_denied("getBoardsHistory"))
}

async fn handle_get_blocks_compliance_history(
    RequiredSession(_session): RequiredSession,
    RawQuery(query): RawQuery,
) -> ApiResult {
    require_modified_since(query.as_deref())?;
    Err(access_denied("getBlocksHistory"))
}
