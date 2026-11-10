//! Statistics, which a standalone server does not report.

use axum::Router;
use axum::routing::get;

use super::{ApiResult, RequiredSession};
use crate::app::App;
use crate::error::AppError;

pub(super) fn routes() -> Router<App> {
    Router::new().route("/statistics", get(handle_statistics))
}

/// The board and card counts are for the administrators of the host application of the
/// original's other mode; a signed-in user of a standalone server is told so
/// (`server/api/statistics.go:36-39`).
async fn handle_statistics(RequiredSession(_session): RequiredSession) -> ApiResult {
    Err(AppError::not_implemented(
        "not permitted in standalone mode",
    ))
}
