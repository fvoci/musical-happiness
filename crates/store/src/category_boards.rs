//! The boards in each sidebar category.
//!
//! A user has at most one row per board: the category the board is in, its place among
//! the boards of that category, and whether the user has hidden it. The rows refer to
//! boards by identifier only; nothing here reads the boards themselves.

use std::collections::HashSet;

use fvoci_model::{CATEGORY_BOARDS_SORT_ORDER_GAP, CategoryBoardMetadata, CategoryBoards, new_id};

use crate::Store;
use crate::categories::get_user_categories;
use crate::db::{Id, Scope, now_millis};
use crate::error::StoreError;

/// Whether a board shows in its user's sidebar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoardVisibility {
    /// The board shows.
    Visible,
    /// The user has hidden the board.
    Hidden,
}

impl Store {
    /// Returns the categories of a user in a team, in the order of
    /// [`Store::get_user_categories`], each with its boards by ascending sort order.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Corrupt`] if the stored type of a category is not one this
    /// server knows, and [`StoreError::Database`] if a query fails.
    pub async fn get_user_category_boards(
        &self,
        user_id: &str,
        team_id: &str,
    ) -> Result<Vec<CategoryBoards>, StoreError> {
        self.with_conn(async |scope| get_user_category_boards(scope, user_id, team_id).await)
            .await
    }

    /// Puts boards into a category of a user. A board named more than once counts once.
    ///
    /// A board the user has in no category yet is added visible and with sort order zero,
    /// which places it before boards that were reordered. A board the user has in another
    /// category moves to this one and keeps its sort order and its hidden flag. The
    /// category is not checked to belong to the user.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the statement fails; nothing is written in
    /// that case.
    pub async fn add_update_category_board(
        &self,
        user_id: &str,
        category_id: &str,
        board_ids: &[String],
    ) -> Result<(), StoreError> {
        self.with_conn(async |scope| {
            add_update_category_board(scope, user_id, category_id, board_ids).await
        })
        .await
    }

    /// Gives the boards of a category the sort orders 10, 11, 12 and so on, in the order
    /// of their identifiers in `new_order`, and returns `new_order`.
    ///
    /// Boards of the category that `new_order` does not name keep their sort order, which
    /// may then be that of a board the order names. Identifiers that are not boards of
    /// the category are ignored. An empty order
    /// changes nothing.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the statement fails.
    pub async fn reorder_category_boards(
        &self,
        category_id: &str,
        new_order: Vec<String>,
    ) -> Result<Vec<String>, StoreError> {
        self.with_conn(async |scope| {
            if new_order.is_empty() {
                return Ok(());
            }
            let mut sql = scope.sql("UPDATE category_boards SET sort_order = CASE board_id");
            // Preserved quirk: consecutive boards are one apart, starting at the gap. The
            // original adds the gap to the position where it means to multiply by it
            // (server/services/store/sqlstore/category_boards.go:144).
            for (sort_order, board_id) in (CATEGORY_BOARDS_SORT_ORDER_GAP..).zip(&new_order) {
                sql.push_bind(" WHEN ", Id(board_id))
                    .push_bind(" THEN ", sort_order);
            }
            // A board of the category that the order leaves out keeps its place.
            sql.push(" ELSE sort_order END")
                .push_bind(" WHERE category_id = ", Id(category_id));
            scope.execute(sql).await?;
            Ok(())
        })
        .await?;
        Ok(new_order)
    }

    /// Hides a board in a category of a user, or shows it again.
    ///
    /// Succeeds and changes nothing if the user does not have the board in the category.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the statement fails.
    pub async fn set_board_visibility(
        &self,
        user_id: &str,
        category_id: &str,
        board_id: &str,
        visibility: BoardVisibility,
    ) -> Result<(), StoreError> {
        self.with_conn(async |scope| {
            let mut sql = scope.sql("UPDATE category_boards SET hidden = ");
            sql.bind(visibility == BoardVisibility::Hidden)
                .push_bind(" WHERE user_id = ", Id(user_id))
                .push_bind(" AND category_id = ", Id(category_id))
                .push_bind(" AND board_id = ", Id(board_id));
            scope.execute(sql).await?;
            Ok(())
        })
        .await
    }
}

pub(crate) async fn get_user_category_boards(
    scope: &mut Scope<'_>,
    user_id: &str,
    team_id: &str,
) -> Result<Vec<CategoryBoards>, StoreError> {
    let categories = get_user_categories(scope, user_id, team_id).await?;
    let mut user_category_boards = Vec::with_capacity(categories.len());
    for category in categories {
        let board_metadata = category_board_metadata(scope, &category.id).await?;
        user_category_boards.push(CategoryBoards {
            category,
            board_metadata,
        });
    }
    Ok(user_category_boards)
}

/// Returns the boards of a category by ascending sort order. Boards with equal sort
/// orders, such as those never reordered, come in no particular order.
async fn category_board_metadata(
    scope: &mut Scope<'_>,
    category_id: &str,
) -> Result<Vec<CategoryBoardMetadata>, StoreError> {
    let mut sql = scope.sql("SELECT board_id, hidden FROM category_boards WHERE category_id = ");
    // Preserved quirk: the rows are not narrowed to the user the category belongs to, so
    // a board that another user put into this category is listed with it
    // (server/services/store/sqlstore/category_boards.go:42).
    sql.bind(Id(category_id)).push(" ORDER BY sort_order");
    scope
        .fetch_all(sql)
        .await?
        .iter()
        .map(|row| {
            Ok(CategoryBoardMetadata {
                board_id: row.id(0)?,
                hidden: row.bool(1)?,
            })
        })
        .collect()
}

pub(crate) async fn add_update_category_board(
    scope: &mut Scope<'_>,
    user_id: &str,
    category_id: &str,
    board_ids: &[String],
) -> Result<(), StoreError> {
    // One statement must not name a board twice: PostgreSQL refuses an upsert that would
    // touch the same row a second time. The rows are written in the order the boards are
    // first named; the original writes them in the random order of a hash map, and no
    // reader depends on either.
    let mut seen = HashSet::with_capacity(board_ids.len());
    let board_ids: Vec<&str> = board_ids
        .iter()
        .map(String::as_str)
        .filter(|&board_id| seen.insert(board_id))
        .collect();
    if board_ids.is_empty() {
        return Ok(());
    }

    let now = now_millis();
    let mut sql = scope.sql("INSERT INTO category_boards (id, user_id, category_id, board_id, create_at, update_at, sort_order, \
         hidden) VALUES ");
    for (index, board_id) in board_ids.into_iter().enumerate() {
        sql.push(if index == 0 { "(" } else { ", (" })
            .bind(Id(&new_id()))
            .push_bind(", ", Id(user_id))
            .push_bind(", ", Id(category_id))
            .push_bind(", ", Id(board_id))
            .push_bind(", ", now)
            .push_bind(", ", now)
            .push(", 0, false)");
    }
    // Preserved quirk: a board that changes category keeps the sort order it had in the
    // old one, and its hidden flag (server/services/store/sqlstore/category_boards.go:105).
    sql.push(
        " ON CONFLICT (user_id, board_id) DO UPDATE SET category_id = EXCLUDED.category_id, \
         update_at = EXCLUDED.update_at",
    );
    scope.execute(sql).await?;
    Ok(())
}
