//! Blocks: reading and changing them, and telling websocket listeners and webhooks.

use fvoci_model::{Block, BlockPatch, BlockPatchBatch, BlockType, CommentVersion};
use fvoci_store::{BlockFilter, DeletedBlock, DuplicateAs};

use super::App;
use crate::collab::Deletion;
use crate::error::AppError;

/// Whether the people a change to a block newly names (those its body mentions, those
/// a card is given to) are told of it. A client that writes again what was said before
/// (a copy, a change taken back or made again, via `disableNotify`) suppresses it while
/// its writer watches what it writes new all the same ([`Self::Suppress`]); a bulk copy
/// from another board (`sourceBoardId`) tells nobody and makes nobody watch
/// ([`Self::Copied`]).
///
/// Websocket broadcasts and webhooks are not notifications: they happen either way.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ChangeNotification {
    /// Report the change.
    #[default]
    Send,
    /// Do not report the change (`disableNotify`): tell nobody, while the writer still
    /// watches newly written cards, pages or comments.
    Suppress,
    /// Bulk copy from another board (`sourceBoardId`): tell nobody and make nobody watch.
    Copied,
}

/// What happened to a block, as the notification service names it.
#[derive(Debug, Clone, Copy)]
pub(super) enum ChangeAction {
    Add,
    Update,
    Delete,
}

impl ChangeAction {
    fn as_str(self) -> &'static str {
        match self {
            Self::Add => "add",
            Self::Update => "update",
            Self::Delete => "delete",
        }
    }
}

impl App {
    /// The database for a write of blocks that tells of itself as `notification`
    /// says: one that is suppressed tells nobody whom it newly names.
    fn store_telling(&self, notification: ChangeNotification) -> fvoci_store::Store {
        match notification {
            ChangeNotification::Send => self.store().clone(),
            ChangeNotification::Suppress => self.store().quietly(),
            ChangeNotification::Copied => self.store().telling_nobody(),
        }
    }

    /// Returns the blocks of a board, optionally only those with the given parent or of
    /// the given type, in no particular order.
    ///
    /// The type is compared as the stored string: [`BlockType::Other`] finds the blocks
    /// of a type the server does not know. The empty type does not filter. An empty
    /// board identifier finds nothing.
    ///
    /// # Errors
    ///
    /// Returns the store's error if the query fails.
    pub async fn blocks(
        &self,
        container_id: &str,
        parent_id: Option<&str>,
        block_type: Option<&BlockType>,
    ) -> Result<Vec<Block>, AppError> {
        if container_id.is_empty() {
            return Ok(Vec::new());
        }
        let mut filter = BlockFilter::container(container_id);
        if let Some(parent_id) = parent_id {
            filter = filter.with_parent(parent_id);
        }
        if let Some(block_type) = block_type {
            filter = filter.with_type(block_type.clone());
        }
        let mut blocks = self.store().get_blocks(&filter).await?;
        hide_inactive_numbers(&mut blocks);
        Ok(blocks)
    }

    /// Returns all blocks of a board, in no particular order.
    ///
    /// # Errors
    ///
    /// Returns the store's error if the query fails.
    pub async fn blocks_for_container(&self, container_id: &str) -> Result<Vec<Block>, AppError> {
        let mut blocks = self
            .store()
            .get_blocks(&BlockFilter::container(container_id))
            .await?;
        hide_inactive_numbers(&mut blocks);
        Ok(blocks)
    }

    /// Returns the block with the given identifier: only a block of a board of the kinds
    /// the handle reaches ([`App::reaching`]) and, for a handle of one workspace
    /// ([`App::in_workspace`]), of that workspace or of a built-in template.
    ///
    /// # Errors
    ///
    /// Returns a not-found error if there is no such block.
    pub async fn block(&self, block_id: &str) -> Result<Block, AppError> {
        // The handle finds the blocks of the boards it reaches only, in the one read.
        let found = self
            .store()
            .get_block_within(block_id, self.workspace.as_deref(), self.kinds)
            .await;
        match found {
            Ok(mut block) => {
                block.hide_inactive_number();
                Ok(block)
            }
            Err(error) if error.is_not_found() => Err(AppError::not_found("block")),
            Err(error) => Err(error.into()),
        }
    }

    /// Inserts a block, or updates the block that already has its identifier, and tells
    /// websocket listeners and webhooks in the background.
    ///
    /// `block` is updated as [`fvoci_store::Store::insert_block`] describes.
    ///
    /// # Errors
    ///
    /// Returns a not-found error if the block's board does not exist and
    /// a bad request if the title or the fields are too large.
    pub async fn insert_block(
        &self,
        block: &mut Block,
        modified_by: &str,
        notification: ChangeNotification,
    ) -> Result<(), AppError> {
        let container = self.store().get_container(&block.container_id).await?;
        if block.block_type.has_room() {
            // A card stored again writes its body whole, around its document.
            let (mut stored, modified_by) = (block.clone(), modified_by.to_owned());
            let card_ids = vec![block.id.clone()];
            *block = self
                .collab()
                .write_whole(self, card_ids, async move |app| {
                    app.store_telling(notification)
                        .insert_block(&mut stored, &modified_by)
                        .await?;
                    Ok(stored)
                })
                .await?;
        } else {
            let _structure = self.collab().changing_structure(self).await;
            self.store_telling(notification)
                .insert_block(block, modified_by)
                .await?;
        }
        block.hide_inactive_number();
        log_notification(notification, ChangeAction::Add, &block.id);

        let app = self.clone();
        let block = block.clone();
        self.spawn_background(async move {
            app.ws()
                .broadcast_block_change(&container.workspace_id, &block)
                .await;
            app.tell_webhooks(&block).await;
        });
        Ok(())
    }

    /// Inserts several blocks of one board as [`App::insert_block`] does, telling
    /// websocket listeners about each right after it is stored and webhooks in the
    /// background. No blocks are no error.
    ///
    /// The blocks are stored all or none. The original stores them one by one and keeps
    /// those before a refused one (server/app/blocks.go:182-191).
    ///
    /// # Errors
    ///
    /// Returns a bad request if the blocks belong to more than one board or a title or
    /// the fields of a block are too large, and a not-found error if the board does not
    /// exist.
    pub async fn insert_blocks(
        &self,
        blocks: &mut [Block],
        modified_by: &str,
        notification: ChangeNotification,
    ) -> Result<(), AppError> {
        let Some(first) = blocks.first() else {
            return Ok(());
        };
        let container_id = first.container_id.clone();
        if blocks
            .iter()
            .any(|block| block.container_id != container_id)
        {
            return Err(AppError::bad_request(
                "the blocks belong to more than one board",
            ));
        }
        let container = self.store().get_container(&container_id).await?;

        // All or none: the original stores the blocks one by one, and answers with an
        // error for the third while the first two stay.
        let card_ids: Vec<String> = blocks
            .iter()
            .filter(|block| block.block_type.has_room())
            .map(|block| block.id.clone())
            .collect();
        if card_ids.is_empty() {
            let _structure = self.collab().changing_structure(self).await;
            self.store_telling(notification)
                .insert_blocks(blocks, modified_by)
                .await?;
        } else {
            // Cards stored again write their bodies whole, around their documents.
            let (mut stored, modified_by) = (blocks.to_vec(), modified_by.to_owned());
            let stored = self
                .collab()
                .write_whole(self, card_ids, async move |app| {
                    app.store_telling(notification)
                        .insert_blocks(&mut stored, &modified_by)
                        .await?;
                    Ok(stored)
                })
                .await?;
            blocks.clone_from_slice(&stored);
        }
        hide_inactive_numbers(blocks);
        for block in blocks.iter() {
            self.ws()
                .broadcast_block_change(&container.workspace_id, block)
                .await;
        }
        for block in blocks.iter() {
            log_notification(notification, ChangeAction::Add, &block.id);
        }
        self.notify_webhooks(blocks);
        Ok(())
    }

    /// Applies a patch to a block and returns the patched block; websocket listeners
    /// and webhooks are told in the background.
    ///
    /// # Errors
    ///
    /// Returns a not-found error if the block or its board does not exist and
    /// a bad request if the patched title or fields are too large.
    pub async fn patch_block(
        &self,
        block_id: &str,
        patch: &BlockPatch,
        modified_by: &str,
        notification: ChangeNotification,
    ) -> Result<Block, AppError> {
        let old_block = self.store().get_block(block_id).await?;
        let container = self.store().get_container(&old_block.container_id).await?;
        if patch.body.is_keep() || !old_block.block_type.has_room() {
            let _structure = self.collab().changing_structure(self).await;
            self.store_telling(notification)
                .patch_block(block_id, patch, modified_by)
                .await?;
        } else {
            // A whole body is written around the document of a body edited together.
            let (block_id, patch, modified_by) =
                (block_id.to_owned(), patch.clone(), modified_by.to_owned());
            let card_ids = vec![block_id.clone()];
            self.collab()
                .write_whole(self, card_ids, async move |app| {
                    Ok(app
                        .store_telling(notification)
                        .patch_block(&block_id, &patch, &modified_by)
                        .await?)
                })
                .await?;
        }
        let mut block = self.store().get_block(block_id).await?;
        block.hide_inactive_number();
        self.tell_of_update(&container.workspace_id, &block, notification);
        Ok(block)
    }

    /// Tells the websocket listeners of a workspace and the webhooks, in the
    /// background, of a block that was changed.
    pub(super) fn tell_of_update(
        &self,
        workspace_id: &str,
        block: &Block,
        notification: ChangeNotification,
    ) {
        log_notification(notification, ChangeAction::Update, &block.id);
        let (app, workspace_id, changed) = (self.clone(), workspace_id.to_owned(), block.clone());
        self.spawn_background(async move {
            app.ws()
                .broadcast_block_change(&workspace_id, &changed)
                .await;
            app.tell_webhooks(&changed).await;
        });
    }

    /// Applies each patch of a batch to the block at the same position, all or none;
    /// websocket listeners in `workspace_id` and webhooks are told in the background.
    ///
    /// The blocks may belong to different boards. The members of each block's board who
    /// listen to `workspace_id` are told, so a workspace other than the boards' own reaches only
    /// the listeners of the blocks themselves.
    ///
    /// # Errors
    ///
    /// Returns a not-found error unless every identifier names exactly one block
    /// (an identifier listed twice counts as missing), a bad request if a
    /// patched title or the patched fields are too large, and an internal error if
    /// there are fewer patches than identifiers.
    pub async fn patch_blocks(
        &self,
        workspace_id: &str,
        batch: &BlockPatchBatch,
        modified_by: &str,
        notification: ChangeNotification,
    ) -> Result<(), AppError> {
        // Only whether all of them exist matters here; the original hands the old blocks
        // to its notification service.
        let blocks = self.store().get_blocks_by_ids(&batch.block_ids).await?;
        // Whole bodies are written around the documents of bodies edited together.
        let card_ids =
            crate::collab::cards_written_whole(&blocks, &batch.block_ids, &batch.block_patches);
        if card_ids.is_empty() {
            let _structure = self.collab().changing_structure(self).await;
            self.store_telling(notification)
                .patch_blocks(batch, modified_by)
                .await?;
        } else {
            let (batch, modified_by) = (batch.clone(), modified_by.to_owned());
            self.collab()
                .write_whole(self, card_ids, async move |app| {
                    Ok(app
                        .store_telling(notification)
                        .patch_blocks(&batch, &modified_by)
                        .await?)
                })
                .await?;
        }
        for block_id in &batch.block_ids {
            log_notification(notification, ChangeAction::Update, block_id);
        }

        let app = self.clone();
        let workspace_id = workspace_id.to_owned();
        let block_ids = batch.block_ids.clone();
        self.spawn_background(async move {
            for block_id in &block_ids {
                let mut block = match app.store().get_block(block_id).await {
                    Ok(block) => block,
                    Err(error) => {
                        tracing::error!(%error, block_id, "cannot read a patched block to announce it");
                        return;
                    }
                };
                block.hide_inactive_number();
                app.ws().broadcast_block_change(&workspace_id, &block).await;
                app.tell_webhooks(&block).await;
            }
        });
        Ok(())
    }

    /// Deletes a block and everything below it; websocket listeners are told in the
    /// background. Webhooks are not told about deletions.
    ///
    /// # Errors
    ///
    /// Returns a not-found error if the block or its board does not exist.
    pub async fn delete_block(
        &self,
        block_id: &str,
        modified_by: &str,
        notification: ChangeNotification,
    ) -> Result<(), AppError> {
        let block = self.store().get_block(block_id).await?;
        let container = self.store().get_container(&block.container_id).await?;
        let (removed, by) = (block_id.to_owned(), modified_by.to_owned());
        let deleted = self
            .collab()
            .remove(self, Deletion::Block(removed.clone()), async move |app| {
                Ok(app.store().delete_block_family(&removed, &by).await?)
            })
            .await?;
        log_notification(notification, ChangeAction::Delete, block_id);

        let app = self.clone();
        // Always broadcast the deleted block; for cards, also broadcast descendant cards
        // that went to the trash along with it so board clients drop sub-cards too.
        let child_card_ids: Vec<String> = deleted
            .into_iter()
            .filter(|each| each.id != block.id && each.block_type == BlockType::Card)
            .map(|each| each.id)
            .collect();
        self.spawn_background(async move {
            app.ws()
                .broadcast_block_delete(&container.workspace_id, &block.id, &block.container_id)
                .await;
            for child_id in &child_card_ids {
                app.ws()
                    .broadcast_block_delete(&container.workspace_id, child_id, &block.container_id)
                    .await;
            }
        });
        Ok(())
    }

    /// Returns a block that is in the trash, or `None` if no block of that identifier
    /// is there.
    ///
    /// # Errors
    ///
    /// Returns the store's error if the trash cannot be read.
    pub async fn deleted_block(&self, block_id: &str) -> Result<Option<DeletedBlock>, AppError> {
        match self.store().get_deleted_block(block_id).await {
            Ok(mut deleted) => {
                deleted.block.hide_inactive_number();
                Ok(Some(deleted))
            }
            Err(error) if error.is_not_found() => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    /// Returns the blocks of the deletion a block of the trash went with, which restoring
    /// the block restores together: the one that was deleted first, then those it took
    /// along. Empty if no block of that identifier is there.
    ///
    /// # Errors
    ///
    /// Returns the store's error if the trash cannot be read.
    pub async fn deletion(&self, block_id: &str) -> Result<Vec<Block>, AppError> {
        match self.store().get_deletion(block_id).await {
            Ok(mut blocks) => {
                hide_inactive_numbers(&mut blocks);
                Ok(blocks)
            }
            Err(error) if error.is_not_found() => Ok(Vec::new()),
            Err(error) => Err(error.into()),
        }
    }

    /// Returns who deleted a block of the trash: the user whose deletion put it there,
    /// empty if nobody's did or it is not there.
    ///
    /// # Errors
    ///
    /// Returns the store's error if the trash cannot be read.
    pub async fn deleted_by(&self, block_id: &str) -> Result<String, AppError> {
        match self.store().get_deleted_block(block_id).await {
            Ok(deleted) => Ok(deleted.deleted_by),
            Err(error) if error.is_not_found() => Ok(String::new()),
            Err(error) => Err(error.into()),
        }
    }

    /// Returns the blocks below a block, at any depth: what deleting it takes along.
    ///
    /// # Errors
    ///
    /// Returns the store's error if the blocks cannot be read.
    pub async fn blocks_below(&self, block_id: &str) -> Result<Vec<Block>, AppError> {
        let mut blocks = self.store().get_blocks_below(block_id).await?;
        hide_inactive_numbers(&mut blocks);
        Ok(blocks)
    }

    /// Returns the earlier versions of a comment's body, the first first.
    ///
    /// # Errors
    ///
    /// Returns the store's error if they cannot be read.
    pub async fn comment_versions(&self, block_id: &str) -> Result<Vec<CommentVersion>, AppError> {
        Ok(self.store().get_comment_versions(block_id).await?)
    }

    /// Restores a block from the trash, with everything that was deleted along with
    /// it, and returns the block; websocket listeners and webhooks are told in the
    /// background for every restored block. A top-level card in the deletion whose parent
    /// card is no longer live is re-rooted to the top level.
    ///
    /// # Errors
    ///
    /// Returns a not-found error if the block is not in the trash or has nothing to
    /// come back to: its board, or (for non-card blocks) the block it was on, does not
    /// exist. Returns a conflict error if a card cannot be restored under its live parent
    /// (for example, because the parent became a template or the subtree would exceed
    /// [`fvoci_model::MAX_CARD_DEPTH`]).
    pub async fn undelete_block(
        &self,
        block_id: &str,
        modified_by: &str,
    ) -> Result<Block, AppError> {
        let restored = {
            let _structure = self.collab().changing_structure(self).await;
            self.store()
                .undelete_block_family(block_id, modified_by)
                .await
        };
        let mut restored = match restored {
            Ok(blocks) => blocks,
            Err(error) if error.is_not_found() => return Err(AppError::not_found("block")),
            Err(error) => return Err(error.into()),
        };
        hide_inactive_numbers(&mut restored);
        let block = restored
            .iter()
            .find(|each| each.id == block_id)
            .cloned()
            .ok_or_else(|| AppError::not_found("block"))?;
        let container = self.store().get_container(&block.container_id).await?;
        // An undeletion is always reported; there is no flag for it.
        log_notification(ChangeNotification::Send, ChangeAction::Add, block_id);

        let app = self.clone();
        let root_id = block_id.to_owned();
        self.spawn_background(async move {
            for each in &restored {
                if each.id == root_id || each.block_type == BlockType::Card {
                    app.ws()
                        .broadcast_block_change(&container.workspace_id, each)
                        .await;
                    app.tell_webhooks(each).await;
                }
            }
        });
        Ok(block)
    }

    /// Copies a block of a board and its descendants, except comments, under new
    /// identifiers, gives copied images, file blocks and attachments files of their own,
    /// and returns the copies, the copy of the block itself first. Websocket listeners are told in
    /// the background; webhooks are not told about duplicates.
    ///
    /// # Errors
    ///
    /// Returns a not-found error if the board does not exist or has no such block,
    /// or if the block is a comment, which is never copied.
    pub async fn duplicate_block(
        &self,
        container_id: &str,
        block_id: &str,
        user_id: &str,
        duplicate_as: DuplicateAs,
    ) -> Result<Vec<Block>, AppError> {
        let container = self.store().get_container(container_id).await?;
        // The card and all descendant cards below it, which are copied with it.
        let mut sources = vec![block_id.to_owned()];
        sources.extend(
            self.blocks_below(block_id)
                .await?
                .into_iter()
                .filter(|child| {
                    child.container_id == container_id && child.block_type == BlockType::Card
                })
                .map(|child| child.id),
        );
        self.collab().write_bodies(self, &sources).await?;
        let mut blocks = self
            .store()
            .duplicate_block(container_id, block_id, user_id, duplicate_as)
            .await?;
        let files = self
            .copy_and_update_card_files(container_id, user_id, &mut blocks)
            .await;
        hide_inactive_numbers(&mut blocks);
        if files.is_ok() {
            self.keep_bodies_as_copied(&blocks).await;
        }

        // The copies are stored whatever became of their files, so they are announced
        // whatever became of their files.
        let app = self.clone();
        let copies = blocks.clone();
        self.spawn_background(async move {
            for block in &copies {
                app.ws()
                    .broadcast_block_change(&container.workspace_id, block)
                    .await;
            }
        });
        files?;
        Ok(blocks)
    }

    /// Keeps the body of each copied card, as it was copied and pointed at its own files,
    /// as the copy's one version: a copy begins with the body it was made with to go back
    /// to, and none of the original's versions. A version that cannot be kept is logged:
    /// the copy is stored and told of already, as a card that has none yet.
    async fn keep_bodies_as_copied(&self, copies: &[Block]) {
        let with_bodies = copies
            .iter()
            .filter(|copy| copy.block_type.has_room() && copy.body.is_some());
        for copy in with_bodies {
            if let Err(error) = self.store().keep_body_version(&copy.id).await {
                tracing::error!(
                    block_id = %copy.id, %error,
                    "the body of a copy was not kept as its version"
                );
            }
        }
    }

    /// Posts the blocks to the webhooks, one after the other, in the background.
    fn notify_webhooks(&self, blocks: &[Block]) {
        if self.config().block_change_webhooks.is_empty() {
            return;
        }
        let app = self.clone();
        let blocks = blocks.to_vec();
        self.spawn_background(async move {
            for block in &blocks {
                app.tell_webhooks(block).await;
            }
        });
    }
}

/// Hides stored card numbers on blocks that do not currently take one (card templates).
pub(super) fn hide_inactive_numbers(blocks: &mut [Block]) {
    for block in blocks {
        block.hide_inactive_number();
    }
}

/// The whole of a notification on a standalone server: the log line the original's
/// notification service writes for a block change.
pub(super) fn log_notification(
    notification: ChangeNotification,
    action: ChangeAction,
    block_id: &str,
) {
    if notification == ChangeNotification::Send {
        tracing::debug!(action = action.as_str(), block_id, "Block change event");
    }
}
