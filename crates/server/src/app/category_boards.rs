//! Which boards a user has filed under which sidebar category.

use std::collections::{HashMap, HashSet};

use fvoci_model::{
    BoardCategoryWebsocketData, CATEGORY_BOARDS_SORT_ORDER_GAP, Category, CategoryBoardMetadata,
    CategoryBoards, CategoryType,
};
use fvoci_store::{BoardVisibility, PublicBoards};

use super::App;
use super::category::CATEGORY;
use crate::error::AppError;

/// The name of the category every user has, which collects the boards filed nowhere else.
pub const DEFAULT_CATEGORY_BOARDS: &str = "Boards";

fn has_default_category(category_boards: &[CategoryBoards]) -> bool {
    category_boards
        .iter()
        .any(|entry| entry.category.name == DEFAULT_CATEGORY_BOARDS)
}

impl App {
    /// Returns a user's categories in a team with the boards in each, creating the
    /// default category first if the user does not have it yet.
    ///
    /// # Errors
    ///
    /// Returns the store's error if the categories cannot be read or the default
    /// category cannot be created.
    pub async fn user_category_boards(
        &self,
        user_id: &str,
        team_id: &str,
    ) -> Result<Vec<CategoryBoards>, AppError> {
        let category_boards = self
            .store()
            .get_user_category_boards(user_id, team_id)
            .await?;
        if has_default_category(&category_boards) {
            return Ok(category_boards);
        }

        // The original looks and then creates without holding anything in between, so
        // two requests of a new user that arrive together each create a default
        // category, and the boards of one of them are filed nowhere. Here the second
        // request waits and finds the category the first one created. The category
        // exists before boards are filed under it, so the nested call that filing makes
        // returns above and does not wait for this turn again.
        let _turn = self.default_category_turn().await;
        let mut category_boards = self
            .store()
            .get_user_category_boards(user_id, team_id)
            .await?;
        if !has_default_category(&category_boards) {
            let created = self
                .create_boards_category(user_id, team_id, &category_boards)
                .await?;
            category_boards.push(created);
        }
        Ok(category_boards)
    }

    /// Creates the default category and files under it every board the user is an
    /// explicit member of that is in no category yet.
    async fn create_boards_category(
        &self,
        user_id: &str,
        team_id: &str,
        existing: &[CategoryBoards],
    ) -> Result<CategoryBoards, AppError> {
        let sort_order = i64::try_from(existing.len())
            .unwrap_or(i64::MAX)
            .saturating_mul(CATEGORY_BOARDS_SORT_ORDER_GAP);
        let category = Category {
            name: DEFAULT_CATEGORY_BOARDS.to_owned(),
            user_id: user_id.to_owned(),
            team_id: team_id.to_owned(),
            collapsed: false,
            category_type: CategoryType::System,
            sort_order,
            ..Category::default()
        };
        let created = self.create_category(category).await.map_err(|error| {
            error.context("createBoardsCategory default category creation failed")
        })?;

        // The boards are read before the memberships. The original reads them the other
        // way round, so a board created by another request in between is in its list of
        // boards but not in its list of memberships, and the request fails.
        let boards = self
            .store()
            .get_boards_for_user_and_team(user_id, team_id, PublicBoards::Exclude)
            .await
            .map_err(|error| {
                AppError::from(error)
                    .context("createBoardsCategory error fetching user's team's boards")
            })?;

        let memberships: HashMap<String, bool> = self
            .store()
            .get_members_for_user(user_id)
            .await
            .map_err(|error| {
                AppError::from(error)
                    .context("createBoardsCategory error fetching user's board memberships")
            })?
            .into_iter()
            .map(|member| (member.board_id, member.synthetic))
            .collect();
        let mut board_ids = Vec::new();
        for board in boards {
            let Some(synthetic) = memberships.get(&board.id) else {
                return Err(AppError::internal(
                    "createBoardsCategory: board membership not found for user's board",
                ));
            };
            // A board the user can merely see shows in the sidebar only once opened.
            if *synthetic {
                continue;
            }
            let filed = existing
                .iter()
                .flat_map(|entry| &entry.board_metadata)
                .any(|metadata| metadata.board_id == board.id);
            if !filed {
                board_ids.push(board.id);
            }
        }

        let board_metadata = board_ids
            .iter()
            .map(|board_id| CategoryBoardMetadata {
                board_id: board_id.clone(),
                hidden: false,
            })
            .collect();
        if !board_ids.is_empty() {
            // Boxed: this function and the one it calls call each other.
            Box::pin(self.add_update_user_category_board(
                team_id,
                user_id,
                &created.id,
                &board_ids,
            ))
            .await
            .map_err(|error| {
                error.context(&format!(
                    "createBoardsCategory failed to add category-less board to the default \
                         category, defaultCategoryID: {}, error",
                    created.id
                ))
            })?;
        }
        Ok(CategoryBoards {
            category: created,
            board_metadata,
        })
    }

    /// Files boards under one of a user's categories, moving them out of any other, and
    /// tells the user's connection the category's new content.
    ///
    /// The boards are not looked at: any identifier can be filed, whether or not it is
    /// a board the user can see.
    ///
    /// # Errors
    ///
    /// Returns a permission error if the category belongs to another user, an internal
    /// error if it is not one of the user's categories in the team, or the store's error
    /// if the boards cannot be filed. Nothing is written in any of these cases.
    pub async fn add_update_user_category_board(
        &self,
        team_id: &str,
        user_id: &str,
        category_id: &str,
        board_ids: &[String],
    ) -> Result<(), AppError> {
        if board_ids.is_empty() {
            return Ok(());
        }
        // The original writes first and only then finds that the user has no such
        // category, which lets a user file boards into another user's category
        // (server/app/category_boards.go:150-170). The check comes first here.
        self.user_category(team_id, user_id, category_id).await?;
        self.store()
            .add_update_category_board(user_id, category_id, board_ids)
            .await?;

        let category_boards = self.user_category_boards(user_id, team_id).await?;
        let Some(updated) = category_boards
            .iter()
            .find(|entry| entry.category.id == category_id)
        else {
            return Err(AppError::not_found(CATEGORY));
        };
        let payload: Vec<BoardCategoryWebsocketData> = updated
            .board_metadata
            .iter()
            .map(|metadata| BoardCategoryWebsocketData {
                board_id: metadata.board_id.clone(),
                category_id: category_id.to_owned(),
                hidden: metadata.hidden,
            })
            .collect();
        self.ws()
            .broadcast_category_board_change(team_id, user_id, &payload);
        Ok(())
    }

    /// Puts the boards of one of a user's categories into the given order and tells the
    /// user's connection about it. Returns the order.
    ///
    /// # Errors
    ///
    /// Returns an internal error if the category is not one of the user's categories in
    /// the team, or `new_order` does not name exactly as many boards as the category
    /// has, or names one that is not in it, and the store's error if a read or write
    /// fails.
    pub async fn reorder_category_boards(
        &self,
        user_id: &str,
        team_id: &str,
        category_id: &str,
        new_order: Vec<String>,
    ) -> Result<Vec<String>, AppError> {
        self.verify_new_category_boards_match_existing(user_id, team_id, category_id, &new_order)
            .await?;
        let new_order = self
            .store()
            .reorder_category_boards(category_id, new_order)
            .await?;
        self.ws()
            .broadcast_category_boards_reorder(team_id, user_id, category_id, &new_order);
        Ok(new_order)
    }

    /// Checks that a new order names the boards of the category and nothing else.
    ///
    /// Preserved quirk: only the count and the membership are compared, so an order that
    /// names one board twice and leaves another out passes
    /// (server/app/category_boards.go:212-263).
    async fn verify_new_category_boards_match_existing(
        &self,
        user_id: &str,
        team_id: &str,
        category_id: &str,
        new_order: &[String],
    ) -> Result<(), AppError> {
        let category_boards = self.user_category_boards(user_id, team_id).await?;
        let Some(target) = category_boards
            .iter()
            .find(|entry| entry.category.id == category_id)
        else {
            return Err(AppError::not_found(CATEGORY));
        };
        if target.board_metadata.len() != new_order.len() {
            return Err(AppError::bad_request(format_args!(
                "the new order must list every board of the category once: it has {}, the \
                 category has {}",
                new_order.len(),
                target.board_metadata.len(),
            )));
        }
        let existing_ids: HashSet<&str> = target
            .board_metadata
            .iter()
            .map(|metadata| metadata.board_id.as_str())
            .collect();
        match new_order
            .iter()
            .find(|board_id| !existing_ids.contains(board_id.as_str()))
        {
            Some(unknown) => Err(AppError::bad_request(format_args!(
                "the new order names {unknown}, which is no board of the category"
            ))),
            None => Ok(()),
        }
    }

    /// Hides a board in one of a user's categories, or shows it again, and tells the
    /// user's connection about it.
    ///
    /// The route has checked that the category is the user's and that the user sees the
    /// board. Preserved quirk: if the user does not have the board in the category,
    /// nothing changes, and the user's connection is told of the change all the same
    /// (server/app/category_boards.go:265-279).
    ///
    /// # Errors
    ///
    /// Returns the store's error if the write fails.
    pub async fn set_board_visibility(
        &self,
        team_id: &str,
        user_id: &str,
        category_id: &str,
        board_id: &str,
        visibility: BoardVisibility,
    ) -> Result<(), AppError> {
        self.user_category(team_id, user_id, category_id).await?;
        self.store()
            .set_board_visibility(user_id, category_id, board_id, visibility)
            .await
            .map_err(|error| {
                AppError::from(error)
                    .context("SetBoardVisibility: failed to update board visibility")
            })?;
        let change = BoardCategoryWebsocketData {
            board_id: board_id.to_owned(),
            category_id: category_id.to_owned(),
            hidden: visibility == BoardVisibility::Hidden,
        };
        self.ws()
            .broadcast_category_board_change(team_id, user_id, &[change]);
        Ok(())
    }
}
