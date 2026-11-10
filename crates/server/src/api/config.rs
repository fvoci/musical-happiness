//! `GET /client-config`.

use axum::Router;
use axum::extract::State;
use axum::routing::get;

use super::{ApiResult, json_response};
use crate::app::App;

pub(super) fn routes() -> Router<App> {
    Router::new().route("/client-config", get(get_client_config))
}

async fn get_client_config(State(app): State<App>) -> ApiResult {
    Ok(json_response(&app.client_config()))
}
