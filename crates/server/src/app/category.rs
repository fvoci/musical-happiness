//! A user's sidebar categories.

use std::collections::HashSet;

use fvoci_model::{Category, CategoryType};

use super::App;
use super::category_boards::DEFAULT_CATEGORY_BOARDS;
use crate::auth::now_millis;
use crate::error::AppError;

/// The refusal for a category that is another user's, or the user's in another team.
pub(super) const CATEGORY_PERMISSION_DENIED: &str = "category doesn't belong to user";

/// The refusal for a category the user does not have among their categories of a team.
pub(super) const CATEGORY_NOT_FOUND: &str =
    "category ID specified in input does not exist for user";

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
    /// Returns a bad request if `category` is not valid, a not-found error if the
    /// category does not exist or is deleted, a permission error if it belongs to
    /// another user or team than `category` says, or the store's error if it cannot be
    /// written or read back.
    pub async fn update_category(&self, mut category: Category) -> Result<Category, AppError> {
        let now = now_millis();
        category.hydrate(now);
        category.validate()?;

        let existing = self.store().get_category(&category.id).await?;
        if existing.delete_at != 0 {
            return Err(AppError::NotFound("category is deleted".to_owned()));
        }
        if existing.user_id != category.user_id || existing.team_id != category.team_id {
            return Err(AppError::forbidden(CATEGORY_PERMISSION_DENIED));
        }
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
    /// user's default category, and tells the owner's connection about it. Deleting a
    /// category that is deleted already changes nothing and returns it.
    ///
    /// # Errors
    ///
    /// Returns a not-found error if the category does not exist, a permission error if
    /// it belongs to another user, a bad request if it belongs to another team, an
    /// internal error if it is a system category, or the store's error if a read or
    /// write fails.
    pub async fn delete_category(
        &self,
        category_id: &str,
        user_id: &str,
        team_id: &str,
    ) -> Result<Category, AppError> {
        let existing = self.store().get_category(category_id).await?;
        // The original looks at the deletion mark first and hands a deleted category to
        // whoever asks for it (server/app/category.go:104-112). The owner comes first.
        if existing.user_id != user_id {
            return Err(AppError::forbidden(CATEGORY_PERMISSION_DENIED));
        }
        if existing.delete_at != 0 {
            // Deleting again would overwrite the time of the deletion.
            return Ok(existing);
        }
        if existing.team_id != team_id {
            return Err(AppError::bad_request("category doesn't belong to the team"));
        }
        if existing.category_type == CategoryType::System {
            // Preserved quirk: the original does not classify this error, so the refusal
            // is a server error (server/app/category.go:13, :119).
            return Err(AppError::internal("cannot delete a system category"));
        }

        self.move_boards_to_default_category(user_id, team_id, category_id)
            .await?;
        self.store()
            .delete_category(category_id, user_id, team_id)
            .await?;

        let deleted = self.store().get_category(category_id).await?;
        self.ws().broadcast_category_change(&deleted);
        Ok(deleted)
    }

    /// Moves the boards of a category to the user's default category, which is created
    /// if the user does not have it yet.
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
            return Err(AppError::internal(CATEGORY_NOT_FOUND));
        };
        let Some(default) = category_boards
            .iter()
            .find(|entry| entry.category.name == DEFAULT_CATEGORY_BOARDS)
        else {
            return Err(AppError::internal(
                "moveBoardsToDefaultCategory: no default category found",
            ));
        };

        let board_ids: Vec<String> = source
            .board_metadata
            .iter()
            .map(|metadata| metadata.board_id.clone())
            .collect();
        self.add_update_user_category_board(team_id, user_id, &default.category.id, &board_ids)
            .await
            .map_err(|error| error.context("moveBoardsToDefaultCategory"))
    }

    /// Puts a user's categories in a team into the given order and tells the user's
    /// connection about it. Returns the order.
    ///
    /// # Errors
    ///
    /// Returns an internal error if `new_order` does not name exactly as many categories
    /// as the user has, or names one the user does not have, and the store's error if a
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
    /// Preserved quirk: only the count and the membership are compared, so an order that
    /// names one category twice and leaves another out passes; the category left out
    /// keeps its place (server/app/category.go:210-246). The refusals are not
    /// classified, so they are server errors.
    async fn verify_new_categories_match_existing(
        &self,
        user_id: &str,
        team_id: &str,
        new_order: &[String],
    ) -> Result<(), AppError> {
        let existing = self.store().get_user_categories(user_id, team_id).await?;
        if new_order.len() != existing.len() {
            return Err(AppError::internal(format_args!(
                "cannot update category order, passed list of categories different size than \
                 in database length new categories: {}, length existing categories: {}, \
                 userID: {user_id}, teamID: {team_id}",
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
            Some(unknown) => Err(AppError::internal(format_args!(
                "{CATEGORY_NOT_FOUND} specified category ID: {unknown}, userID: {user_id}, \
                 teamID: {team_id}"
            ))),
            None => Ok(()),
        }
    }
}
