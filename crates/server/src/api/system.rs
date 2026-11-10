//! `/hello` and `/ping`, which sit outside `/api/v1` and need neither a session nor the
//! cross-site request forgery header.

use axum::Router;
use axum::extract::State;
use axum::routing::get;

use super::{ApiResult, json_response};
use crate::app::App;

pub(super) fn routes() -> Router<App> {
    Router::new()
        .route("/hello", get(handle_hello))
        .route("/ping", get(handle_ping))
}

async fn handle_hello() -> &'static str {
    "Hello"
}

async fn handle_ping(State(app): State<App>) -> ApiResult {
    Ok(json_response(&app.server_metadata().await))
}
