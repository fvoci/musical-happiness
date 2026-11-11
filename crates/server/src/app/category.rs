//! A user's sidebar categories.

use std::collections::HashSet;

use fvoci_model::{Category, CategoryType};

use super::App;
use super::category_boards::{DEFAULT_CATEGORY_BOARDS, is_filed_in};
use crate::auth::now_millis;
use crate::error::AppError;

/// What a request misses that names a category the user does not have.
pub(super) const CATEGORY: &str = "category";

impl App {
    /// Returns the category with the given identifier, deleted or not, whoever owns it.
    ///
    /// # Errors
    ///
    /// Returns a not-found error if there is no such category, or the store's error if
    /// it cannot be read.
    pub async fn category(&self, category_id: &str) -> Result<Category, AppError> {
        Ok(self.store().get_category(category_id).await?)
    }

    /// Creates a category, giving it an identifier and timestamps if it has none, and
    /// tells the owner's connection about it.
    ///
    /// # Errors
    ///
    /// Returns a bad request if the category is not valid, or the store's error if it
    /// cannot be written or read back.
    pub async fn create_category(&self, mut category: Category) -> Result<Category, AppError> {
        category.hydrate(now_millis());
        category.validate()?;
        self.store().create_category(&category).await?;
        let created = self.store().get_category(&category.id).await?;
        self.ws().broadcast_category_change(&created);
        Ok(created)
    }

    /// Changes the name and the collapsed state of a category to those of `category`,
    /// and tells the owner's connection about it. Nothing else can be changed, and a
    /// system category keeps its name.
    ///
    /// # Errors
    ///
    /// Returns a bad request if `category` is not valid, a not-found error if the user
    /// and team `category` names have no such category (also if it is deleted), or the
    /// store's error if it cannot be written or read back.
    pub async fn update_category(&self, mut category: Category) -> Result<Category, AppError> {
        let now = now_millis();
        category.hydrate(now);
        category.validate()?;

        let existing = self
            .user_category(&category.team_id, &category.user_id, &category.id)
            .await?;
        if existing.category_type == CategoryType::System {
            category.name = existing.name;
        }
        category.update_at = now;
        self.store().update_category(&category).await?;

        let updated = self.store().get_category(&category.id).await?;
        self.ws().broadcast_category_change(&updated);
        Ok(updated)
    }

    /// Deletes one of a user's categories in a team, after moving its boards to the
    /// user's default category, and tells the owner's connection about it.
    ///
    /// # Errors
    ///
    /// Returns a not-found error if the user has no such category in the team (also if
    /// it is deleted already), a permission error if it is a system category, or the
    /// store's error if a read or write fails.
    pub async fn delete_category(
        &self,
        category_id: &str,
        user_id: &str,
        team_id: &str,
    ) -> Result<Category, AppError> {
        let existing = self.user_category(team_id, user_id, category_id).await?;
        if existing.category_type == CategoryType::System {
            return Err(AppError::forbidden("a system category cannot be deleted"));
        }

        self.move_boards_to_default_category(user_id, team_id, category_id)
            .await?;
        self.store()
            .delete_category(category_id, user_id, team_id)
            .await?;

        // The category is gone from the store. What is sent to the user's connections is
        // the category as it was, marked as deleted now.
        let deleted = Category {
            delete_at: now_millis(),
            ..existing
        };
        self.ws().broadcast_category_change(&deleted);
        Ok(deleted)
    }

    /// Returns one of a user's categories in a team.
    ///
    /// # Errors
    ///
    /// Returns a not-found error if there is no such category, or it is another user's or
    /// in another team: the categories of a user are nobody else's
    /// business, so for everyone else they do not exist.
    pub(crate) async fn user_category(
        &self,
        team_id: &str,
        user_id: &str,
        category_id: &str,
    ) -> Result<Category, AppError> {
        let category = match self.store().get_category(category_id).await {
            Ok(category) => category,
            Err(error) if error.is_not_found() => return Err(AppError::not_found(CATEGORY)),
            Err(error) => return Err(error.into()),
        };
        if category.user_id != user_id || category.team_id != team_id {
            return Err(AppError::not_found(CATEGORY));
        }
        Ok(category)
    }

    /// Moves the boards of a category to the user's default category, which is created
    /// if the user does not have it yet. A board of another team, filed here before the
    /// categories took the boards of their own team only, is not moved: it goes with the
    /// category, and is filed nowhere in the team.
    async fn move_boards_to_default_category(
        &self,
        user_id: &str,
        team_id: &str,
        source_category_id: &str,
    ) -> Result<(), AppError> {
        let category_boards = self.user_category_boards(user_id, team_id).await?;
        let Some(source) = category_boards
            .iter()
            .find(|entry| entry.category.id == source_category_id)
        else {
            return Err(AppError::not_found(CATEGORY));
        };
        let Some(default) = category_boards
            .iter()
            .find(|entry| entry.category.name == DEFAULT_CATEGORY_BOARDS)
        else {
            return Err(AppError::internal(
                "moveBoardsToDefaultCategory: no default category found",
            ));
        };

        let mut board_ids = Vec::with_capacity(source.board_metadata.len());
        for metadata in &source.board_metadata {
            let board = self.board(&metadata.board_id).await?;
            if is_filed_in(&board, team_id) {
                board_ids.push(board.id);
            }
        }
        self.add_update_user_category_board(team_id, user_id, &default.category.id, &board_ids)
            .await
            .map_err(|error| error.context("moveBoardsToDefaultCategory"))
    }

    /// Puts a user's categories in a team into the given order and tells the user's
    /// connection about it. Returns the order.
    ///
    /// # Errors
    ///
    /// Returns a bad request if `new_order` does not name exactly as many categories as
    /// the user has, or names one the user does not have, and the store's error if a
    /// read or write fails.
    pub async fn reorder_categories(
        &self,
        user_id: &str,
        team_id: &str,
        new_order: Vec<String>,
    ) -> Result<Vec<String>, AppError> {
        self.verify_new_categories_match_existing(user_id, team_id, &new_order)
            .await?;
        let new_order = self
            .store()
            .reorder_categories(user_id, team_id, new_order)
            .await?;
        self.ws()
            .broadcast_category_reorder(team_id, user_id, &new_order);
        Ok(new_order)
    }

    /// Checks that a new order names the user's categories and nothing else.
    ///
    /// As in the original, only the count and the membership are compared, so an order
    /// that names one category twice and leaves another out passes; the category left
    /// out keeps its place (server/app/category.go:210-246).
    async fn verify_new_categories_match_existing(
        &self,
        user_id: &str,
        team_id: &str,
        new_order: &[String],
    ) -> Result<(), AppError> {
        let existing = self.store().get_user_categories(user_id, team_id).await?;
        if new_order.len() != existing.len() {
            return Err(AppError::bad_request(format_args!(
                "the new order must list every category once: it has {}, the user has {}",
                new_order.len(),
                existing.len(),
            )));
        }
        let existing_ids: HashSet<&str> = existing
            .iter()
            .map(|category| category.id.as_str())
            .collect();
        match new_order
            .iter()
            .find(|category_id| !existing_ids.contains(category_id.as_str()))
        {
            Some(unknown) => Err(AppError::bad_request(format_args!(
                "the new order names {unknown}, which is none of the user's categories"
            ))),
            None => Ok(()),
        }
    }
}
