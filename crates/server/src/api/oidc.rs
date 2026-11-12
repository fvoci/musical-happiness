//! Signing in through an organization's `OpenID Connect` provider: what the login page
//! may know beforehand, the start, and the return from the provider.
//!
//! The start and the return are addresses a browser is sent to, not requests of a
//! script: they answer with a redirect (303), to the provider, to the web app, or to
//! the login page with the name of what went wrong in its `oidc_error` parameter. The
//! browser that starts a sign-in gets a cookie of its own for it, and only the browser
//! with that cookie finishes it.

use axum::extract::{Extension, RawQuery, State};
use axum::http::header::{LOCATION, SET_COOKIE};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use fvoci_model::LoginOptions;

use super::auth::{address_of, user_agent};
use super::contract::params::{LoginOrg, OidcCode, OidcError, OidcState, StartOrg};
use super::contract::tag;
use super::{ApiResult, json_response, query_param};
use crate::app::{App, Callback, LOGIN_FLOW_LIFETIME_MILLIS, LoginFailure};
use crate::auth::token::{Https, cookie_value, query_value};
use crate::client_address::ClientAddress;
use crate::error::Reason;

/// The cookie that ties a sign-in to the browser that started it, where the server is
/// reached over HTTPS: the `__Host-` prefix keeps other hosts of the domain from
/// planting one.
const FLOW_COOKIE_SECURE: &str = "__Host-fvoci_oidc";
/// The same cookie where the server is reached over plain HTTP.
const FLOW_COOKIE_PLAIN: &str = "fvoci_oidc";

/// The routes anyone may call: they come before there is a session.
pub(super) fn unauthenticated_routes() -> OpenApiRouter<App> {
    OpenApiRouter::new()
        .routes(routes!(handle_login_options))
        .routes(routes!(handle_start))
        .routes(routes!(handle_callback))
}

/// What the login page shows
///
/// The organization the login page is for, and whether people sign in through its
/// provider: the one `org` names, or without it the only organization of a server that
/// holds one. Its identifier, its name and whether it has a provider that is on, and
/// nothing else. An identifier that names no organization in use answers as none
/// (`organization` is `null`), and so does a server that takes in several
/// organizations when none is named.
#[utoipa::path(
    get,
    path = "/login-options",
    operation_id = "getLoginOptions",
    tag = tag::AUTH,
    params(LoginOrg),
    security(()),
    responses((status = 200, description = "What the login page shows.", body = LoginOptions)),
)]
async fn handle_login_options(State(app): State<App>, RawQuery(query): RawQuery) -> ApiResult {
    let org = query.as_deref().and_then(|query| query_value(query, "org"));
    let options = app.login_options(org.as_deref()).await?;
    Ok(json_response(&options))
}

/// Start signing in through a provider
///
/// For a browser: sends it to the provider of the organization `org` (303), with a
/// cookie that ties the sign-in to it, and ten minutes to come back. A browser that
/// says a page of another site sent it here (`Sec-Fetch-Site` other than `same-origin`
/// or `none`) is sent to the login page with `oidc_state_invalid`. If the
/// organization has no provider that is on, or it cannot be reached, the browser is
/// sent to the login page with the reason `oidc_unavailable` in `oidc_error`, by the
/// name of its problem (`oidc-unavailable`), or `too-many-requests`.
#[utoipa::path(
    get,
    path = "/auth/oidc/start",
    operation_id = "startOidcLogin",
    tag = tag::AUTH,
    params(StartOrg),
    security(()),
    responses((
        status = 303,
        description = "To the provider, or to the login page.",
        headers(
            ("Location" = String, description = "Where the browser goes."),
            ("Set-Cookie" = String, description = "The cookie of the sign-in, if it was started."),
        ),
    )),
)]
async fn handle_start(
    State(app): State<App>,
    client: Option<Extension<ClientAddress>>,
    headers: HeaderMap,
    RawQuery(query): RawQuery,
) -> ApiResult {
    let org_id = query_param(query.as_deref(), "org");
    if sent_from_another_site(&headers) {
        tracing::info!("a sign-in through a provider was started from another site");
        return Ok(to_login_page(
            &app,
            Some(&org_id),
            LoginFailure::Reason(Reason::OidcStateInvalid),
        ));
    }
    match app.start_oidc_login(&org_id, address_of(client)).await {
        Ok(started) => {
            let mut response = see_other(&started.authorization_url);
            let cookie = flow_cookie(
                &app,
                &started.browser_secret,
                LOGIN_FLOW_LIFETIME_MILLIS / 1000,
            );
            response.headers_mut().append(SET_COOKIE, cookie);
            Ok(response)
        }
        Err(failure) => Ok(to_login_page(&app, Some(&org_id), failure)),
    }
}

/// Come back from a provider
///
/// Where a provider sends the browser back to, for every organization. The sign-in is
/// taken by its `state`, once, and only from the browser that started it; the code is
/// exchanged for an ID token, whose signature, issuer, audience, time and nonce are
/// checked. The person is known by the issuer and the subject: an account of theirs
/// signs in, and somebody who has none gets one, if the organization lets newcomers in
/// and no account has their email address. The session comes in the `Set-Cookie`
/// header, and the browser goes to the web app (303).
///
/// Otherwise the browser goes to the login page of the organization (`/login?org=…`),
/// with the reason of what went wrong in `oidc_error`, by the name of its problem
/// (`oidc_state_invalid` as `oidc-state-invalid`): `oidc_state_invalid`, `oidc_denied`,
/// `oidc_token_refused`, `oidc_token_invalid`, `oidc_email_missing`,
/// `oidc_email_unverified`, `oidc_email_taken`, `oidc_join_closed`, `oidc_unavailable`,
/// or `too-many-requests`.
#[utoipa::path(
    get,
    path = "/auth/oidc/callback",
    operation_id = "finishOidcLogin",
    tag = tag::AUTH,
    params(OidcState, OidcCode, OidcError),
    security(()),
    responses((
        status = 303,
        description = "To the web app, signed in, or to the login page.",
        headers(
            ("Location" = String, description = "Where the browser goes."),
            ("Set-Cookie" = String, description = "The session cookie, and the cookie of the sign-in cleared."),
        ),
    )),
)]
async fn handle_callback(
    State(app): State<App>,
    headers: HeaderMap,
    RawQuery(query): RawQuery,
) -> ApiResult {
    let query = query.unwrap_or_default();
    let callback = Callback {
        state: query_value(&query, "state").unwrap_or_default(),
        code: query_value(&query, "code").unwrap_or_default(),
        error: query_value(&query, "error"),
        browser_secret: cookie_value(&headers, flow_cookie_name(&app)).unwrap_or_default(),
    };
    let (org_id, result) = app
        .finish_oidc_login(&callback, &user_agent(&headers))
        .await;
    let mut response = match result {
        Ok(finished) => {
            let mut response = see_other(&app_root(&app));
            let cookie = app.authenticator().session_cookie().set(&finished.token);
            response.headers_mut().append(SET_COOKIE, cookie);
            response
        }
        Err(failure) => to_login_page(&app, org_id.as_deref(), failure),
    };
    // The sign-in is over, whatever came of it.
    response
        .headers_mut()
        .append(SET_COOKIE, flow_cookie(&app, "", 0));
    Ok(response)
}

/// Whether a browser says it was sent here by a page of another site
/// (`Sec-Fetch-Site`). A sign-in started so could be another's: a page of theirs would
/// send the browser through a provider that lets anybody in as the one person they
/// chose, and the browser would come back signed in to that account. A start comes
/// from the server's own login page, or from an address the person typed or followed
/// from outside a page (`none`). A client that sends no such header is no browser that
/// a page could send.
fn sent_from_another_site(headers: &HeaderMap) -> bool {
    headers
        .get("sec-fetch-site")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|site| !matches!(site, "same-origin" | "none"))
}

/// A redirect to `location`, which the server made: always a valid header value.
fn see_other(location: &str) -> Response {
    let mut response = StatusCode::SEE_OTHER.into_response();
    if let Ok(value) = HeaderValue::from_str(location) {
        response.headers_mut().insert(LOCATION, value);
    } else {
        tracing::error!("a redirect to an address that is no header value");
        *response.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
    }
    response
}

/// The address of the web app, from the server's own address.
fn app_root(app: &App) -> String {
    format!("{}/", app.config().server_root.trim_end_matches('/'))
}

/// A redirect to the login page of the organization `org_id`, if one is known, with the
/// name of what went wrong.
fn to_login_page(app: &App, org_id: Option<&str>, failure: LoginFailure) -> Response {
    let mut location = format!("{}login?", app_root(app));
    // The identifier goes back only if it is one: nothing of the request is repeated.
    if let Some(org_id) = org_id.filter(|org_id| fvoci_model::is_id(org_id)) {
        location.push_str("org=");
        location.push_str(org_id);
        location.push('&');
    }
    location.push_str("oidc_error=");
    location.push_str(&failure.name());
    see_other(&location)
}

fn flow_cookie_name(app: &App) -> &'static str {
    if app.authenticator().origins().https() == Https::Yes {
        FLOW_COOKIE_SECURE
    } else {
        FLOW_COOKIE_PLAIN
    }
}

/// The cookie of a sign-in: not readable by scripts, sent along with the browser's
/// return from the provider (a top-level navigation, which `SameSite=Lax` lets
/// through), for the whole host as the `__Host-` prefix wants.
fn flow_cookie(app: &App, secret: &str, max_age_seconds: i64) -> HeaderValue {
    let secure = if app.authenticator().origins().https() == Https::Yes {
        "; Secure"
    } else {
        ""
    };
    let cookie = format!(
        "{}={secret}; Max-Age={max_age_seconds}; Path=/; HttpOnly; SameSite=Lax{secure}",
        flow_cookie_name(app)
    );
    // A secret is base64url text and the rest is fixed: always a valid header value.
    HeaderValue::from_str(&cookie).unwrap_or_else(|_| HeaderValue::from_static(""))
}
