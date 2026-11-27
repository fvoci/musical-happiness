//! Reactions to the comments, cards, pages and units of a container: adding one, taking
//! one off, reading them, and telling the container's members what they are now
//! (`reactionsChanged`). Nobody is notified of a reaction, and no webhook is told.

use std::collections::HashMap;

use fvoci_model::{Block, Emoji, REACTION_KINDS_PER_BLOCK, Reaction};
use fvoci_store::Reacted;

use super::App;
use crate::error::{AppError, Reason};

impl App {
    /// Adds the reaction `emoji` of `user_id` to `block`, of the workspace
    /// `workspace_id`, and tells the container's members if it was not there.
    ///
    /// # Errors
    ///
    /// Returns 400 with the reason `too_many_reactions` if the block has as many other
    /// emoji as a block may, 404 if the block is gone or the user is no longer in its
    /// workspace, and the store's error.
    pub(crate) async fn add_reaction(
        &self,
        workspace_id: &str,
        block: &Block,
        user_id: &str,
        emoji: &Emoji,
    ) -> Result<(), AppError> {
        match self.store().add_reaction(&block.id, user_id, emoji).await? {
            Reacted::Added => self.announce_reactions(workspace_id, block),
            Reacted::AlreadyThere => {}
            Reacted::TooManyKinds => return Err(too_many_reactions()),
        }
        Ok(())
    }

    /// Takes the reaction `emoji` of `user_id` off `block`, of the workspace
    /// `workspace_id`, and tells the container's members if it was there.
    ///
    /// # Errors
    ///
    /// Returns the store's error.
    pub(crate) async fn remove_reaction(
        &self,
        workspace_id: &str,
        block: &Block,
        user_id: &str,
        emoji: &Emoji,
    ) -> Result<(), AppError> {
        if self
            .store()
            .remove_reaction(&block.id, user_id, emoji)
            .await?
        {
            self.announce_reactions(workspace_id, block);
        }
        Ok(())
    }

    /// The reactions to those of `block_ids` that have any, by block
    /// ([`fvoci_store::Store::reactions_of`]).
    ///
    /// # Errors
    ///
    /// Returns the store's error.
    pub(crate) async fn reactions_of(
        &self,
        block_ids: &[&str],
    ) -> Result<HashMap<String, Vec<Reaction>>, AppError> {
        Ok(self.store().reactions_of(block_ids).await?)
    }

    /// The reactions to `block_id`, the emoji it was first answered with first.
    ///
    /// # Errors
    ///
    /// Returns the store's error.
    pub(crate) async fn reactions_to(&self, block_id: &str) -> Result<Vec<Reaction>, AppError> {
        let mut reactions = self.reactions_of(&[block_id]).await?;
        Ok(reactions.remove(block_id).unwrap_or_default())
    }

    /// Tells the members of the block's container its reactions in the background, as
    /// they are when they are told: what was read last is told last, so two changes
    /// made one after the other leave every listener with the second.
    fn announce_reactions(&self, workspace_id: &str, block: &Block) {
        let app = self.clone();
        let workspace_id = workspace_id.to_owned();
        let (container_id, block_id) = (block.container_id.clone(), block.id.clone());
        self.spawn_background(async move {
            let _turn = app.inner.telling_reactions.lock().await;
            let reactions = match app.reactions_to(&block_id).await {
                Ok(reactions) => reactions,
                Err(error) => {
                    tracing::error!(%error, block_id, "cannot read reactions to tell them");
                    return;
                }
            };
            app.ws()
                .broadcast_reactions_change(&workspace_id, &container_id, &block_id, &reactions)
                .await;
        });
    }
}

/// What a reaction with one emoji more than a block may have is answered with.
fn too_many_reactions() -> AppError {
    AppError::bad_request(format_args!(
        "a block is answered with at most {REACTION_KINDS_PER_BLOCK} different emoji"
    ))
    .with_reason(Reason::TooManyReactions)
    .with_limit(u64::try_from(REACTION_KINDS_PER_BLOCK).unwrap_or(u64::MAX))
}
