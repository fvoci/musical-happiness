//! Subscriptions of users to the changes of a block.
//!
//! Unsubscribing marks the subscription as deleted; subscribing again revives it.

use fvoci_model::{BlockType, Subscriber, SubscriberType, Subscription};

use crate::Store;
use crate::db::{Id, Row, Scope, Sql, now_millis};
use crate::error::StoreError;

/// The columns of a subscription, in the order [`subscription_from_row`] reads them.
const SUBSCRIPTION_COLUMNS: &str =
    "block_type, block_id, subscriber_type, subscriber_id, notified_at, create_at, delete_at";

impl Store {
    /// Subscribes a subscriber to a block and returns the subscription with the current
    /// time as creation and notification time, and not deleted.
    ///
    /// If the subscriber already has a subscription to the block, deleted or not, that
    /// one is revived and marked as notified now instead. It keeps its stored creation
    /// time and types, although the returned subscription carries the given types and
    /// the current time.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::InvalidSubscription`] if the subscription fails
    /// [`Subscription::validate`].
    pub async fn create_subscription(
        &self,
        subscription: Subscription,
    ) -> Result<Subscription, StoreError> {
        self.with_conn(async |scope| create_subscription(scope, subscription).await)
            .await
    }

    /// Marks the subscription of a subscriber to a block as deleted.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NotFound`] if the subscriber never subscribed to the block.
    pub async fn delete_subscription(
        &self,
        block_id: &str,
        subscriber_id: &str,
    ) -> Result<(), StoreError> {
        self.with_conn(async |scope| {
            // Preserved quirk: a subscription that is already deleted matches too, so
            // deleting it again succeeds and moves its deletion time
            // (`server/services/store/sqlstore/subscriptions.go:100-104`).
            let mut sql = scope.sql("UPDATE subscriptions SET delete_at = ");
            sql.bind(now_millis())
                .push_bind(" WHERE block_id = ", Id(block_id))
                .push_bind(" AND subscriber_id = ", Id(subscriber_id));
            if scope.execute(sql).await? == 0 {
                return Err(subscription_not_found(block_id, subscriber_id));
            }
            Ok(())
        })
        .await
    }

    /// Returns the subscription of a subscriber to a block, unless deleted.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NotFound`] if there is no such subscription.
    pub async fn get_subscription(
        &self,
        block_id: &str,
        subscriber_id: &str,
    ) -> Result<Subscription, StoreError> {
        self.with_conn(async |scope| {
            let mut sql = select_subscriptions(scope);
            sql.push_bind(" AND block_id = ", Id(block_id))
                .push_bind(" AND subscriber_id = ", Id(subscriber_id));
            fetch_subscriptions(scope, sql)
                .await?
                .into_iter()
                .next()
                .ok_or_else(|| subscription_not_found(block_id, subscriber_id))
        })
        .await
    }

    /// Returns the subscriptions of a subscriber that are not deleted, in no particular
    /// order.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the query fails.
    pub async fn get_subscriptions(
        &self,
        subscriber_id: &str,
    ) -> Result<Vec<Subscription>, StoreError> {
        self.with_conn(async |scope| {
            let mut sql = select_subscriptions(scope);
            sql.push_bind(" AND subscriber_id = ", Id(subscriber_id));
            fetch_subscriptions(scope, sql).await
        })
        .await
    }

    /// Returns the subscribers of a block, the one notified longest ago first.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the query fails.
    pub async fn get_subscribers_for_block(
        &self,
        block_id: &str,
    ) -> Result<Vec<Subscriber>, StoreError> {
        self.with_conn(async |scope| {
            let mut sql = scope.sql(
                "SELECT subscriber_type, subscriber_id, notified_at FROM subscriptions \
                 WHERE delete_at = 0 AND block_id = ",
            );
            sql.bind(Id(block_id)).push(" ORDER BY notified_at");
            scope
                .fetch_all(sql)
                .await?
                .iter()
                .map(|row| {
                    Ok(Subscriber {
                        subscriber_type: subscriber_type_from_text(&row.text(0)?)?,
                        subscriber_id: row.id(1)?,
                        notified_at: row.int(2)?,
                    })
                })
                .collect()
        })
        .await
    }

    /// Returns the number of subscribers of a block.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the query fails.
    pub async fn get_subscribers_count_for_block(&self, block_id: &str) -> Result<i64, StoreError> {
        self.with_conn(async |scope| {
            let mut sql = scope.sql(
                "SELECT COUNT(subscriber_id) FROM subscriptions WHERE delete_at = 0 AND block_id = ",
            );
            sql.bind(Id(block_id));
            match scope.fetch_optional(sql).await? {
                Some(row) => row.int(0),
                None => Ok(0),
            }
        })
        .await
    }
}

/// Starts the query for the subscriptions that are not deleted; conditions follow with
/// `AND`.
fn select_subscriptions(scope: &Scope<'_>) -> Sql {
    scope.sql(&format!(
        "SELECT {SUBSCRIPTION_COLUMNS} FROM subscriptions WHERE delete_at = 0"
    ))
}

async fn fetch_subscriptions(
    scope: &mut Scope<'_>,
    sql: Sql,
) -> Result<Vec<Subscription>, StoreError> {
    scope
        .fetch_all(sql)
        .await?
        .iter()
        .map(subscription_from_row)
        .collect()
}

fn subscription_from_row(row: &Row) -> Result<Subscription, StoreError> {
    Ok(Subscription {
        block_type: BlockType::from(row.text(0)?),
        block_id: row.id(1)?,
        subscriber_type: subscriber_type_from_text(&row.text(2)?)?,
        subscriber_id: row.id(3)?,
        notified_at: row.int(4)?,
        create_at: row.int(5)?,
        delete_at: row.int(6)?,
    })
}

fn subscriber_type_from_text(text: &str) -> Result<SubscriberType, StoreError> {
    [SubscriberType::User, SubscriberType::Channel]
        .into_iter()
        .find(|subscriber_type| subscriber_type.as_str() == text)
        .ok_or_else(|| StoreError::Corrupt {
            column: "subscriber_type",
            reason: format!("unknown subscriber type {text:?}"),
        })
}

fn subscription_not_found(block_id: &str, subscriber_id: &str) -> StoreError {
    StoreError::not_found(format_args!(
        "subscription BlockID={block_id} SubscriberID={subscriber_id}"
    ))
}

/// Marks as deleted every subscription of a subscriber to a block of the board. Blocks
/// that are deleted count too: they can be restored, and their subscriptions with them.
pub(crate) async fn end_subscriptions_to_board(
    scope: &mut Scope<'_>,
    board_id: &str,
    subscriber_id: &str,
) -> Result<(), StoreError> {
    let mut sql = scope.sql("UPDATE subscriptions SET delete_at = ");
    sql.bind(now_millis()).push_bind(
        " WHERE delete_at = 0 AND subscriber_id = ",
        Id(subscriber_id),
    );
    for (opening, blocks) in [(" AND (", "blocks"), (" OR ", "blocks_history")] {
        sql.push(&format!(
            "{opening}EXISTS (SELECT 1 FROM {blocks} WHERE {blocks}.id = subscriptions.block_id \
             AND {blocks}.board_id = "
        ))
        .bind(Id(board_id))
        .push(")");
    }
    sql.push(")");
    scope.execute(sql).await?;
    Ok(())
}

pub(crate) async fn create_subscription(
    scope: &mut Scope<'_>,
    subscription: Subscription,
) -> Result<Subscription, StoreError> {
    subscription.validate()?;

    // The notification time starts at now so that the first notification does not
    // report the block's whole history.
    let now = now_millis();
    let subscription = Subscription {
        notified_at: now,
        create_at: now,
        delete_at: 0,
        ..subscription
    };

    // Preserved quirk: on a conflict only the deletion and notification times are
    // written. The stored creation time and types stay, while the caller gets back the
    // given types and the current time
    // (`server/services/store/sqlstore/subscriptions.go:82`).
    let mut sql = scope.sql(&format!(
        "INSERT INTO subscriptions ({SUBSCRIPTION_COLUMNS}) VALUES ("
    ));
    sql.bind(subscription.block_type.as_str())
        .push_bind(", ", Id(&subscription.block_id))
        .push_bind(", ", subscription.subscriber_type.as_str())
        .push_bind(", ", Id(&subscription.subscriber_id))
        .push_bind(", ", subscription.notified_at)
        .push_bind(", ", subscription.create_at)
        .push_bind(", ", subscription.delete_at)
        .push(
            ") ON CONFLICT (block_id, subscriber_id) DO UPDATE SET delete_at = 0, \
             notified_at = EXCLUDED.notified_at",
        );
    scope.execute(sql).await?;
    Ok(subscription)
}
