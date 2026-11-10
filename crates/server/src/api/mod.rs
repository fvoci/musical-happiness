//! The HTTP API under `/api/v1`, plus `/hello` and `/ping`.
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
use axum::http::{HeaderValue, Method, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use serde::Serialize;

pub(crate) use access::{
    board_refusal, require_admin, require_board_permission, require_board_visible,
    require_permission_through, require_team_access, require_team_in_body, user_is_guest,
};
pub(crate) use body::{
    decode_json, decode_object, decode_object_members, decode_object_with, json_object,
};
pub(crate) use limits::WEBSOCKET_MESSAGE_LIMIT;
pub(crate) use session::{OptionalSession, RequiredSession};

use limits::{
    BodyLimit, SessionBodyLimit, UNAUTHENTICATED_BODY_LIMIT, limit_request_body,
    limit_session_request_body,
};

use crate::app::{App, ChangeNotification};
use crate::error::{AppError, Reason};
use fvoci_store::DuplicateAs;

/// The header that shows a request was sent by a script of the server's own pages, and
/// its value. A browser does not let another site's page set it, which is one half of
/// the API's protection against cross-site request forgery; see
/// [`guard_against_forgery`].
const HEADER_REQUESTED_WITH: &str = "X-Requested-With";
const HEADER_REQUESTED_WITH_XML: &str = "XMLHttpRequest";

/// The result of an API handler.
pub(crate) type ApiResult = Result<Response, AppError>;

/// The routes under `/api/v1`.
///
/// Every request that changes something is checked for cross-site request forgery, and
/// responses are marked as not cacheable unless a handler set its own policy. Request bodies are limited: to
/// `max_request_bytes` where a route takes JSON and the request has a session, to less
/// where no session is needed, none is claimed, or the claimed one does not exist. The
/// routes that take files are outside these limits.
pub fn router(app: &App) -> Router<App> {
    let session_limit = SessionBodyLimit {
        app: app.clone(),
        limit: BodyLimit(app.config().max_request_bytes),
    };
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
            session_limit,
            limit_session_request_body,
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
        .route_layer(middleware::from_fn_with_state(
            app.clone(),
            session::renew_session_cookie,
        ))
        .route_layer(middleware::from_fn_with_state(
            app.clone(),
            guard_against_forgery,
        ))
        .route_layer(middleware::from_fn(mark_not_cacheable))
}

/// The routes outside `/api/v1`: `/hello` and `/ping`.
pub fn system_router() -> Router<App> {
    system::routes()
}

/// The administration routes, with their full paths (`/api/v1/admin/...`).
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

/// Refuses requests that another site's page may have made a browser send.
///
/// A browser sends the session cookie along by itself, so a request that comes with it
/// must show that the server's own pages sent it. Two things show that, and a request
/// that changes something needs both:
///
/// * If it has an `Origin` header, the origin is the server's own (or one of
///   `allowed_origins`). Browsers set the header on such requests and scripts cannot
///   change it.
/// * It has the header `X-Requested-With: XMLHttpRequest`, which a page of another site
///   cannot add without the server's leave, and no leave is given. A request with a
///   token in its `Authorization` header needs none: no browser adds that header by
///   itself.
///
/// `GET` and `HEAD` requests change nothing and are not checked: an image or a link can
/// point at the API. A request without any session is checked like one with the cookie:
/// a forged login would sign the browser in to somebody else's account.
pub(crate) async fn guard_against_forgery(
    State(app): State<App>,
    request: Request,
    next: Next,
) -> Response {
    if is_safe(request.method()) {
        return next.run(request).await;
    }
    let headers = request.headers();
    if app
        .authenticator()
        .origins()
        .foreign_origin(headers)
        .is_some()
    {
        tracing::debug!("a request from another origin was refused");
        return foreign_origin().into_response();
    }
    let sent_on_purpose = crate::auth::token::bearer_token(headers).is_some()
        || headers
            .get(HEADER_REQUESTED_WITH)
            .and_then(|value| value.to_str().ok())
            == Some(HEADER_REQUESTED_WITH_XML);
    if sent_on_purpose {
        next.run(request).await
    } else {
        tracing::debug!("a request without the X-Requested-With header was refused");
        AppError::forbidden("the X-Requested-With: XMLHttpRequest header is required")
            .with_reason(Reason::Csrf)
            .into_response()
    }
}

/// Whether a request of this method changes nothing.
fn is_safe(method: &Method) -> bool {
    matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS)
}

/// The refusal of a request, or of a websocket handshake, that a page of another origin
/// sent.
pub(crate) fn foreign_origin() -> AppError {
    AppError::forbidden("requests from this origin are not accepted")
        .with_reason(Reason::ForeignOrigin)
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

/// What an operation that made something new answers with: status 201 and what it
/// made, as JSON.
pub(crate) fn created(value: &impl Serialize) -> Response {
    (StatusCode::CREATED, Json(value)).into_response()
}

/// What an operation without a result answers with: status 204 and no body.
pub(crate) fn no_content() -> Response {
    StatusCode::NO_CONTENT.into_response()
}

/// Returns the percent-decoded value of a query parameter, or the empty string.
pub(crate) fn query_param(query: Option<&str>, name: &str) -> String {
    query
        .and_then(|query| crate::auth::token::query_value(query, name))
        .unwrap_or_default()
}

/// Reads the `disableNotify` query flag.
pub(crate) fn change_notification(query: Option<&str>) -> ChangeNotification {
    if query_param(query, "disableNotify") == TRUE {
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
///
/// # Errors
///
/// Returns the store's error if the board's sharing cannot be read: that is no "no",
/// and answered as one it would tell a reader with a good link that the link is dead.
pub(crate) async fn has_valid_read_token(
    app: &App,
    query: Option<&str>,
    board_id: &str,
) -> Result<bool, AppError> {
    let read_token = query_param(query, "readToken");
    if read_token.is_empty() {
        return Ok(false);
    }
    app.authenticator()
        .is_valid_read_token(board_id, &read_token)
        .await
}
