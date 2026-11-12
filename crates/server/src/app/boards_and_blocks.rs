//! Boards and blocks created, patched and deleted together.

use std::collections::HashMap;

use fvoci_model::{BoardMember, BoardsAndBlocks, DeleteBoardsAndBlocks, PatchBoardsAndBlocks};

use fvoci_store::NewImages;

use super::App;
use super::boards::CreatorMembership;
use crate::error::AppError;

impl App {
    /// Stores the boards and blocks of an archive as they are, identifiers included,
    /// tells the boards' members and the webhooks, and files each board that is not a
    /// template under the default category of `user_id`. The boards start without
    /// members. The images of the bodies name the archive's files, which the import
    /// stores after them and points them at: the board's files are not asked of them
    /// ([`fvoci_store::NewImages::Unchecked`]).
    ///
    /// # Errors
    ///
    /// Returns a bad request if a block is too large, the store's error if nothing could
    /// be stored, or an error if a board cannot be filed under the user's default
    /// category; in the last case the boards and blocks exist nonetheless.
    pub async fn create_imported_boards_and_blocks(
        &self,
        boards_and_blocks: BoardsAndBlocks,
        user_id: &str,
    ) -> Result<BoardsAndBlocks, AppError> {
        self.create_boards_and_blocks_as(boards_and_blocks, user_id, CreatorMembership::None)
            .await
    }

    /// Stores boards and their blocks as [`App::create_imported_boards_and_blocks`]
    /// does and makes `user_id` an admin and editor of each board. Each image of a body
    /// shows a file of its board ([`fvoci_store::NewImages::Recorded`]).
    ///
    /// # Errors
    ///
    /// Returns the errors of [`App::create_imported_boards_and_blocks`].
    pub async fn create_boards_and_blocks_with_admin(
        &self,
        boards_and_blocks: BoardsAndBlocks,
        user_id: &str,
    ) -> Result<BoardsAndBlocks, AppError> {
        self.create_boards_and_blocks_as(boards_and_blocks, user_id, CreatorMembership::Admin)
            .await
    }

    async fn create_boards_and_blocks_as(
        &self,
        mut created: BoardsAndBlocks,
        user_id: &str,
        membership: CreatorMembership,
    ) -> Result<BoardsAndBlocks, AppError> {
        let members = match membership {
            CreatorMembership::Admin => {
                self.store()
                    .create_boards_and_blocks_with_admin(&mut created, user_id)
                    .await?
            }
            CreatorMembership::None => {
                self.store()
                    .create_boards_and_blocks(&mut created, user_id, NewImages::Unchecked)
                    .await?;
                Vec::new()
            }
        };

        self.announce_created(&created, &members).await;

        for board in &created.boards {
            if !board.is_template {
                self.add_boards_to_default_category(
                    user_id,
                    &board.workspace_id,
                    std::slice::from_ref(&board.id),
                )
                .await?;
            }
        }
        Ok(created)
    }

    /// Tells the boards' members and the webhooks about boards and blocks that were
    /// stored together, and about the memberships made with them. Not in the
    /// background: the original does this before it answers, because the operation is
    /// rare.
    pub(crate) async fn announce_created(
        &self,
        created: &BoardsAndBlocks,
        members: &[BoardMember],
    ) {
        // All new boards belong to one workspace. The original crashes here without boards;
        // then there is nobody to tell.
        let workspace_id = created
            .boards
            .first()
            .map_or("", |board| board.workspace_id.as_str());
        for board in &created.boards {
            self.ws().broadcast_board_change(workspace_id, board).await;
        }
        for block in &created.blocks {
            self.ws().broadcast_block_change(workspace_id, block).await;
            self.webhook().notify_update(block).await;
        }
        for member in members {
            self.ws()
                .broadcast_member_change(workspace_id, &member.board_id, member)
                .await;
        }
    }

    /// Applies board and block patches, all or none, and tells the boards' members and
    /// the webhooks. Returns the patched boards and blocks in the order of the request.
    ///
    /// # Errors
    ///
    /// Returns a not-found error if a board or block does not exist or a block is listed
    /// twice, a bad request if a patched block is too large, or the store's error.
    pub async fn patch_boards_and_blocks(
        &self,
        patches: &PatchBoardsAndBlocks,
        user_id: &str,
    ) -> Result<BoardsAndBlocks, AppError> {
        // The original reads the blocks first to hand their old state to its
        // notifications. What remains of that is the failure when a block is missing or
        // listed twice.
        let blocks = if patches.block_ids.is_empty() {
            Vec::new()
        } else {
            self.store().get_blocks_by_ids(&patches.block_ids).await?
        };

        // Whole bodies are written around the documents of bodies edited together.
        let card_ids =
            crate::collab::cards_written_whole(&blocks, &patches.block_ids, &patches.block_patches);
        let result = if card_ids.is_empty() {
            self.store()
                .patch_boards_and_blocks(patches, user_id)
                .await?
        } else {
            let (patches, user_id) = (patches.clone(), user_id.to_owned());
            self.collab()
                .write_whole(self, card_ids, async move |app| {
                    Ok(app
                        .store()
                        .patch_boards_and_blocks(&patches, &user_id)
                        .await?)
                })
                .await?
        };

        let app = self.clone();
        let broadcast = result.clone();
        self.spawn_background(async move {
            let workspace_id = broadcast
                .boards
                .first()
                .map_or("", |board| board.workspace_id.as_str());
            for block in &broadcast.blocks {
                app.ws().broadcast_block_change(workspace_id, block).await;
                app.webhook().notify_update(block).await;
            }
            for board in &broadcast.boards {
                app.ws()
                    .broadcast_board_change(&board.workspace_id, board)
                    .await;
            }
        });

        Ok(result)
    }

    /// Deletes blocks and boards, all or none, and tells the boards' members.
    ///
    /// # Errors
    ///
    /// Returns a bad request if no board is listed, a not-found error if a board or
    /// block does not exist, or the store's error.
    pub async fn delete_boards_and_blocks(
        &self,
        request: &DeleteBoardsAndBlocks,
        user_id: &str,
    ) -> Result<(), AppError> {
        // The original crashes without boards.
        let Some(first_board_id) = request.boards.first() else {
            return Err(AppError::bad_request(
                fvoci_model::BoardsAndBlocksError::NoBoards,
            ));
        };
        let workspace_id = self.store().get_board(first_board_id).await?.workspace_id;

        // The memberships go with the boards, so who is told is who was a member; and
        // the blocks are gone afterwards, so which board each was on is read first.
        let mut former_members = HashMap::with_capacity(request.boards.len());
        for board_id in &request.boards {
            let members = self.store().get_members_for_board(board_id).await?;
            former_members.insert(board_id.as_str(), members);
        }
        let mut deleted_blocks = Vec::with_capacity(request.blocks.len());
        for block_id in &request.blocks {
            let block = self.store().get_block(block_id).await?;
            deleted_blocks.push((block.id, block.board_id));
        }

        self.store()
            .delete_boards_and_blocks(request, user_id)
            .await?;

        for (block_id, board_id) in &deleted_blocks {
            // A block of a board the request does not list was refused by the store.
            let members = former_members
                .get(board_id.as_str())
                .map_or(&[][..], Vec::as_slice);
            self.ws()
                .broadcast_block_delete_with_board(&workspace_id, block_id, board_id, members);
        }
        for board_id in &request.boards {
            let members = former_members
                .get(board_id.as_str())
                .map_or(&[][..], Vec::as_slice);
            self.ws()
                .broadcast_board_delete(&workspace_id, board_id, members);
            // The board's sharing went with it; so does whoever followed it by its link,
            // as with `deleteBoard`.
            self.ws().close_readers_of_board(board_id);
        }
        Ok(())
    }
}
