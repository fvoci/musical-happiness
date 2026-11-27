//! A write of a container's tree, the pages of a wiki or the chapters and units of a
//! course: made under the container's structure barrier, and told of once stored.

use fvoci_model::{Block, Container};
use fvoci_store::{StoreError, Written};

use super::App;
use super::blocks::{ChangeAction, ChangeNotification, log_notification};

impl App {
    /// Writes the tree of `container` as `write` does, under the container's structure
    /// barrier, and tells websocket listeners and webhooks in the background of what it
    /// wrote, in the order [`Written::into_blocks`] gives. Returns the block the write
    /// was about.
    ///
    /// # Errors
    ///
    /// Returns the store's error of `write`, for the caller to say as its route does;
    /// nothing is told of a write that failed.
    pub(super) async fn write_tree(
        &self,
        container: &Container,
        action: ChangeAction,
        write: impl AsyncFnOnce() -> Result<Written, StoreError>,
    ) -> Result<Block, StoreError> {
        let written = {
            let _structure = self.collab().changing_structure(self, &container.id).await;
            write().await?
        };
        log_notification(ChangeNotification::Send, action, &written.first.id);
        let first = written.first.clone();
        self.announce(&container.workspace_id, written.into_blocks().collect());
        Ok(first)
    }

    /// Tells websocket listeners and webhooks in the background of the blocks, in turn.
    pub(super) fn announce(&self, workspace_id: &str, told: Vec<Block>) {
        let app = self.clone();
        let workspace_id = workspace_id.to_owned();
        self.spawn_background(async move {
            for block in &told {
                app.ws().broadcast_block_change(&workspace_id, block).await;
                app.tell_webhooks(block);
            }
        });
    }
}
