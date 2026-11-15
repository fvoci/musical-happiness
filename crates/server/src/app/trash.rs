//! What is in the trash, as lists to choose what to bring back from.

use fvoci_model::{DeletedItem, WorkspaceRole};

use super::App;
use super::cards::CardRange;
use crate::error::AppError;

impl App {
    /// Returns one page of the cards of a board that are in the trash, the last deleted
    /// first. `page` and `per_page` are read as for the board's cards.
    ///
    /// # Errors
    ///
    /// Returns the store's error if the trash cannot be read.
    pub async fn deleted_cards(
        &self,
        board_id: &str,
        page: i64,
        per_page: i64,
    ) -> Result<Vec<DeletedItem>, AppError> {
        let paging = match CardRange::new(page, per_page) {
            CardRange::All => None,
            CardRange::Page(paging) => Some(paging),
            CardRange::Nothing => return Ok(Vec::new()),
        };
        let deleted = self.store().get_deleted_cards(board_id, paging).await?;
        Ok(deleted
            .into_iter()
            .map(|deleted| DeletedItem {
                id: deleted.block.id,
                title: deleted.block.title,
                deleted_at: deleted.block.delete_at,
                deleted_by: deleted.deleted_by,
                kind: None,
            })
            .collect())
    }

    /// Returns one page of the boards of a workspace that are in the trash and that the user
    /// was an admin of when they were deleted, or, for an administrator of the workspace
    /// (`role`), every one: whom `undeleteBoard` lets bring them back. Of the kinds the
    /// handle reaches ([`App::reaching`]). The last deleted first; `page` and `per_page`
    /// are read as for cards.
    ///
    /// # Errors
    ///
    /// Returns the store's error if the trash cannot be read.
    pub async fn deleted_boards(
        &self,
        workspace_id: &str,
        user_id: &str,
        role: WorkspaceRole,
        page: i64,
        per_page: i64,
    ) -> Result<Vec<DeletedItem>, AppError> {
        let paging = match CardRange::new(page, per_page) {
            CardRange::All => None,
            CardRange::Page(paging) => Some(paging),
            CardRange::Nothing => return Ok(Vec::new()),
        };
        let deleted = self
            .store()
            .get_deleted_boards(
                workspace_id,
                (role != WorkspaceRole::Admin).then_some(user_id),
                paging,
                self.kinds(),
            )
            .await?;
        Ok(deleted
            .into_iter()
            .map(|deleted| DeletedItem {
                id: deleted.board.id,
                title: deleted.board.title,
                deleted_at: deleted.board.delete_at,
                deleted_by: deleted.deleted_by,
                kind: Some(deleted.board.kind),
            })
            .collect())
    }
}
