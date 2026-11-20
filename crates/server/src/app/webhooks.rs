//! The one door to the server operator's webhooks (`block_change_webhooks`): what is
//! posted there leaves the server, to an address its operator chose. A block of an
//! organization people work in is posted when it changes; nothing of an account's own
//! space is, which is its owner's alone.

use fvoci_model::Block;

use super::App;

impl App {
    /// Posts the block, which is of the organization this handle works in, to the
    /// operator's webhooks, one after the other; nothing if the organization is the
    /// space of an account, or if the server cannot tell whose the block is. For the
    /// background: a request does not wait for it.
    pub(crate) async fn tell_webhooks(&self, block: &Block) {
        if self.config().block_change_webhooks.is_empty() {
            return;
        }
        match self.store().is_own_space().await {
            Ok(false) => self.inner.webhook.notify_update(block).await,
            Ok(true) => {}
            Err(error) => {
                tracing::warn!(
                    block_id = %block.id, %error,
                    "a change was not posted to the webhooks: whose it is could not be read"
                );
            }
        }
    }
}
