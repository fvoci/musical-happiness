//! Channels, which a standalone server does not have.

use axum::Router;
use axum::routing::get;

use super::{ApiResult, RequiredSession};
use crate::app::App;
use crate::error::AppError;

pub(super) fn routes() -> Router<App> {
    Router::new().route(
        "/teams/{team_id}/channels/{channel_id}",
        get(handle_get_channel),
    )
}

/// Channels belong to the host application of the original's other mode; a signed-in
/// user of a standalone server is told so (`server/api/channels.go:53-56`).
async fn handle_get_channel(RequiredSession(_session): RequiredSession) -> ApiResult {
    Err(AppError::not_implemented(
        "not permitted in standalone mode",
    ))
}
