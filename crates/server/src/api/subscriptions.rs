//! Subscriptions to blocks.

use axum::body::Bytes;
use axum::extract::Path;
use fvoci_model::{BlockType, RefusalReason, SubscriberType, Subscription, SubscriptionError};
use serde::Deserialize;
use serde_json::Value;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::contract::bodies::NewSubscription;
use super::contract::params::MemberWorkspaceId;
use super::contract::params::{BlockId, SubscriberId};
use super::contract::tag;
use super::workspace::InWorkspace;

use super::decode_object_members;
use super::require_permission_through;
use super::{ApiResult, created, json_response, no_content};
use crate::app::App;
use crate::error::AppError;
use crate::permissions::ContainerPermission;

/// The refusal of this area, whatever is refused.
const ACCESS_DENIED: &str = "access denied";

/// What a request to end a subscription misses.
const SUBSCRIPTION: &str = "subscription";

pub(super) fn routes() -> OpenApiRouter<App> {
    // The first position after `/subscriptions` is a block in one route and a
    // subscriber in the other. A router position has one name, so it gets a neutral
    // one, and each handler reads it as what it is. The contract names each as what it
    // is: see `contract::RENAMED_PATHS`.
    OpenApiRouter::new()
        .routes(routes!(handle_create_subscription))
        .routes(routes!(handle_delete_subscription))
        .routes(routes!(handle_get_subscriptions))
}

/// A subscription as a request body carries it.
///
/// The model's subscription holds a subscriber type that exists, so decoding into it
/// would refuse an unknown one as malformed JSON, before the other members are looked
/// at. Any string is decoded here and refused as the last step of the validation, in
/// [`SubscriptionRequest::into_subscription`].
#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct SubscriptionRequest {
    block_type: BlockType,
    block_id: String,
    subscriber_type: String,
    subscriber_id: String,
    notified_at: i64,
    create_at: i64,
    delete_at: i64,
}

impl SubscriptionRequest {
    /// Decodes the body: a member that is `null` or missing leaves the default. A body
    /// that is no JSON object, `null` included, or whose members have the wrong types is
    /// a bad request.
    fn decode(body: &[u8]) -> Result<Self, AppError> {
        let members = decode_object_members(body)?;
        serde_json::from_value(Value::Object(members)).map_err(AppError::bad_request)
    }

    /// Validates in the original's order (server/model/subscription.go `IsValid`):
    /// block identifier, block type, subscriber identifier, subscriber type.
    fn into_subscription(self) -> Result<Subscription, AppError> {
        let subscriber_type = match self.subscriber_type.as_str() {
            "user" => Some(SubscriberType::User),
            // The original also takes `channel`, for the channels of its host
            // application. There are none here.
            _ => None,
        };
        let subscription = Subscription {
            block_type: self.block_type,
            block_id: self.block_id,
            // A stand-in until the type is checked below, after the other members.
            subscriber_type: subscriber_type.unwrap_or(SubscriberType::User),
            subscriber_id: self.subscriber_id,
            notified_at: self.notified_at,
            create_at: self.create_at,
            delete_at: self.delete_at,
        };
        // Every member that is refused, the type among them.
        let mut refused = subscription
            .validate()
            .err()
            .map(SubscriptionError::into_refusals)
            .unwrap_or_default();
        if subscriber_type.is_none() {
            let reason = if self.subscriber_type.is_empty() {
                RefusalReason::Required
            } else {
                RefusalReason::Invalid
            };
            refused.refuse("subscriberType", reason, "invalid subscriber type");
        }
        refused.into_result()?;
        Ok(subscription)
    }
}

/// Subscribe to a block
///
/// Subscribes the user of the session to a block of a board they are a member of. A
/// `subscriberId` that is not the user of the session is 403, and a `subscriberType`
/// other than `user` is 400. The `blockType` is stored as it is sent, and not compared
/// with the block. Subscribing again is no error: the subscription stays as it was
/// made, with its `blockType` and `createAt`, and only its `notifiedAt` is set anew.
#[utoipa::path(
    post,
    path = "/w/{workspaceId}/subscriptions",
    params(MemberWorkspaceId),
    operation_id = "createSubscription",
    tag = tag::SUBSCRIPTIONS,
    request_body = NewSubscription,
    responses((status = 201, description = "The subscription.", body = Subscription)),
)]
async fn handle_create_subscription(ws: InWorkspace, body: Bytes) -> ApiResult {
    let subscription = SubscriptionRequest::decode(&body)?.into_subscription()?;
    if ws.session.user_id != subscription.subscriber_id {
        return Err(
            AppError::forbidden("a user subscribes for themselves only").with_field("subscriberId")
        );
    }
    // The request names a block and no board: a block on a board the user may not read
    // does not exist for them.
    let block = ws.app.block(&subscription.block_id).await?;
    require_permission_through(
        &ws.app,
        &ws.session.user_id,
        &block.container_id,
        ContainerPermission::View,
        ACCESS_DENIED,
        "block",
    )
    .await?;

    let subscription = ws.app.create_subscription(subscription).await?;
    tracing::debug!(
        subscriber_id = %subscription.subscriber_id,
        block_id = %subscription.block_id,
        "CREATE subscription"
    );
    Ok(created(&subscription))
}

/// End a subscription
///
/// Ends the subscription of the user of the session to a block. Nothing about the
/// block is checked: a subscription to a block the user may no longer see can still be
/// ended. A subscription that is not there is 404, and so are those of another user.
#[utoipa::path(
    delete,
    path = "/w/{workspaceId}/subscriptions/{blockOrSubscriberId}/{subscriberId}",
    operation_id = "deleteSubscription",
    tag = tag::SUBSCRIPTIONS,
    params(MemberWorkspaceId, BlockId, SubscriberId),
    responses((status = 204, description = "The subscription is ended.")),
)]
async fn handle_delete_subscription(
    ws: InWorkspace,
    Path((_, block_id, subscriber_id)): Path<(String, String, String)>,
) -> ApiResult {
    // The subscriptions of another user are nobody's business: not even that there is
    // one is said.
    if ws.session.user_id != subscriber_id {
        return Err(AppError::not_found(SUBSCRIPTION));
    }
    ws.app
        .delete_subscription(&block_id, &subscriber_id)
        .await
        .map_err(|error| {
            if error.is_not_found() {
                AppError::not_found(SUBSCRIPTION)
            } else {
                error
            }
        })?;
    tracing::debug!(block_id, subscriber_id, "DELETE subscription");
    Ok(no_content())
}

/// The subscriptions of the user
///
/// The subscriptions of the user of the session; those of another user are not there.
/// Oldest first, and by the identifier of the block among those made in the same
/// millisecond.
#[utoipa::path(
    get,
    path = "/w/{workspaceId}/subscriptions/{blockOrSubscriberId}",
    operation_id = "getSubscriptions",
    tag = tag::SUBSCRIPTIONS,
    params(MemberWorkspaceId, SubscriberId),
    responses((status = 200, description = "The subscriptions.", body = Vec<Subscription>)),
)]
async fn handle_get_subscriptions(
    ws: InWorkspace,
    Path((_, subscriber_id)): Path<(String, String)>,
) -> ApiResult {
    if ws.session.user_id != subscriber_id {
        return Err(AppError::not_found("subscriber"));
    }
    let subscriptions = ws.app.subscriptions(&subscriber_id).await?;
    tracing::debug!(
        subscriber_id,
        count = subscriptions.len(),
        "GET subscriptions"
    );
    Ok(json_response(&subscriptions))
}
