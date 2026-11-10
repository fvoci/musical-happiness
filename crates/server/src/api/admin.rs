//! The administration API: setting a user's password without knowing the old one, which
//! also ends every session of that user, and making a user an administrator of the
//! instance.
//!
//! It is served on the local Unix socket only, never on the TCP listener, so that being
//! able to open the socket file is the credential. There is no session and no cross-site
//! request forgery header, as in the original (`server/api/api.go` `RegisterAdminRoutes`).

use axum::Router;
use axum::body::Bytes;
use axum::extract::{ConnectInfo, Path, Request, State};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use serde::Deserialize;

use super::{ApiResult, decode_json, no_content};
use crate::Peer;
use crate::app::App;
use crate::error::AppError;

pub(super) fn routes() -> Router<App> {
    Router::new()
        .route(
            "/api/v1/admin/users/{username}/password",
            post(handle_admin_set_password),
        )
        .route(
            "/api/v1/admin/users/{username}/admin",
            post(handle_admin_make_admin),
        )
        .route_layer(middleware::from_fn(require_unix_connection))
        // The socket speaks the API's language for what it does not have as well.
        .fallback(|| async { AppError::not_found("route") })
        .method_not_allowed_fallback(|| async { AppError::method_not_allowed() })
}

/// The body of a request to set a password.
#[derive(Deserialize)]
struct AdminSetPasswordData {
    /// Absent and `null` read as empty, as the original's decoder reads them.
    #[serde(default)]
    password: Option<String>,
}

/// Refuses requests that did not arrive through a Unix socket, the original's
/// `adminRequired` (`server/api/auth.go:399-410`). The routes are not mounted anywhere
/// else; this keeps them closed should they ever be.
async fn require_unix_connection(request: Request, next: Next) -> Response {
    let peer = request
        .extensions()
        .get::<ConnectInfo<Peer>>()
        .map(|ConnectInfo(peer)| *peer);
    if peer == Some(Peer::Unix) {
        next.run(request).await
    } else {
        // For anyone who does not come through the socket these routes do not exist.
        AppError::not_found("route").into_response()
    }
}

async fn handle_admin_set_password(
    State(app): State<App>,
    Path(username): Path<String>,
    body: Bytes,
) -> ApiResult {
    // A body of `null` decodes to nothing in the original and so to an empty password.
    let data: Option<AdminSetPasswordData> = decode_json(&body)?;
    let password = data.and_then(|data| data.password).unwrap_or_default();

    // The rules for a password are the same whoever sets it.
    fvoci_model::check_new_password(&password)?;

    app.reset_user_password(&username, &password).await?;
    tracing::debug!(username, "AdminSetPassword");
    Ok(no_content())
}

/// Makes a user an administrator of the instance: the way back in when no
/// administrator can sign in any more.
async fn handle_admin_make_admin(
    State(app): State<App>,
    Path(username): Path<String>,
) -> ApiResult {
    let user = app.make_admin_by_username(&username).await?;
    tracing::info!(user_id = %user.id, "made an administrator through the administration socket");
    Ok(no_content())
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;

    use axum::body::Body;
    use axum::http::StatusCode;
    use tower::ServiceExt as _;

    use super::*;

    fn guarded() -> Router {
        Router::new()
            .route("/guarded", post(|| async { "reached" }))
            .route_layer(middleware::from_fn(require_unix_connection))
    }

    async fn status_for(peer: Option<Peer>) -> (StatusCode, String) {
        let mut request = Request::post("/guarded")
            .body(Body::empty())
            .expect("a valid request");
        if let Some(peer) = peer {
            request.extensions_mut().insert(ConnectInfo(peer));
        }
        let response = guarded()
            .oneshot(request)
            .await
            .expect("the router does not fail");
        let status = response.status();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("the body is read");
        (status, String::from_utf8_lossy(&body).into_owned())
    }

    /// `adminRequired` of `server/api/auth.go`: only a Unix socket connection passes.
    #[tokio::test]
    async fn admin_required_lets_only_unix_connections_through() {
        let (status, body) = status_for(Some(Peer::Unix)).await;
        assert_eq!(status, StatusCode::OK, "a Unix socket connection: {body}");
        assert_eq!(body, "reached");

        let tcp: SocketAddr = "127.0.0.1:4000".parse().expect("a socket address");
        for peer in [Some(Peer::Tcp(tcp)), None] {
            let (status, body) = status_for(peer).await;
            assert_eq!(status, StatusCode::NOT_FOUND, "{peer:?}: {body}");
            assert_eq!(
                body, r#"{"error":"not_found","message":"route not found"}"#,
                "{peer:?}"
            );
        }
    }
}
