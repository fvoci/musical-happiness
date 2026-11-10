//! Sidebar categories, which group a user's boards within a team.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::id::{IdKind, new_id};
use crate::serde_util::null_as_default;

/// The spacing between the sort orders of consecutive boards in a category.
pub const CATEGORY_BOARDS_SORT_ORDER_GAP: i64 = 10;

/// Whether a category was created by the server or by the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
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

/// Reason a [`Category`] is rejected before it is stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("{0}")]
pub struct CategoryError(&'static str);

/// A sidebar category of one user in one team.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Category {
    /// The category's identifier.
    pub id: String,
    /// The display name.
    pub name: String,
    /// The identifier of the user the category belongs to.
    #[serde(rename = "userID")]
    pub user_id: String,
    /// The identifier of the team the category belongs to.
    #[serde(rename = "teamID")]
    pub team_id: String,
    /// Creation time in milliseconds since the Unix epoch.
    pub create_at: i64,
    /// Last modification time in milliseconds since the Unix epoch.
    pub update_at: i64,
    /// Deletion time in milliseconds since the Unix epoch; zero if not deleted.
    pub delete_at: i64,
    /// Whether the category is collapsed in the sidebar.
    pub collapsed: bool,
    /// The position of the category among the user's categories.
    pub sort_order: i64,
    /// The sorting method applied to the boards in the category.
    pub sorting: String,
    /// Whether this is a system or a custom category.
    #[serde(rename = "type")]
    pub category_type: CategoryType,
}

impl Category {
    /// Fills in the members a new category may leave out: a fresh identifier, `now`
    /// (milliseconds since the epoch) as creation time, the creation time as modification
    /// time, and zero for a negative sort order.
    pub fn hydrate(&mut self, now: i64) {
        if self.id.is_empty() {
            self.id = new_id(IdKind::None);
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
    /// Returns a [`CategoryError`] if the identifier, name, user or team is blank.
    pub fn validate(&self) -> Result<(), CategoryError> {
        let required = [
            (&self.id, "category ID cannot be empty"),
            (&self.name, "category name cannot be empty"),
            (&self.user_id, "category user ID cannot be empty"),
            (&self.team_id, "category team id ID cannot be empty"),
        ];
        match required
            .into_iter()
            .find(|(value, _)| value.trim().is_empty())
        {
            Some((_, message)) => Err(CategoryError(message)),
            None => Ok(()),
        }
    }
}

/// A board's place in a category.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct CategoryBoardMetadata {
    /// The identifier of the board.
    #[serde(rename = "boardID")]
    pub board_id: String,
    /// Whether the user has hidden the board from the sidebar.
    pub hidden: bool,
}

/// A category together with the boards in it, in display order.
///
/// Preserved quirk: the JSON form always carries `"sortOrder": 0`, whatever the
/// category's own sort order. In the original a second, never-assigned `sortOrder` member
/// hides the category's. Clients order categories by their position in the response.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
#[serde(default)]
pub struct CategoryBoards {
    /// The category. Its members appear inline in the JSON form.
    #[serde(flatten)]
    pub category: Category,
    /// The boards in the category.
    #[serde(rename = "boardMetadata", deserialize_with = "null_as_default")]
    pub board_metadata: Vec<CategoryBoardMetadata>,
}

impl Serialize for CategoryBoards {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Wire<'a> {
            #[serde(flatten)]
            category: Category,
            #[serde(rename = "boardMetadata")]
            board_metadata: &'a [CategoryBoardMetadata],
        }

        Wire {
            category: Category {
                sort_order: 0,
                ..self.category.clone()
            },
            board_metadata: &self.board_metadata,
        }
        .serialize(serializer)
    }
}

/// The payload of the websocket message sent when a board moves between categories or is
/// hidden.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct BoardCategoryWebsocketData {
    /// The identifier of the board.
    #[serde(rename = "boardID")]
    pub board_id: String,
    /// The identifier of the category the board is now in.
    #[serde(rename = "categoryID")]
    pub category_id: String,
    /// Whether the board is hidden from the sidebar.
    pub hidden: bool,
}
