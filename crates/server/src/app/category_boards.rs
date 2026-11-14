//! Which boards a user has filed under which sidebar category.

use std::collections::{HashMap, HashSet};

use fvoci_model::{
    Board, BoardCategoryWebsocketData, BoardKinds, CATEGORY_BOARDS_SORT_ORDER_GAP, Category,
    CategoryBoardMetadata, CategoryBoards, CategoryType,
};
use fvoci_store::{BoardVisibility, PublicBoards};

use super::App;
use super::category::CATEGORY;
use crate::error::AppError;

/// The name of the category every user has, which collects the boards filed nowhere else.
pub const DEFAULT_CATEGORY_BOARDS: &str = fvoci_model::DEFAULT_CATEGORY_NAME;

/// The default category of a user who has `existing` categories in the workspace: it
/// goes after them.
fn default_category(user_id: &str, workspace_id: &str, existing: usize) -> Category {
    Category {
        name: DEFAULT_CATEGORY_BOARDS.to_owned(),
        user_id: user_id.to_owned(),
        workspace_id: workspace_id.to_owned(),
        collapsed: false,
        category_type: CategoryType::System,
        sort_order: i64::try_from(existing)
            .unwrap_or(i64::MAX)
            .saturating_mul(CATEGORY_BOARDS_SORT_ORDER_GAP),
        ..Category::default()
    }
}

fn has_default_category(category_boards: &[CategoryBoards]) -> bool {
    category_boards
        .iter()
        .any(|entry| entry.category.name == DEFAULT_CATEGORY_BOARDS)
}

impl App {
    /// Returns a user's categories in a workspace with the boards in each, creating the
    /// default category first if the user does not have it yet.
    ///
    /// # Errors
    ///
    /// Returns the store's error if the categories cannot be read or the default
    /// category cannot be created.
    pub async fn user_category_boards(
        &self,
        user_id: &str,
        workspace_id: &str,
    ) -> Result<Vec<CategoryBoards>, AppError> {
        let category_boards = self
            .store()
            .get_user_category_boards(user_id, workspace_id)
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
            .get_user_category_boards(user_id, workspace_id)
            .await?;
        if !has_default_category(&category_boards) {
            self.create_boards_category(user_id, workspace_id, &category_boards)
                .await?;
            // Read again: making the category moved the others, and the list is in the
            // order of the categories as they are now.
            category_boards = self
                .store()
                .get_user_category_boards(user_id, workspace_id)
                .await?;
        }
        Ok(category_boards)
    }

    /// The sidebar of a guest of the workspace: their categories as
    /// [`App::user_category_boards`] has them, with the boards they are on now and no
    /// other. A board they filed while they worked in the workspace, or before they
    /// left it and came back as a guest, is not theirs to know of any more.
    ///
    /// # Errors
    ///
    /// Returns the store's error if the categories or the boards cannot be read.
    pub async fn category_boards_of_guest(
        &self,
        user_id: &str,
        workspace_id: &str,
    ) -> Result<Vec<CategoryBoards>, AppError> {
        let mut category_boards = self.user_category_boards(user_id, workspace_id).await?;
        let on: HashSet<String> = self
            .readable_workspace_boards(user_id, workspace_id, fvoci_model::WorkspaceRole::Guest)
            .await?
            .into_iter()
            .map(|board| board.id)
            .collect();
        for category in &mut category_boards {
            category
                .board_metadata
                .retain(|entry| on.contains(&entry.board_id));
        }
        Ok(category_boards)
    }

    /// Creates the default category and files under it every board the user is an
    /// explicit member of that is in no category yet.
    async fn create_boards_category(
        &self,
        user_id: &str,
        workspace_id: &str,
        existing: &[CategoryBoards],
    ) -> Result<CategoryBoards, AppError> {
        let category = default_category(user_id, workspace_id, existing.len());
        let created = self.create_category(category).await.map_err(|error| {
            error.context("createBoardsCategory default category creation failed")
        })?;

        // The boards are read before the memberships. The original reads them the other
        // way round, so a board created by another request in between is in its list of
        // boards but not in its list of memberships, and the request fails.
        // Boards and wikis alike, whichever the request is about that makes the category:
        // it is made once, and files what the user is on.
        let boards = self
            .store()
            .get_boards_for_user_and_workspace(
                user_id,
                workspace_id,
                PublicBoards::Exclude,
                BoardKinds::Every,
            )
            .await
            .map_err(|error| {
                AppError::from(error)
                    .context("createBoardsCategory error fetching user's workspace's boards")
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
            .map(|member| {
                (
                    member.board_id,
                    member.synthetic && member.inherited_role.is_none(),
                )
            })
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
                workspace_id,
                user_id,
                &created.id,
                &board_ids,
            ))
            .await
            .map_err(|error| {
                error.context(format_args!(
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
    /// Each board must be one of the workspace or a built-in template; whether the user may
    /// see it is the caller's to check.
    ///
    /// # Errors
    ///
    /// Returns a not-found error if the category is not one of the user's categories in
    /// the workspace, or a board is not there or is one of another workspace; or the store's error
    /// if the boards cannot be filed. Nothing is written in any of these cases.
    pub async fn add_update_user_category_board(
        &self,
        workspace_id: &str,
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
        self.user_category(workspace_id, user_id, category_id)
            .await?;
        for board_id in board_ids {
            self.require_board_of_workspace(workspace_id, board_id)
                .await?;
        }
        self.store()
            .add_update_category_board(user_id, category_id, board_ids)
            .await?;

        let category_boards = self.user_category_boards(user_id, workspace_id).await?;
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
            .broadcast_category_board_change(workspace_id, user_id, &payload)
            .await;
        Ok(())
    }

    /// Puts the boards of one of a user's categories into the given order and tells the
    /// user's connection about it. Returns the order.
    ///
    /// # Errors
    ///
    /// Returns a not-found error if the category is not one of the user's categories in
    /// the workspace, a bad request if `new_order` does not name exactly as many boards as
    /// the category has, or names one that is not in it, and the store's error if a
    /// read or write fails.
    pub async fn reorder_category_boards(
        &self,
        user_id: &str,
        workspace_id: &str,
        category_id: &str,
        new_order: Vec<String>,
    ) -> Result<Vec<String>, AppError> {
        self.verify_new_category_boards_match_existing(
            user_id,
            workspace_id,
            category_id,
            &new_order,
        )
        .await?;
        let new_order = self
            .store()
            .reorder_category_boards(category_id, new_order)
            .await?;
        self.ws()
            .broadcast_category_boards_reorder(workspace_id, user_id, category_id, &new_order)
            .await;
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
        workspace_id: &str,
        category_id: &str,
        new_order: &[String],
    ) -> Result<(), AppError> {
        let category_boards = self.user_category_boards(user_id, workspace_id).await?;
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
    /// The category must be the user's and the board one of its workspace or a built-in
    /// template (both checked here), and the route has checked that the user sees the
    /// board. Preserved quirk: if the user does not have the board in the category,
    /// nothing changes, and the user's connection is told of the change all the same
    /// (server/app/category_boards.go:265-279).
    ///
    /// # Errors
    ///
    /// Returns a not-found error if the category is not one of the user's categories in
    /// the workspace, or the board is not there or is one of another workspace; and the store's
    /// error if the write fails.
    pub async fn set_board_visibility(
        &self,
        workspace_id: &str,
        user_id: &str,
        category_id: &str,
        board_id: &str,
        visibility: BoardVisibility,
    ) -> Result<(), AppError> {
        self.user_category(workspace_id, user_id, category_id)
            .await?;
        self.require_board_of_workspace(workspace_id, board_id)
            .await?;
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
            .broadcast_category_board_change(workspace_id, user_id, &[change])
            .await;
        Ok(())
    }

    /// Fails with the not-found error of a board that is not there unless the board is
    /// one a category of the workspace takes: one of the workspace, or a built-in template, which
    /// every workspace's categories take. A user who works in two workspaces of an organization
    /// sees the boards of both; a category holds those of its own.
    ///
    /// # Errors
    ///
    /// Returns a not-found error for a board that is not there or is another workspace's, and
    /// the store's error if the board cannot be read.
    pub(crate) async fn require_board_of_workspace(
        &self,
        workspace_id: &str,
        board_id: &str,
    ) -> Result<(), AppError> {
        // The workspace named, which may be another than the handle's (a board filed
        // back in its own): compared here, not by `App::board`.
        let board = match self.store().get_board(board_id).await {
            Ok(board) => board,
            Err(error) if error.is_not_found() => return Err(AppError::not_found("board")),
            Err(error) => return Err(error.into()),
        };
        if category_of_workspace_takes(&board, workspace_id) {
            Ok(())
        } else {
            Err(AppError::not_found("board"))
        }
    }
}

/// Whether a category of the workspace takes the board: one of the workspace, or a built-in
/// template.
pub(super) fn category_of_workspace_takes(board: &Board, workspace_id: &str) -> bool {
    board.workspace_id == workspace_id || board.is_builtin
}
