//! Subscriptions of users to the changes of a block.
//!
//! A standalone server only keeps them. The original hands every change of a
//! subscription to its websocket adapter, and the adapter of the standalone server does
//! nothing with it (server/ws/server.go:785-787), so nothing is broadcast here. Nothing
//! reads the subscriptions to notify anyone either: the notification service of a
//! standalone server only writes a log line per block change.

use std::collections::HashMap;

use fvoci_model::Subscription;
use fvoci_store::WatchedBlock;

use super::App;
use crate::auth::now_millis;
use crate::error::AppError;

impl App {
    /// Subscribes a subscriber to a block and returns the subscription, created and
    /// last notified now.
    ///
    /// Subscribing again, also after unsubscribing, revives the stored subscription:
    /// it keeps its creation time and types, although the returned one carries the
    /// current time and the given types.
    ///
    /// # Errors
    ///
    /// Returns a bad request if the subscription has no block identifier, block type or
    /// subscriber identifier.
    pub async fn create_subscription(
        &self,
        subscription: Subscription,
    ) -> Result<Subscription, AppError> {
        Ok(self.store().create_subscription(subscription).await?)
    }

    /// Ends the subscription of a subscriber to a block and returns it, marked as
    /// deleted now. For a handle of one workspace (`App::in_workspace`) only one to a
    /// block of a board of that workspace is there.
    ///
    /// # Errors
    ///
    /// Returns a not-found error if the subscriber has no subscription to the
    /// block, or has ended it already.
    pub async fn delete_subscription(
        &self,
        block_id: &str,
        subscriber_id: &str,
    ) -> Result<Subscription, AppError> {
        // One to a block the subscriber no longer reads is ended all the same.
        if let Some(workspace_id) = self.workspace.as_deref()
            && !self
                .store()
                .get_subscriptions_in_workspace(subscriber_id, workspace_id)
                .await?
                .iter()
                .any(|subscription| subscription.block_id == block_id)
        {
            return Err(AppError::not_found("subscription"));
        }
        let mut subscription = self
            .store()
            .get_subscription(block_id, subscriber_id)
            .await?;
        self.store()
            .delete_subscription(block_id, subscriber_id)
            .await?;
        subscription.delete_at = now_millis();
        // The original looks up the block's board here to name a workspace for a broadcast
        // that its standalone websocket server does not send, and crashes if the block
        // is gone by now (server/app/subscriptions.go:43-51). There is nothing to look
        // up for nothing to send.
        Ok(subscription)
    }

    /// Returns the subscriptions of a subscriber that have not ended, oldest first: for a
    /// handle of one workspace (`App::in_workspace`), those to blocks of its boards.
    /// Only those to a block of a board or a wiki the subscriber reads now, or may join
    /// to read (`Grant::reads_on_joining`), as they are told of it: of any other they
    /// are not to know even the identifier.
    ///
    /// # Errors
    ///
    /// Returns the store's error if a query fails.
    pub async fn subscriptions(&self, subscriber_id: &str) -> Result<Vec<Subscription>, AppError> {
        let watched = self
            .store()
            .get_watched_blocks(subscriber_id, self.workspace.as_deref())
            .await?;
        let mut reads: HashMap<String, bool> = HashMap::new();
        let mut kept = Vec::with_capacity(watched.len());
        for WatchedBlock {
            subscription,
            container_id,
        } in watched
        {
            let reads_it = if let Some(reads_it) = reads.get(&container_id) {
                *reads_it
            } else {
                let reads_it = self
                    .container_access(subscriber_id, &container_id)
                    .await?
                    .is_some_and(|access| access.grant.reads_on_joining());
                reads.insert(container_id, reads_it);
                reads_it
            };
            if reads_it {
                kept.push(subscription);
            }
        }
        Ok(kept)
    }
}
