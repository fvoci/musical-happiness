//! Blocks: reading and changing them, and telling websocket listeners and webhooks.

use fvoci_model::{Block, BlockPatch, BlockPatchBatch, BlockType};
use fvoci_store::{BlockFilter, DuplicateAs, HistoryOrder, HistoryQuery};

use super::App;
use crate::error::AppError;

/// Whether a change to a block is reported to the notification service, which tells
/// subscribers and mentioned users about it. Bulk operations suppress it.
///
/// A standalone server has no notification service that delivers anything, so all a
/// notification amounts to here is a debug log line. Websocket broadcasts and webhooks
/// are not notifications: they happen either way.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ChangeNotification {
    /// Report the change.
    #[default]
    Send,
    /// Do not report the change.
    Suppress,
}

/// What happened to a block, as the notification service names it.
#[derive(Debug, Clone, Copy)]
enum ChangeAction {
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
    /// Returns the blocks of a board, optionally only those with the given parent or of
    /// the given type, in no particular order.
    ///
    /// The type is compared as the stored string: [`BlockType::Other`] finds the blocks
    /// of a type the server does not know. The empty type and the type `unknown` do not
    /// filter. An empty board identifier finds nothing.
    ///
    /// # Errors
    ///
    /// Returns the store's error if the query fails.
    pub async fn blocks(
        &self,
        board_id: &str,
        parent_id: Option<&str>,
        block_type: Option<&BlockType>,
    ) -> Result<Vec<Block>, AppError> {
        if board_id.is_empty() {
            return Ok(Vec::new());
        }
        let mut filter = BlockFilter::board(board_id);
        if let Some(parent_id) = parent_id {
            filter = filter.with_parent(parent_id);
        }
        if let Some(block_type) = block_type {
            filter = filter.with_type(block_type.clone());
        }
        Ok(self.store().get_blocks(&filter).await?)
    }

    /// Returns all blocks of a board, in no particular order.
    ///
    /// # Errors
    ///
    /// Returns the store's error if the query fails.
    pub async fn blocks_for_board(&self, board_id: &str) -> Result<Vec<Block>, AppError> {
        Ok(self
            .store()
            .get_blocks(&BlockFilter::board(board_id))
            .await?)
    }

    /// Returns the block with the given identifier.
    ///
    /// # Errors
    ///
    /// Returns a not-found error if there is no such block.
    pub async fn block(&self, block_id: &str) -> Result<Block, AppError> {
        match self.store().get_block(block_id).await {
            Ok(block) => Ok(block),
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
        let board = self.store().get_board(&block.board_id).await?;
        self.store().insert_block(block, modified_by).await?;
        log_notification(notification, ChangeAction::Add, &block.id);

        let app = self.clone();
        let block = block.clone();
        self.spawn_background(async move {
            app.ws()
                .broadcast_block_change(&board.team_id, &block)
                .await;
            app.webhook().notify_update(&block).await;
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
        let board_id = first.board_id.clone();
        if blocks.iter().any(|block| block.board_id != board_id) {
            return Err(AppError::bad_request(
                "the blocks belong to more than one board",
            ));
        }
        let board = self.store().get_board(&board_id).await?;

        // All or none: the original stores the blocks one by one, and answers with an
        // error for the third while the first two stay.
        self.store().insert_blocks(blocks, modified_by).await?;
        for block in blocks.iter() {
            self.ws()
                .broadcast_block_change(&board.team_id, block)
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
        let board = self.store().get_board(&old_block.board_id).await?;
        self.store()
            .patch_block(block_id, patch, modified_by)
            .await?;
        let block = self.store().get_block(block_id).await?;
        log_notification(notification, ChangeAction::Update, block_id);

        let app = self.clone();
        let changed = block.clone();
        self.spawn_background(async move {
            app.ws()
                .broadcast_block_change(&board.team_id, &changed)
                .await;
            app.webhook().notify_update(&changed).await;
        });
        Ok(block)
    }

    /// Applies each patch of a batch to the block at the same position, all or none;
    /// websocket listeners in `team_id` and webhooks are told in the background.
    ///
    /// The blocks may belong to different boards. The members of each block's board who
    /// listen to `team_id` are told, so a team other than the boards' own reaches only
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
        team_id: &str,
        batch: &BlockPatchBatch,
        modified_by: &str,
        notification: ChangeNotification,
    ) -> Result<(), AppError> {
        // Only whether all of them exist matters here; the original hands the old blocks
        // to its notification service.
        self.store().get_blocks_by_ids(&batch.block_ids).await?;
        self.store().patch_blocks(batch, modified_by).await?;
        for block_id in &batch.block_ids {
            log_notification(notification, ChangeAction::Update, block_id);
        }

        let app = self.clone();
        let team_id = team_id.to_owned();
        let block_ids = batch.block_ids.clone();
        self.spawn_background(async move {
            for block_id in &block_ids {
                let block = match app.store().get_block(block_id).await {
                    Ok(block) => block,
                    Err(error) => {
                        tracing::error!(%error, block_id, "cannot read a patched block to announce it");
                        return;
                    }
                };
                app.ws().broadcast_block_change(&team_id, &block).await;
                app.webhook().notify_update(&block).await;
            }
        });
        Ok(())
    }

    /// Deletes a block and its direct children; websocket listeners are told in the
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
        let board = self.store().get_board(&block.board_id).await?;
        self.store().delete_block(block_id, modified_by).await?;
        log_notification(notification, ChangeAction::Delete, block_id);

        let app = self.clone();
        self.spawn_background(async move {
            app.ws()
                .broadcast_block_delete(&board.team_id, &block.id, &block.board_id)
                .await;
        });
        Ok(())
    }

    /// Returns the newest recorded state of a block, deleted or not, or `None` if no
    /// block with this identifier was ever stored.
    ///
    /// # Errors
    ///
    /// Returns the store's error if the query fails.
    pub async fn last_block_history_entry(
        &self,
        block_id: &str,
    ) -> Result<Option<Block>, AppError> {
        let newest = HistoryQuery {
            limit: Some(1),
            order: HistoryOrder::Descending,
            ..HistoryQuery::default()
        };
        let entries = self.store().get_block_history(block_id, &newest).await?;
        Ok(entries.into_iter().next())
    }

    /// Restores a deleted block and its deleted direct children and returns the block;
    /// websocket listeners and webhooks are told in the background. A block that is not
    /// deleted is returned and announced as it is. Returns `None`, and does nothing, if
    /// no block with this identifier was ever stored.
    ///
    /// # Errors
    ///
    /// Returns a not-found error if the block or its board cannot be read after
    /// the block was restored.
    pub async fn undelete_block(
        &self,
        block_id: &str,
        modified_by: &str,
    ) -> Result<Option<Block>, AppError> {
        if self.last_block_history_entry(block_id).await?.is_none() {
            return Ok(None);
        }
        self.store().undelete_block(block_id, modified_by).await?;
        let block = self
            .store()
            .get_block(block_id)
            .await
            .inspect_err(|error| {
                if error.is_not_found() {
                    tracing::error!(
                        block_id,
                        "Error loading the block after a successful undelete, not propagating \
                     through websockets or notifications"
                    );
                }
            })?;
        let board = self.store().get_board(&block.board_id).await?;
        // An undeletion is always reported; the original has no flag for it.
        log_notification(ChangeNotification::Send, ChangeAction::Add, block_id);

        let app = self.clone();
        let restored = block.clone();
        self.spawn_background(async move {
            app.ws()
                .broadcast_block_change(&board.team_id, &restored)
                .await;
            app.webhook().notify_update(&restored).await;
        });
        Ok(Some(block))
    }

    /// Copies a block of a board and its direct children, except comments, under new
    /// identifiers, gives copied images and attachments files of their own, and returns
    /// the copies, the copy of the block itself first. Websocket listeners are told in
    /// the background; webhooks are not told about duplicates.
    ///
    /// # Errors
    ///
    /// Returns a not-found error if the board does not exist or has no such block,
    /// or if the block is a comment, which is never copied.
    pub async fn duplicate_block(
        &self,
        board_id: &str,
        block_id: &str,
        user_id: &str,
        duplicate_as: DuplicateAs,
    ) -> Result<Vec<Block>, AppError> {
        let board = self.store().get_board(board_id).await?;
        let blocks = self
            .store()
            .duplicate_block(board_id, block_id, user_id, duplicate_as)
            .await?;
        self.copy_and_update_card_files(board_id, user_id, &blocks, duplicate_as)
            .await?;

        let app = self.clone();
        let copies = blocks.clone();
        self.spawn_background(async move {
            for block in &copies {
                app.ws().broadcast_block_change(&board.team_id, block).await;
            }
        });
        Ok(blocks)
    }

    /// Posts the blocks to the webhooks, one after the other, in the background.
    fn notify_webhooks(&self, blocks: &[Block]) {
        if self.config().webhook_update.is_empty() {
            return;
        }
        let app = self.clone();
        let blocks = blocks.to_vec();
        self.spawn_background(async move {
            for block in &blocks {
                app.webhook().notify_update(block).await;
            }
        });
    }
}

/// The whole of a notification on a standalone server: the log line the original's
/// notification service writes for a block change.
fn log_notification(notification: ChangeNotification, action: ChangeAction, block_id: &str) {
    if notification == ChangeNotification::Send {
        tracing::debug!(action = action.as_str(), block_id, "Block change event");
    }
}
