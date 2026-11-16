//! Public sharing of a board.

use axum::body::Bytes;
use axum::extract::Path;
use fvoci_model::{RefusalReason, Refusals, Sharing};
use fvoci_store::SharingChange;
use serde_json::Value;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::contract::bodies::SharingRequest;
use super::contract::params::{ContainerId, MemberWorkspaceId};
use super::contract::tag;
use super::workspace::InContainer;

use super::{ApiResult, json_response};
use super::{decode_json, json_object, require_container_permission};
use crate::app::App;
use crate::error::{AppError, Reason};
use crate::permissions::ContainerPermission;

pub(super) fn routes() -> OpenApiRouter<App> {
    OpenApiRouter::new().routes(routes!(handle_post_sharing, handle_get_sharing))
}

/// Checks that the user may share the board: its admins may. The board must still be
/// there.
async fn require_share_permission(
    app: &App,
    user_id: &str,
    container_id: &str,
) -> Result<(), AppError> {
    require_container_permission(
        app,
        user_id,
        container_id,
        ContainerPermission::Share,
        "access denied to sharing the board",
    )
    .await?;
    app.container(container_id).await?;
    Ok(())
}

/// Whether a board is shared
///
/// Whether the board is shared, and with which read token. A board that was never
/// shared is not shared. For the board's admins.
#[utoipa::path(
    get,
    path = "/w/{workspaceId}/containers/{containerId}/sharing",
    operation_id = "getSharing",
    tag = tag::SHARING,
    params(MemberWorkspaceId, ContainerId),
    responses((status = 200, description = "The sharing state.", body = Sharing)),
)]
async fn handle_get_sharing(
    InContainer(ws): InContainer,
    Path((_, container_id)): Path<(String, String)>,
) -> ApiResult {
    require_share_permission(&ws.app, &ws.session.user_id, &container_id).await?;
    let sharing = ws.app.sharing(&container_id).await?;
    tracing::debug!(container_id, enabled = sharing.enabled, "GET sharing");
    Ok(json_response(&sharing))
}

/// Reads the body of `POST /containers/{containerId}/sharing`: `enabled`, a boolean, and
/// optionally `regenerate`, a boolean. A `token` is refused, `null` too: the server
/// makes it.
fn sharing_change(body: &Bytes) -> Result<SharingChange, AppError> {
    let mut members = json_object(decode_json(body)?)?;
    // Every member that is refused, not the first alone.
    let mut refused = Refusals::new();
    if members.contains_key("token") {
        refused.refuse(
            "token",
            RefusalReason::Invalid,
            "the server makes the read token; a request cannot set it",
        );
    }
    members.retain(|_, member| !member.is_null());
    let mut flag = |name: &'static str, absent: Option<bool>| {
        let (read, reason) = match members.get(name) {
            Some(Value::Bool(flag)) => (Some(*flag), RefusalReason::Invalid),
            None | Some(Value::Null) => (absent, RefusalReason::Required),
            Some(_) => (None, RefusalReason::Invalid),
        };
        if read.is_none() {
            refused.refuse(name, reason, format!("{name} must be true or false"));
        }
        // What a refused member is read as is never used: the request is refused.
        read.unwrap_or_default()
    };
    let enabled = flag("enabled", None);
    let regenerate = flag("regenerate", Some(false));
    refused.into_result()?;
    Ok(match (enabled, regenerate) {
        (false, _) => SharingChange::Off,
        (true, false) => SharingChange::On,
        (true, true) => SharingChange::OnWithNewToken,
    })
}

/// Share a board, or stop sharing it
///
/// Shares the board, shares it under a new read token, or stops sharing it. The server
/// makes the token; a request that names one is 400. Where the server is configured
/// without public sharing, every request is refused with 403 and the reason
/// `sharing_disabled`, also one that stops sharing. While the organization has its
/// public links off (`patchOrganization`), sharing a board is refused with 403 and the
/// reason `public_links_off`; stopping it is not. For the board's admins.
#[utoipa::path(
    post,
    path = "/w/{workspaceId}/containers/{containerId}/sharing",
    operation_id = "setSharing",
    tag = tag::SHARING,
    params(MemberWorkspaceId, ContainerId),
    request_body = SharingRequest,
    responses((
        status = 200,
        description = "The sharing state as it is now, with the token while the board is shared.",
        body = Sharing,
    )),
)]
async fn handle_post_sharing(
    InContainer(ws): InContainer,
    Path((_, container_id)): Path<(String, String)>,
    body: Bytes,
) -> ApiResult {
    require_share_permission(&ws.app, &ws.session.user_id, &container_id).await?;

    if !ws.app.config().enable_public_shared_boards {
        tracing::warn!(
            container_id,
            user_id = %ws.session.user_id,
            "Attempt to turn on sharing for board via API failed, sharing off in configuration."
        );
        return Err(
            AppError::forbidden("public sharing is turned off on this server")
                .with_reason(Reason::SharingDisabled),
        );
    }

    let change = sharing_change(&body)?;
    let sharing = ws
        .app
        .set_sharing(&container_id, change, &ws.session.user_id)
        .await?;
    tracing::debug!(container_id, enabled = sharing.enabled, "POST sharing");
    Ok(json_response(&sharing))
}
