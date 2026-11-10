//! Public sharing of a board.

use axum::Router;
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::routing::post;
use fvoci_store::SharingChange;
use serde_json::Value;

use super::{ApiResult, RequiredSession, json_response};
use super::{decode_object_members, require_board_permission};
use crate::app::App;
use crate::error::{AppError, Reason};
use crate::permissions::BoardPermission;

pub(super) fn routes() -> Router<App> {
    Router::new().route(
        "/boards/{board_id}/sharing",
        post(handle_post_sharing).get(handle_get_sharing),
    )
}

/// Checks that the user may share the board: its admins may. The board must still be
/// there.
async fn require_share_permission(
    app: &App,
    user_id: &str,
    board_id: &str,
) -> Result<(), AppError> {
    require_board_permission(
        app,
        user_id,
        board_id,
        BoardPermission::ShareBoard,
        "access denied to sharing the board",
    )
    .await?;
    app.board(board_id).await?;
    Ok(())
}

/// `GET /boards/{board_id}/sharing`: whether the board is shared, and with which token.
/// A board that was never shared is not shared.
async fn handle_get_sharing(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    Path(board_id): Path<String>,
) -> ApiResult {
    require_share_permission(&app, &session.user_id, &board_id).await?;
    let sharing = app.sharing(&board_id).await?;
    tracing::debug!(board_id, enabled = sharing.enabled, "GET sharing");
    Ok(json_response(&sharing))
}

/// Reads the body of `POST /boards/{board_id}/sharing`: `enabled`, a boolean, and
/// optionally `regenerate`, a boolean. A `token` is refused: the server makes it.
fn sharing_change(body: &Bytes) -> Result<SharingChange, AppError> {
    let members = decode_object_members(body)?;
    if members.contains_key("token") {
        return Err(AppError::bad_request(
            "the server makes the read token; a request cannot set it",
        )
        .with_field("token"));
    }
    let flag = |name: &'static str, absent: Option<bool>| match members.get(name) {
        Some(Value::Bool(flag)) => Ok(*flag),
        None | Some(Value::Null) => absent.ok_or_else(|| {
            AppError::bad_request(format!("{name} must be true or false")).with_field(name)
        }),
        Some(_) => {
            Err(AppError::bad_request(format!("{name} must be true or false")).with_field(name))
        }
    };
    let enabled = flag("enabled", None)?;
    let regenerate = flag("regenerate", Some(false))?;
    Ok(match (enabled, regenerate) {
        (false, _) => SharingChange::Off,
        (true, false) => SharingChange::On,
        (true, true) => SharingChange::OnWithNewToken,
    })
}

/// `POST /boards/{board_id}/sharing`: shares the board, shares it under a new token, or
/// stops sharing it. Answers the sharing state, with the token while the board is
/// shared.
async fn handle_post_sharing(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    Path(board_id): Path<String>,
    body: Bytes,
) -> ApiResult {
    require_share_permission(&app, &session.user_id, &board_id).await?;

    if !app.config().enable_public_shared_boards {
        tracing::warn!(
            board_id,
            user_id = %session.user_id,
            "Attempt to turn on sharing for board via API failed, sharing off in configuration."
        );
        return Err(
            AppError::forbidden("public sharing is turned off on this server")
                .with_reason(Reason::SharingDisabled),
        );
    }

    let change = sharing_change(&body)?;
    let sharing = app.set_sharing(&board_id, change, &session.user_id).await?;
    tracing::debug!(board_id, enabled = sharing.enabled, "POST sharing");
    Ok(json_response(&sharing))
}
