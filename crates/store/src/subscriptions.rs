//! Subscriptions of users to the changes of a block.
//!
//! A subscription is a row while it lasts. It ends when the subscriber ends it, when the
//! block is deleted, and when the subscriber leaves the block's board.

use fvoci_model::{BlockType, Subscriber, SubscriberType, Subscription};

use crate::Store;
use crate::db::{Id, Row, Scope, Sql, now_millis};
use crate::error::StoreError;

/// The columns of a subscription, in the order [`subscription_from_row`] reads them.
const SUBSCRIPTION_COLUMNS: &str =
    "block_type, block_id, subscriber_type, subscriber_id, notified_at, create_at";

impl Store {
    /// Subscribes a subscriber to a block and returns the subscription as it is stored:
    /// created and notified now.
    ///
    /// If the subscriber is subscribed to the block already, that subscription stays as
    /// it is, creation time and types included, and is marked as notified now; that one
    /// is returned.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::InvalidSubscription`] if the subscription fails
    /// [`Subscription::validate`].
    pub async fn create_subscription(
        &self,
        subscription: Subscription,
    ) -> Result<Subscription, StoreError> {
        self.transaction(async |scope| create_subscription(scope, subscription).await)
            .await
    }

    /// Ends the subscription of a subscriber to a block.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NotFound`] if the subscriber is not subscribed to the block.
    pub async fn delete_subscription(
        &self,
        block_id: &str,
        subscriber_id: &str,
    ) -> Result<(), StoreError> {
        self.with_conn(async |scope| {
            let mut sql = scope.sql("DELETE FROM subscriptions WHERE block_id = ");
            sql.bind(Id(block_id))
                .push_bind(" AND subscriber_id = ", Id(subscriber_id));
            if scope.execute(sql).await? == 0 {
                return Err(subscription_not_found(block_id, subscriber_id));
            }
            Ok(())
        })
        .await
    }

    /// Returns the subscription of a subscriber to a block.
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

    /// Returns the subscriptions of a subscriber, oldest first.
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
            sql.push_bind(" AND subscriber_id = ", Id(subscriber_id))
                .push(" ORDER BY create_at, block_id");
            fetch_subscriptions(scope, sql).await
        })
        .await
    }

    /// Returns the subscribers of a block, the one notified longest ago first; those
    /// notified at the same time come by identifier.
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
                 WHERE block_id = ",
            );
            sql.bind(Id(block_id))
                .push(" ORDER BY notified_at, subscriber_id");
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
            let mut sql =
                scope.sql("SELECT COUNT(subscriber_id) FROM subscriptions WHERE block_id = ");
            sql.bind(Id(block_id));
            match scope.fetch_optional(sql).await? {
                Some(row) => row.int(0),
                None => Ok(0),
            }
        })
        .await
    }
}

/// Starts the query for subscriptions; conditions follow with `AND`.
fn select_subscriptions(scope: &Scope<'_>) -> Sql {
    scope.sql(&format!(
        "SELECT {SUBSCRIPTION_COLUMNS} FROM subscriptions WHERE 1 = 1"
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
        // A subscription that exists is not ended.
        delete_at: 0,
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

/// Ends every subscription of a subscriber to a block of the board.
pub(crate) async fn end_subscriptions_to_board(
    scope: &mut Scope<'_>,
    board_id: &str,
    subscriber_id: &str,
) -> Result<(), StoreError> {
    let mut sql = scope.sql("DELETE FROM subscriptions WHERE subscriber_id = ");
    sql.bind(Id(subscriber_id))
        .push_bind(
            " AND block_id IN (SELECT id FROM blocks WHERE board_id = ",
            Id(board_id),
        )
        .push(")");
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

    // Subscribing again to what one is subscribed to keeps the subscription as it is,
    // creation time and types included, and marks it as notified now.
    let mut sql = scope.sql(&format!(
        "INSERT INTO subscriptions ({SUBSCRIPTION_COLUMNS}) VALUES ("
    ));
    sql.bind(subscription.block_type.as_str())
        .push_bind(", ", Id(&subscription.block_id))
        .push_bind(", ", subscription.subscriber_type.as_str())
        .push_bind(", ", Id(&subscription.subscriber_id))
        .push_bind(", ", subscription.notified_at)
        .push_bind(", ", subscription.create_at)
        .push(
            ") ON CONFLICT (block_id, subscriber_id) DO UPDATE SET \
             notified_at = EXCLUDED.notified_at",
        );
    scope
        .execute(sql)
        .await
        .map_err(|error| error.missing_reference("block or subscriber"))?;

    let mut sql = select_subscriptions(scope);
    sql.push_bind(" AND block_id = ", Id(&subscription.block_id))
        .push_bind(" AND subscriber_id = ", Id(&subscription.subscriber_id));
    fetch_subscriptions(scope, sql)
        .await?
        .into_iter()
        .next()
        .ok_or_else(|| subscription_not_found(&subscription.block_id, &subscription.subscriber_id))
}
