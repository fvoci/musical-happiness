//! Sidebar categories, which group a user's boards within a team.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::id::new_id;
use crate::refusal::{RefusalReason, Refusals};
use crate::serde_util::null_as_default;

/// The spacing between the sort orders of consecutive boards in a category.
pub const CATEGORY_BOARDS_SORT_ORDER_GAP: i64 = 10;

/// Whether a category was created by the server or by the user.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, utoipa::ToSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum CategoryType {
    /// A built-in category, such as the default "Boards" one.
    System,
    /// A category the user created.
    #[default]
    Custom,
}

impl CategoryType {
    /// Returns the wire and database form.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Custom => "custom",
        }
    }
}

/// Why a [`Category`] is rejected before it is stored: every member that is refused.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{0}")]
pub struct CategoryError(Refusals);

impl CategoryError {
    /// The members that were refused.
    pub fn into_refusals(self) -> Refusals {
        self.0
    }
}

/// A sidebar category of one user in one team.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(default, rename_all = "camelCase")]
pub struct Category {
    /// The category's identifier.
    #[serde(deserialize_with = "null_as_default")]
    pub id: String,
    /// The display name.
    #[serde(deserialize_with = "null_as_default")]
    pub name: String,
    /// The identifier of the user the category belongs to.
    #[serde(deserialize_with = "null_as_default")]
    pub user_id: String,
    /// The identifier of the team the category belongs to.
    #[serde(deserialize_with = "null_as_default")]
    pub team_id: String,
    /// Creation time in milliseconds since the Unix epoch.
    #[serde(deserialize_with = "null_as_default")]
    pub create_at: i64,
    /// Last modification time in milliseconds since the Unix epoch.
    #[serde(deserialize_with = "null_as_default")]
    pub update_at: i64,
    /// Deletion time in milliseconds since the Unix epoch; zero if not deleted.
    #[serde(deserialize_with = "null_as_default")]
    pub delete_at: i64,
    /// Whether the category is collapsed in the sidebar.
    #[serde(deserialize_with = "null_as_default")]
    pub collapsed: bool,
    /// The position of the category among the user's categories.
    #[serde(deserialize_with = "null_as_default")]
    pub sort_order: i64,
    /// The sorting method applied to the boards in the category. Nothing stores one;
    /// not part of the JSON form.
    #[serde(skip)]
    pub sorting: String,
    /// Whether this is a system or a custom category.
    #[serde(rename = "type")]
    #[serde(deserialize_with = "null_as_default")]
    pub category_type: CategoryType,
}

impl Category {
    /// Fills in the members a new category may leave out: a fresh identifier, `now`
    /// (milliseconds since the epoch) as creation time, the creation time as modification
    /// time, and zero for a negative sort order.
    pub fn hydrate(&mut self, now: i64) {
        if self.id.is_empty() {
            self.id = new_id();
        }
        if self.create_at == 0 {
            self.create_at = now;
        }
        if self.update_at == 0 {
            self.update_at = self.create_at;
        }
        if self.sort_order < 0 {
            self.sort_order = 0;
        }
    }

    /// Checks that the category may be stored.
    ///
    /// # Errors
    ///
    /// Returns a [`CategoryError`] with every one of the identifier, name, user and team
    /// that is blank.
    pub fn validate(&self) -> Result<(), CategoryError> {
        let required = [
            (&self.id, "id", "category ID cannot be empty"),
            (&self.name, "name", "category name cannot be empty"),
            (&self.user_id, "userId", "category user ID cannot be empty"),
            (
                &self.team_id,
                "teamId",
                "category team id ID cannot be empty",
            ),
        ];
        let mut refused = Refusals::new();
        for (value, member, message) in required {
            if value.trim().is_empty() {
                refused.refuse(member, RefusalReason::Required, message);
            }
        }
        refused.into_result().map_err(CategoryError)
    }
}

/// A board's place in a category.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(default, rename_all = "camelCase")]
pub struct CategoryBoardMetadata {
    /// The identifier of the board.
    #[serde(deserialize_with = "null_as_default")]
    pub board_id: String,
    /// Whether the user has hidden the board from the sidebar.
    #[serde(deserialize_with = "null_as_default")]
    pub hidden: bool,
}

/// A category together with the boards in it, in display order.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(default, rename_all = "camelCase")]
pub struct CategoryBoards {
    /// The category. Its members appear inline in the JSON form.
    #[serde(flatten)]
    pub category: Category,
    /// The boards in the category.
    #[serde(deserialize_with = "null_as_default")]
    pub board_metadata: Vec<CategoryBoardMetadata>,
}

/// The payload of the websocket message sent when a board moves between categories or is
/// hidden.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct BoardCategoryWebsocketData {
    /// The identifier of the board.
    pub board_id: String,
    /// The identifier of the category the board is now in.
    pub category_id: String,
    /// Whether the board is hidden from the sidebar.
    pub hidden: bool,
}
