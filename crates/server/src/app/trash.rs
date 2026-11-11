//! What is in the trash, as lists to choose what to bring back from.

use fvoci_model::DeletedItem;

use super::App;
use super::cards::CardRange;
use crate::error::AppError;
use crate::permissions::{BoardPermission, member_has_permission};

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
            })
            .collect())
    }

    /// Returns one page of the boards of a team that are in the trash and that the user
    /// was an admin of when they were deleted: whom `undeleteBoard` lets bring them
    /// back. The last deleted first; `page` and `per_page` are read as for cards.
    ///
    /// # Errors
    ///
    /// Returns the store's error if the trash cannot be read.
    pub async fn deleted_boards(
        &self,
        team_id: &str,
        user_id: &str,
        page: i64,
        per_page: i64,
    ) -> Result<Vec<DeletedItem>, AppError> {
        let range = CardRange::new(page, per_page);
        if range == CardRange::Nothing {
            return Ok(Vec::new());
        }
        let items = self
            .store()
            .get_deleted_boards(team_id)
            .await?
            .into_iter()
            .filter(|deleted| {
                deleted.members.iter().any(|member| {
                    member.user_id == user_id
                        && member_has_permission(member, BoardPermission::DeleteBoard)
                })
            })
            .map(|deleted| DeletedItem {
                id: deleted.board.id,
                title: deleted.board.title,
                deleted_at: deleted.board.delete_at,
                deleted_by: deleted.deleted_by,
            });
        Ok(match range {
            CardRange::Page(paging) => {
                let per_page = usize::try_from(paging.per_page).unwrap_or(usize::MAX);
                let skip = usize::try_from(paging.page)
                    .unwrap_or(usize::MAX)
                    .saturating_mul(per_page);
                items.skip(skip).take(per_page).collect()
            }
            CardRange::All | CardRange::Nothing => items.collect(),
        })
    }
}
