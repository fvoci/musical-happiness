//! Public sharing of a board.

use axum::Router;
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::routing::post;
use fvoci_model::{SINGLE_USER_ID, Sharing};
use serde_json::Value;

use super::{ApiResult, RequiredSession, empty_response, json_response};
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
    let sharing = match app.sharing(&board_id).await {
        Ok(sharing) => sharing,
        Err(error) if error.is_not_found() => Sharing {
            id: board_id.clone(),
            ..Sharing::default()
        },
        Err(error) => return Err(error),
    };
    tracing::debug!(board_id, enabled = sharing.enabled, "GET sharing");
    Ok(json_response(&sharing))
}

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

    let members = decode_object_members(&body)?;
    let mut sharing: Sharing =
        serde_json::from_value(Value::Object(members)).map_err(AppError::bad_request)?;
    // The board is the one of the path and the modifier the one of the session, whatever
    // the body says. The token is the body's: the client chooses it.
    sharing.id = board_id;
    sharing.modified_by = if session.user_id == SINGLE_USER_ID {
        String::new()
    } else {
        session.user_id
    };

    app.upsert_sharing(&sharing).await?;
    tracing::debug!(board_id = %sharing.id, "POST sharing");
    Ok(empty_response())
}
