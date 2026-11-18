//! Subscriptions of users to changes of a block.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::block::BlockType;
use crate::refusal::{RefusalReason, Refusals};

/// The kind of entity that subscribes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum SubscriberType {
    /// A user.
    User,
    /// A channel; unused by a standalone server, kept for the wire format.
    Channel,
}

impl SubscriberType {
    /// Returns the wire and database form.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Channel => "channel",
        }
    }
}

/// Why a [`Subscription`] is rejected before it is stored: every member that is
/// refused.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{0}")]
pub struct SubscriptionError(Refusals);

impl SubscriptionError {
    /// The members that were refused.
    pub fn into_refusals(self) -> Refusals {
        self.0
    }
}

/// A subscription to the changes of a board or card.
// `subscriberType` is required when decoding; every other member defaults to its empty
// value. (No doc comment: the doc comment is the description in the API contract.)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Subscription {
    /// The type of the block subscribed to.
    #[serde(default)]
    #[schema(value_type = String)]
    pub block_type: BlockType,
    /// The identifier of the block subscribed to.
    #[serde(default)]
    pub block_id: String,
    /// The kind of entity that subscribes.
    pub subscriber_type: SubscriberType,
    /// The identifier of the subscriber.
    #[serde(default)]
    pub subscriber_id: String,
    /// The time of the last notification sent for this subscription, in milliseconds
    /// since the Unix epoch.
    #[serde(default)]
    pub notified_at: i64,
    /// Creation time in milliseconds since the Unix epoch.
    #[serde(default)]
    pub create_at: i64,
    /// Deletion time in milliseconds since the Unix epoch; zero if not deleted.
    #[serde(default)]
    pub delete_at: i64,
}

impl Subscription {
    /// Checks that the subscription may be stored.
    ///
    /// # Errors
    ///
    /// Returns a [`SubscriptionError`] with every one of the block identifier, the block
    /// type and the subscriber identifier that is empty.
    pub fn validate(&self) -> Result<(), SubscriptionError> {
        let mut refused = Refusals::new();
        if self.block_id.is_empty() {
            refused.refuse("blockId", RefusalReason::Required, "missing block id");
        }
        if self.block_type.is_empty() {
            refused.refuse("blockType", RefusalReason::Required, "missing block type");
        }
        if self.subscriber_id.is_empty() {
            refused.refuse(
                "subscriberId",
                RefusalReason::Required,
                "missing subscriber id",
            );
        }
        refused.into_result().map_err(SubscriptionError)
    }
}

/// An entity subscribed to a block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Subscriber {
    /// The kind of entity that subscribes.
    pub subscriber_type: SubscriberType,
    /// The identifier of the subscriber.
    #[serde(default)]
    pub subscriber_id: String,
    /// The time the subscriber was last notified, in milliseconds since the Unix epoch.
    #[serde(default)]
    pub notified_at: i64,
}
