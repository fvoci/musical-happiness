//! Public sharing of a board.

use axum::Router;
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::routing::post;
use fvoci_model::{SINGLE_USER_ID, Sharing};
use serde_json::Value;

use super::decode_object_members;
use super::{ApiResult, RequiredSession, empty_response, json_response};
use crate::app::App;
use crate::error::AppError;
use crate::permissions::BoardPermission;

pub(super) fn routes() -> Router<App> {
    Router::new().route(
        "/boards/{board_id}/sharing",
        post(handle_post_sharing).get(handle_get_sharing),
    )
}

async fn require_share_permission(
    app: &App,
    user_id: &str,
    board_id: &str,
) -> Result<(), AppError> {
    if app
        .has_permission_to_board(user_id, board_id, BoardPermission::ShareBoard)
        .await
    {
        Ok(())
    } else {
        Err(AppError::forbidden("access denied to sharing the board"))
    }
}

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

async fn handle_post_sharing(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    Path(board_id): Path<String>,
    body: Bytes,
) -> ApiResult {
    require_share_permission(&app, &session.user_id, &board_id).await?;

    // A `null` body leaves every member at its default, which turns sharing off.
    let members = decode_object_members(&body)?;
    let mut sharing: Sharing =
        serde_json::from_value(Value::Object(members)).map_err(AppError::internal)?;
    // The board is the one of the path and the modifier the one of the session, whatever
    // the body says. The token is the body's: the client chooses it.
    sharing.id = board_id;
    sharing.modified_by = if session.user_id == SINGLE_USER_ID {
        String::new()
    } else {
        session.user_id
    };

    if !app.config().enable_public_shared_boards {
        tracing::warn!(
            board_id = %sharing.id,
            user_id = %sharing.modified_by,
            "Attempt to turn on sharing for board via API failed, sharing off in configuration."
        );
        // Preserved quirk: the original does not classify this error, so the refusal is
        // a server error (server/api/sharing.go:16, :164).
        return Err(AppError::internal(
            "turning on sharing for board failed, see log for details",
        ));
    }

    app.upsert_sharing(&sharing).await?;
    tracing::debug!(board_id = %sharing.id, "POST sharing");
    Ok(empty_response())
}
