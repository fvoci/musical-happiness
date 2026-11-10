//! Subscriptions to blocks.

use axum::Router;
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::routing::{delete, get, post};
use fvoci_model::{BlockType, SubscriberType, Subscription};
use serde::Deserialize;
use serde_json::Value;

use super::decode_object_members;
use super::require_permission_through;
use super::{ApiResult, RequiredSession, empty_response, json_response};
use crate::app::App;
use crate::error::AppError;
use crate::permissions::BoardPermission;

/// The refusal of this area, whatever is refused.
const ACCESS_DENIED: &str = "access denied";

/// What a request to end a subscription misses.
const SUBSCRIPTION: &str = "subscription";

pub(super) fn routes() -> Router<App> {
    // The first position after `/subscriptions` is a block in one route and a
    // subscriber in the other. A router position has one name, so it gets a neutral
    // one, and each handler reads it as what it is.
    Router::new()
        .route("/subscriptions", post(handle_create_subscription))
        .route(
            "/subscriptions/{block_or_subscriber_id}/{subscriber_id}",
            delete(handle_delete_subscription),
        )
        .route(
            "/subscriptions/{block_or_subscriber_id}",
            get(handle_get_subscriptions),
        )
}

/// A subscription as a request body carries it.
///
/// The model's subscription holds a subscriber type that exists, so decoding into it
/// would refuse an unknown one as malformed JSON, which is a server error on this
/// route. The original decodes any string and refuses it as the last step of its
/// validation, as a bad request; so does [`SubscriptionRequest::into_subscription`].
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
    /// Decodes the body the way the original does: `null`, as the body or as a member,
    /// leaves the default. Any other malformed body is a server error, because the
    /// original does not classify the decoding error (server/api/subscriptions.go:60-63).
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
        subscription.validate().map_err(AppError::bad_request)?;
        if subscriber_type.is_none() {
            return Err(
                AppError::bad_request("invalid subscriber type").with_field("subscriberType")
            );
        }
        Ok(subscription)
    }
}

/// `POST /subscriptions`: subscribes the session's user to a block and answers with
/// the subscription. Subscribing again is no error.
///
/// The block must exist, and the user must have the permission to view its board, as
/// a member of it. The second check is not in the original, where every signed-in
/// user can subscribe to any block whose identifier they know
/// (server/api/subscriptions.go:84-96). A subscription is the standing request to be
/// told what changes on a block; that a standalone server tells subscribers nothing
/// does not make it everyone's to place.
async fn handle_create_subscription(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    body: Bytes,
) -> ApiResult {
    let subscription = SubscriptionRequest::decode(&body)?.into_subscription()?;
    if session.user_id != subscription.subscriber_id {
        return Err(
            AppError::forbidden("a user subscribes for themselves only").with_field("subscriberId")
        );
    }
    // The request names a block and no board: a block on a board the user may not read
    // does not exist for them.
    let block = app.block(&subscription.block_id).await?;
    require_permission_through(
        &app,
        &session.user_id,
        &block.board_id,
        BoardPermission::ViewBoard,
        ACCESS_DENIED,
        "block",
    )
    .await?;

    let subscription = app.create_subscription(subscription).await?;
    tracing::debug!(
        subscriber_id = %subscription.subscriber_id,
        block_id = %subscription.block_id,
        "CREATE subscription"
    );
    Ok(json_response(&subscription))
}

/// `DELETE /subscriptions/{block_id}/{subscriber_id}`: ends the subscription of the
/// session's user to a block. Nothing about the block is checked: a subscription to a
/// block that is gone, or that the user may no longer see, can still be ended.
async fn handle_delete_subscription(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    Path((block_id, subscriber_id)): Path<(String, String)>,
) -> ApiResult {
    // The subscriptions of another user are nobody's business: not even that there is
    // one is said.
    if session.user_id != subscriber_id {
        return Err(AppError::not_found(SUBSCRIPTION));
    }
    app.delete_subscription(&block_id, &subscriber_id)
        .await
        .map_err(|error| {
            if error.is_not_found() {
                AppError::not_found(SUBSCRIPTION)
            } else {
                error
            }
        })?;
    tracing::debug!(block_id, subscriber_id, "DELETE subscription");
    Ok(empty_response())
}

/// `GET /subscriptions/{subscriber_id}`: the subscriptions of the session's user.
async fn handle_get_subscriptions(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    Path(subscriber_id): Path<String>,
) -> ApiResult {
    if session.user_id != subscriber_id {
        return Err(AppError::not_found("subscriber"));
    }
    let subscriptions = app.subscriptions(&subscriber_id).await?;
    tracing::debug!(
        subscriber_id,
        count = subscriptions.len(),
        "GET subscriptions"
    );
    Ok(json_response(&subscriptions))
}
