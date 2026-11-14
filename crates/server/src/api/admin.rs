//! The administration API: setting a user's password without knowing the old one, which
//! also ends every session of that user, making a user an administrator of the
//! instance, releasing an organization from taking no password, and comparing the file
//! storage with the records.
//!
//! It is served on the local Unix socket only, never on the TCP listener, so that being
//! able to open the socket file is the credential. There is no session and no cross-site
//! request forgery header, as in the original (`server/api/api.go` `RegisterAdminRoutes`).

use axum::Router;
use axum::body::Bytes;
use axum::extract::{Path, Request, State};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use serde::Deserialize;

use super::{ApiResult, decode_json, json_response, no_content};
use crate::Peer;
use crate::app::{App, Unrecorded};
use crate::auth::now_millis;
use crate::error::AppError;

pub(super) fn routes() -> Router<App> {
    Router::new()
        .route(
            &format!("{}/admin/users/{{username}}/password-reset", super::PREFIX),
            post(handle_password_reset_link),
        )
        .route(
            &format!("{}/admin/users/{{username}}/password", super::PREFIX),
            post(handle_admin_set_password),
        )
        .route(
            &format!("{}/admin/users/{{username}}/admin", super::PREFIX),
            post(handle_admin_make_admin),
        )
        .route(
            &format!("{}/admin/files/reconcile", super::PREFIX),
            post(handle_reconcile_files),
        )
        .route(
            &format!("{}/admin/orgs/{{orgId}}/sso/release", super::PREFIX),
            post(handle_release_sso),
        )
        .route_layer(middleware::from_fn(require_unix_connection))
        // The socket speaks the API's language for what it does not have as well.
        .fallback(|| async { AppError::not_found("route") })
        .method_not_allowed_fallback(|| async { AppError::method_not_allowed() })
}

/// Makes a single-use link for an operator to deliver without SMTP.
async fn handle_password_reset_link(
    State(app): State<App>,
    Path(username): Path<String>,
) -> ApiResult {
    let url = app.password_reset_link(&username).await?;
    Ok(json_response(&serde_json::json!({"url": url})))
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
    if Peer::of(&request) == Some(Peer::Unix) {
        next.run(request).await
    } else {
        // For anyone who does not come through the socket these routes do not exist.
        AppError::not_found("route").into_response()
    }
}

/// Sets a user's password and ends their sessions; recorded as the operator's in each
/// organization the user is a member of, and as every command is in the log.
async fn handle_admin_set_password(
    State(app): State<App>,
    Path(username): Path<String>,
    body: Bytes,
) -> ApiResult {
    let user_id = app.operator_user_id(&username).await;
    let result = async {
        // A body of `null` decodes to nothing, as in the original.
        let data: Option<AdminSetPasswordData> = decode_json(&body)?;
        let password = data.and_then(|data| data.password).unwrap_or_default();
        fvoci_model::check_new_password(&password)?;
        app.reset_user_password(&username, &password).await?;
        Ok(no_content())
    }
    .await;
    record_operator(Command::ResetPassword(user_id.as_deref()), result)
}

/// Makes a user an administrator of the instance: the way back in when no
/// administrator can sign in any more.
async fn handle_admin_make_admin(
    State(app): State<App>,
    Path(username): Path<String>,
) -> ApiResult {
    let (user_id, result) = match app.make_admin_by_username(&username).await {
        Ok(user) => (Some(user.id), Ok(no_content())),
        Err(error) => (app.operator_user_id(&username).await, Err(error)),
    };
    record_operator(Command::MakeAdmin(user_id.as_deref()), result)
}

/// Releases an organization from taking no password (`ssoRequired` of its provider):
/// the way back in when nobody gets in through the provider, which an administrator
/// of the organization could then not switch off either. The provider stays as it is
/// otherwise, and the release is recorded. 204 also for an organization that takes a
/// password already; 404 for an identifier that names no organization in use.
async fn handle_release_sso(State(app): State<App>, Path(org_id): Path<String>) -> ApiResult {
    let result = app
        .release_sso_as_operator(&org_id)
        .await
        .map(|()| no_content());
    let org_id = fvoci_model::is_id(&org_id).then_some(org_id.as_str());
    record_operator(Command::ReleaseSso(org_id), result)
}

/// The body of a request to compare the file storage with the records; none for a
/// report alone.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReconcileFilesData {
    /// Whether to doom the objects nothing names at places of the server's making that
    /// are older than a day, for the housekeeping to remove. Absent, false.
    #[serde(default)]
    remove_unrecorded: bool,
}

/// Compares the file storage with the records of every organization: the objects
/// nothing names, at places of the server's making and elsewhere, and the places
/// records name where the storage holds nothing. Without a body, or with
/// `removeUnrecorded` false, it changes nothing. With it true, the objects nothing
/// names at places of the server's making that are older than a day are doomed: the
/// housekeeping removes them at its next run, asking again whether something holds
/// them. The others, and a record without its object, are reported and never changed.
/// The storage (`filespath`, or the bucket under `filess3config.pathprefix`) is taken
/// to be this database's alone: another server's objects there are named by nothing
/// here.
async fn handle_reconcile_files(State(app): State<App>, body: Bytes) -> ApiResult {
    let result = async {
        let data: Option<ReconcileFilesData> = if body.is_empty() {
            None
        } else {
            decode_json(&body)?
        };
        let unrecorded = if data.is_some_and(|data| data.remove_unrecorded) {
            Unrecorded::Doom
        } else {
            Unrecorded::Report
        };
        let report = app
            .housekeeping()
            .reconcile_files(unrecorded, now_millis())
            .await?;
        Ok(json_response(&report))
    }
    .await;
    record_operator(Command::ReconcileFiles, result)
}

/// A command of the operator, with what it was about, as its line in the log names
/// them.
#[derive(Debug, Clone, Copy)]
enum Command<'a> {
    /// Setting a user's password: the user, by identifier where the name was one's.
    ResetPassword(Option<&'a str>),
    /// Making a user an administrator of the instance: the user, as above.
    MakeAdmin(Option<&'a str>),
    /// Releasing an organization from taking no password: the organization, by
    /// identifier where the path held one.
    ReleaseSso(Option<&'a str>),
    /// Comparing the file storage with the records: the whole server.
    ReconcileFiles,
}

impl<'a> Command<'a> {
    /// The command's name in the log.
    const fn name(self) -> &'static str {
        match self {
            Self::ResetPassword(_) => "reset_password",
            Self::MakeAdmin(_) => "make_admin",
            Self::ReleaseSso(_) => "release_sso",
            Self::ReconcileFiles => "reconcile_files",
        }
    }

    /// The user it was about, and the organization; each empty for none.
    fn target(self) -> (&'a str, &'a str) {
        match self {
            Self::ResetPassword(user_id) | Self::MakeAdmin(user_id) => {
                (user_id.unwrap_or_default(), "")
            }
            Self::ReleaseSso(org_id) => ("", org_id.unwrap_or_default()),
            Self::ReconcileFiles => ("", ""),
        }
    }
}

/// Writes one line of the log for each command the socket carried out, failures of
/// validation and hashing included: the command, its target, and its outcome. Never
/// what was sent with it. The changes an organization holds are in its records too.
fn record_operator(command: Command<'_>, result: ApiResult) -> ApiResult {
    let status = match &result {
        Ok(response) => response.status().as_u16(),
        Err(error) => error.status(),
    };
    let (user_id, org_id) = command.target();
    tracing::info!(
        target: "audit",
        actor = "operator",
        command = command.name(),
        user_id,
        org_id,
        result = if result.is_ok() { "succeeded" } else { "failed" },
        status,
    );
    result
}

#[cfg(test)]
mod tests {
    use axum::extract::ConnectInfo;

    use crate::connection::Connection;
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
            request.extensions_mut().insert(ConnectInfo(Connection {
                peer,
                breaker: None,
            }));
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
                body,
                r#"{"type":"/problems/not-found","title":"Not found","status":404,"detail":"route not found"}"#,
                "{peer:?}"
            );
        }
    }
}
