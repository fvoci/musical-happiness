//! The one door to the server operator's webhooks (`block_change_webhooks`): what is
//! posted there leaves the server, to an address its operator chose. A block of an
//! organization people work in is posted when it changes; nothing of an account's own
//! space is, which is its owner's alone.

use std::borrow::Cow;

use fvoci_model::{Block, OrgKind};

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
        let kind = self
            .org_kind
            .get_or_try_init(|| async {
                let own = self.store().is_own_space().await?;
                Ok::<_, fvoci_store::StoreError>(if own {
                    OrgKind::Personal
                } else {
                    OrgKind::Shared
                })
            })
            .await;
        match kind {
            Ok(OrgKind::Shared) => {
                let posted = if !block.takes_number() && block.number.is_some() {
                    let mut hidden = block.clone();
                    hidden.hide_inactive_number();
                    Cow::Owned(hidden)
                } else {
                    Cow::Borrowed(block)
                };
                self.inner.webhook.notify_update(&posted).await;
            }
            Ok(OrgKind::Personal) => {}
            Err(error) => {
                tracing::warn!(
                    block_id = %block.id, %error,
                    "a change was not posted to the webhooks: whose it is could not be read"
                );
            }
        }
    }
}
