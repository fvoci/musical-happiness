//! Copying a board or a wiki whole.

use fvoci_model::{BoardError, BoardKind, BoardMember, BoardsAndBlocks, DeleteBoardsAndBlocks};
use fvoci_store::DuplicateAs;

use super::App;
use super::pages::refused_without_member;
use crate::error::AppError;

impl App {
    /// Copies a board and its blocks, except comments, into `to_workspace` for a user, who
    /// becomes the copy's admin. Returns the copies and that membership.
    ///
    /// A wiki is copied with its pages as they stand, their tree, bodies and icons, held
    /// to the organization's limits of pages; the pages in its trash, the comments on
    /// its pages and the versions of their bodies stay behind. A copy that is not a
    /// template is filed under the category the user has filed the source under, or
    /// else under the default category.
    ///
    /// # Errors
    ///
    /// Returns a not-found error if there is no such board, a bad request for a wiki
    /// asked to be copied as a template or larger than the limits, an error if its files
    /// cannot be copied, or the store's error.
    pub async fn duplicate_board(
        &self,
        board_id: &str,
        user_id: &str,
        to_workspace: &str,
        duplicate_as: DuplicateAs,
    ) -> Result<(BoardsAndBlocks, Vec<BoardMember>), AppError> {
        let kind = self.board(board_id).await?.kind;
        if kind == BoardKind::Wiki && duplicate_as == DuplicateAs::Template {
            return Err(
                AppError::bad_request(BoardError::WikiIsTemplate).with_parameter("asTemplate")
            );
        }
        self.write_board_bodies(board_id).await?;
        let to_workspace_named = Some(to_workspace).filter(|workspace| !workspace.is_empty());
        let (mut copies, members) = match kind {
            BoardKind::Board => {
                self.store()
                    .duplicate_board(board_id, user_id, to_workspace_named, duplicate_as)
                    .await?
            }
            BoardKind::Wiki => self
                .store()
                .duplicate_wiki(board_id, user_id, to_workspace_named, self.limits().pages)
                .await
                .map_err(refused_without_member)?,
        };

        // The copied blocks get files of their own.
        if let Err(error) = self
            .copy_and_update_card_files(board_id, user_id, &mut copies.blocks)
            .await
        {
            let delete = DeleteBoardsAndBlocks::from(&copies);
            if let Err(delete_error) = self
                .store()
                .delete_boards_and_blocks(&delete, user_id)
                .await
            {
                tracing::error!(
                    board_id,
                    error = %delete_error,
                    "Cannot delete board after duplication error when updating block's file info"
                );
            }
            return Err(error.context(format_args!(
                "could not patch file IDs while duplicating board {board_id}"
            )));
        }

        if duplicate_as == DuplicateAs::Copy {
            for board in &copies.boards {
                self.set_board_category_from_source(board_id, &board.id, user_id, to_workspace)
                    .await?;
            }
        }

        let app = self.clone();
        let broadcast = copies.clone();
        let new_members = members.clone();
        self.spawn_background(async move {
            let mut workspace_id = "";
            for board in &broadcast.boards {
                workspace_id = &board.workspace_id;
                app.ws().broadcast_board_change(workspace_id, board).await;
            }
            for block in &broadcast.blocks {
                app.ws().broadcast_block_change(workspace_id, block).await;
            }
            for member in &new_members {
                app.ws()
                    .broadcast_member_change(workspace_id, &member.board_id, member)
                    .await;
            }
        });

        Ok((copies, members))
    }

    /// Files a copy of a board under the category its source is filed under for the
    /// user in the workspace, or else under the default category.
    async fn set_board_category_from_source(
        &self,
        source_board_id: &str,
        destination_board_id: &str,
        user_id: &str,
        workspace_id: &str,
    ) -> Result<(), AppError> {
        let destination = [destination_board_id.to_owned()];
        let categories = self.user_category_boards(user_id, workspace_id).await?;
        let source_category = categories.iter().find(|entry| {
            entry
                .board_metadata
                .iter()
                .any(|metadata| metadata.board_id == source_board_id)
        });
        match source_category {
            Some(entry) => {
                self.add_update_user_category_board(
                    workspace_id,
                    user_id,
                    &entry.category.id,
                    &destination,
                )
                .await
            }
            None => {
                self.add_boards_to_default_category(user_id, workspace_id, &destination)
                    .await
            }
        }
    }
}
