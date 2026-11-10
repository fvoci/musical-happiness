//! `/hello` and `/ping`, which sit outside `/api/v1` and need neither a session nor the
//! cross-site request forgery header.

use axum::extract::State;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::contract::tag;

use super::{ApiResult, json_response};
use crate::app::{App, ServerMetadata};

pub(super) fn routes() -> OpenApiRouter<App> {
    OpenApiRouter::new()
        .routes(routes!(handle_hello))
        .routes(routes!(handle_ping))
}

/// Whether the server is there
///
/// Answers the text `Hello`.
#[utoipa::path(
    get,
    path = "/hello",
    operation_id = "hello",
    tag = tag::SYSTEM,
    security(()),
    responses((status = 200, description = "The server is there.", body = String, content_type = "text/plain")),
)]
async fn handle_hello() -> &'static str {
    "Hello"
}

/// What the server is
///
/// The version of the server and what it runs on.
#[utoipa::path(
    get,
    path = "/ping",
    operation_id = "ping",
    tag = tag::SYSTEM,
    security(()),
    responses((status = 200, description = "What the server says about itself.", body = ServerMetadata)),
)]
async fn handle_ping(State(app): State<App>) -> ApiResult {
    Ok(json_response(&app.server_metadata().await))
}
