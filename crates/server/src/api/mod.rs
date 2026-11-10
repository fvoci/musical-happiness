//! The HTTP API under `/api/v2`, plus `/hello` and `/ping`.
//!
//! One module per area, mirroring the original's `server/api` package. Each module has a
//! `routes()` function, and [`router`] puts them together.

mod access;
mod admin;
mod archive;
mod auth;
mod blocks;
mod boards;
mod boards_and_blocks;
mod body;
mod cards;
mod categories;
mod config;
mod content_blocks;
mod files;
mod limits;
mod members;
mod onboarding;
mod search;
mod session;
mod sharing;
mod subscriptions;
mod system;
mod teams;
mod templates;
mod users;

use axum::Json;
use axum::Router;
use axum::extract::{DefaultBodyLimit, Request, State};
use axum::http::header::CACHE_CONTROL;
use axum::http::{HeaderMap, HeaderValue};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use serde_json::json;

pub(crate) use access::{
    board_refusal, require_board_permission, require_board_visible, require_permission_through,
    require_team_access, user_is_guest,
};
pub(crate) use body::{
    decode_json, decode_object, decode_object_members, decode_object_with, json_object,
};
pub(crate) use limits::WEBSOCKET_MESSAGE_LIMIT;
pub(crate) use session::{OptionalSession, RequiredSession};

use limits::{BodyLimit, UNAUTHENTICATED_BODY_LIMIT, limit_request_body};

use crate::app::{App, ChangeNotification};
use crate::error::{AppError, Reason};
use fvoci_store::DuplicateAs;

/// The header every API request must carry, and its value. A browser does not let another
/// site's page set it, which is the API's protection against cross-site request forgery.
const HEADER_REQUESTED_WITH: &str = "X-Requested-With";
const HEADER_REQUESTED_WITH_XML: &str = "XMLHttpRequest";

/// The result of an API handler.
pub(crate) type ApiResult = Result<Response, AppError>;

/// The routes under `/api/v2`.
///
/// Every route checks the cross-site request forgery header, and responses are marked as
/// not cacheable unless a handler set its own policy. Request bodies are limited: to
/// `max_request_bytes` where a route takes JSON, to less where no session is needed or
/// none is claimed. The routes that take files are outside these limits.
pub fn router(max_request_bytes: u64) -> Router<App> {
    let json = Router::new()
        .merge(archive::routes())
        .merge(auth::routes())
        .merge(blocks::routes())
        .merge(boards::routes())
        .merge(boards_and_blocks::routes())
        .merge(cards::routes())
        .merge(categories::routes())
        .merge(config::routes())
        .merge(content_blocks::routes())
        .merge(files::routes())
        .merge(members::routes())
        .merge(onboarding::routes())
        .merge(search::routes())
        .merge(sharing::routes())
        .merge(subscriptions::routes())
        .merge(teams::routes())
        .merge(templates::routes())
        .merge(users::routes())
        .layer(middleware::from_fn_with_state(
            BodyLimit(max_request_bytes),
            limit_request_body,
        ));
    let unauthenticated = auth::unauthenticated_routes().layer(middleware::from_fn_with_state(
        UNAUTHENTICATED_BODY_LIMIT,
        limit_request_body,
    ));
    // Not behind the limits above. A file upload is bounded by `maxfilesize` where it
    // is read, which is "no limit" at its default of 0; an archive import has no limit
    // on the request yet. Both get limits with the chunked upload (design 5.6).
    let uploads = Router::new()
        .merge(files::upload_routes())
        .merge(archive::upload_routes());

    Router::new()
        .merge(json)
        .merge(unauthenticated)
        .merge(uploads)
        // The limits above are the ones that count, and they answer in the API's own
        // shape; the framework's limit of 2 MB on what an extractor reads would cut in
        // below them with an answer of its own.
        .layer(DefaultBodyLimit::disable())
        .route_layer(middleware::from_fn(require_csrf_header))
        .route_layer(middleware::from_fn(mark_not_cacheable))
}

/// The routes outside `/api/v2`: `/hello` and `/ping`.
pub fn system_router() -> Router<App> {
    system::routes()
}

/// The administration routes, with their full paths (`/api/v2/admin/...`).
///
/// They are to be served on the local Unix socket only, and refuse every request that
/// arrives another way. Whatever else is asked on that socket gets a 404.
pub fn admin_router() -> Router<App> {
    // The one body this socket takes is a password; it gets the small limit, and the
    // refusal in the API's own format.
    admin::routes()
        .layer(middleware::from_fn_with_state(
            UNAUTHENTICATED_BODY_LIMIT,
            limit_request_body,
        ))
        .layer(DefaultBodyLimit::disable())
}

async fn require_csrf_header(request: Request, next: Next) -> Response {
    let header = request
        .headers()
        .get(HEADER_REQUESTED_WITH)
        .and_then(|value| value.to_str().ok());
    if header == Some(HEADER_REQUESTED_WITH_XML) {
        next.run(request).await
    } else {
        tracing::debug!("a request without the X-Requested-With header was refused");
        AppError::forbidden("the X-Requested-With: XMLHttpRequest header is required")
            .with_reason(Reason::Csrf)
            .into_response()
    }
}

/// Runs the rest of the request as a task of its own, so that it finishes even if the
/// client disconnects.
///
/// The framework drops a handler when its connection closes. A handler that has stored a
/// board but not yet filed it under a category, or stored blocks but not yet told the
/// listeners, would stop half-way; the original's handlers always run to their end.
pub(crate) async fn run_to_completion(
    State(app): State<App>,
    request: Request,
    next: Next,
) -> Response {
    match app.spawn_tracked(next.run(request)).await {
        Ok(response) => response,
        // Let the panic layer see a panic of the handler as if it had happened here.
        Err(error) if error.is_panic() => std::panic::resume_unwind(error.into_panic()),
        Err(_) => AppError::internal("the request was cancelled").into_response(),
    }
}

/// API responses depend on who asks and change at any time, so no cache may keep them.
async fn mark_not_cacheable(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    response
        .headers_mut()
        .entry(CACHE_CONTROL)
        .or_insert(HeaderValue::from_static("no-store"));
    response
}

/// A JSON response with status 200.
pub(crate) fn json_response(value: &impl Serialize) -> Response {
    Json(value).into_response()
}

/// The empty JSON object that most operations without a result answer with.
pub(crate) fn empty_response() -> Response {
    Json(json!({})).into_response()
}

/// Returns the percent-decoded value of a query parameter, or the empty string.
pub(crate) fn query_param(query: Option<&str>, name: &str) -> String {
    query
        .and_then(|query| crate::auth::token::query_value(query, name))
        .unwrap_or_default()
}

/// Reads the `disable_notify` query flag.
pub(crate) fn change_notification(query: Option<&str>) -> ChangeNotification {
    if query_param(query, "disable_notify") == TRUE {
        ChangeNotification::Suppress
    } else {
        ChangeNotification::Send
    }
}

/// Reads the `asTemplate` query flag of the duplication routes; it counts only as
/// exactly `true`.
pub(crate) fn duplicate_as(query: Option<&str>) -> DuplicateAs {
    if query_param(query, "asTemplate") == TRUE {
        DuplicateAs::Template
    } else {
        DuplicateAs::Copy
    }
}

/// The value of a query flag that is set.
pub(crate) const TRUE: &str = "true";

/// Returns whether the request carries a read token that is valid for the board.
pub(crate) async fn has_valid_read_token(app: &App, query: Option<&str>, board_id: &str) -> bool {
    let read_token = query_param(query, "read_token");
    if read_token.is_empty() {
        return false;
    }
    match app
        .authenticator()
        .is_valid_read_token(board_id, &read_token)
        .await
    {
        Ok(valid) => valid,
        Err(error) => {
            tracing::error!(%error, "IsValidReadTokenForBoard ERROR");
            false
        }
    }
}

/// Returns the session token a request carries, if any.
pub(crate) fn request_token(headers: &HeaderMap, query: Option<&str>) -> String {
    crate::auth::token::parse_auth_token(headers, query)
        .map(|(token, _)| token)
        .unwrap_or_default()
}
