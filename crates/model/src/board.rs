//! Boards and board membership.

use std::collections::{HashMap, HashSet};
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::JsonMap;
use crate::card_property::{CardPropertyError, validate_card_property_definitions};
use crate::serde_util::null_as_default;

/// Who may see and join a board.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum BoardType {
    /// Any member of the team may find and join the board.
    #[default]
    #[serde(rename = "O")]
    Open,
    /// Only invited users may access the board.
    #[serde(rename = "P")]
    Private,
}

impl BoardType {
    /// Returns the one-letter wire and database form.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Open => "O",
            Self::Private => "P",
        }
    }
}

/// A role on a board, from least to most privileged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum BoardRole {
    /// May view the board.
    #[serde(rename = "viewer")]
    Viewer,
    /// May also comment on cards.
    #[serde(rename = "commenter")]
    Commenter,
    /// May also edit cards and properties.
    #[serde(rename = "editor")]
    Editor,
    /// May also manage the board, its members and its sharing.
    #[serde(rename = "admin")]
    Admin,
}

impl BoardRole {
    /// Returns the wire form.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Viewer => "viewer",
            Self::Commenter => "commenter",
            Self::Editor => "editor",
            Self::Admin => "admin",
        }
    }
}

/// The minimum role of a board: the role a user gets who joins the board, or is added
/// to it by somebody who does not manage its roles.
///
/// A board always has one, and it is never admin: nobody becomes an administrator of a
/// board by joining it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum MinimumRole {
    /// Whoever joins may view the board.
    #[serde(rename = "viewer")]
    Viewer,
    /// Whoever joins may also comment on cards.
    #[serde(rename = "commenter")]
    Commenter,
    /// Whoever joins may also edit cards and properties. What a new board starts with.
    #[default]
    #[serde(rename = "editor")]
    Editor,
}

impl MinimumRole {
    /// Every minimum role there is.
    pub const ALL: [Self; 3] = [Self::Viewer, Self::Commenter, Self::Editor];

    /// Returns the wire and database form.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Viewer => "viewer",
            Self::Commenter => "commenter",
            Self::Editor => "editor",
        }
    }
}

impl From<MinimumRole> for BoardRole {
    fn from(role: MinimumRole) -> Self {
        match role {
            MinimumRole::Viewer => Self::Viewer,
            MinimumRole::Commenter => Self::Commenter,
            MinimumRole::Editor => Self::Editor,
        }
    }
}

/// Reasons a [`Board`] is rejected before it is stored.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum BoardError {
    /// The board names no team.
    #[error("empty-team-id")]
    EmptyTeamId,
    /// A card property definition of the board is malformed.
    #[error(transparent)]
    InvalidCardProperty(#[from] CardPropertyError),
}

/// A board: a set of blocks and the property definitions of its cards.
///
/// `type` is required when decoding; every other member defaults to its empty value.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Board {
    /// The board's identifier.
    #[serde(default)]
    #[serde(deserialize_with = "null_as_default")]
    pub id: String,
    /// The identifier of the team the board belongs to.
    #[serde(default)]
    #[serde(deserialize_with = "null_as_default")]
    pub team_id: String,
    /// The identifier of the user who created the board; empty for a built-in template,
    /// which no user made.
    #[serde(default)]
    #[serde(deserialize_with = "null_as_default")]
    pub created_by: String,
    /// The identifier of the user who last modified the board.
    #[serde(default)]
    #[serde(deserialize_with = "null_as_default")]
    pub modified_by: String,
    /// Whether the board is open or private.
    #[serde(rename = "type")]
    pub board_type: BoardType,
    /// The role every member has at least, whatever their own role.
    #[serde(default)]
    #[serde(deserialize_with = "null_as_default")]
    pub minimum_role: MinimumRole,
    /// The board's title.
    #[serde(default)]
    #[serde(deserialize_with = "null_as_default")]
    pub title: String,
    /// The board's description.
    #[serde(default)]
    #[serde(deserialize_with = "null_as_default")]
    pub description: String,
    /// The board's icon, usually one emoji.
    #[serde(default)]
    #[serde(deserialize_with = "null_as_default")]
    pub icon: String,
    /// Whether the description is shown in the interface.
    #[serde(default)]
    #[serde(deserialize_with = "null_as_default")]
    pub show_description: bool,
    /// Whether the board is a template.
    #[serde(default)]
    #[serde(deserialize_with = "null_as_default")]
    pub is_template: bool,
    /// Whether the board is one of the templates the server ships with. They are
    /// offered to every team and nobody can change them. The server decides this:
    /// what a client sends here is ignored.
    #[serde(default)]
    #[serde(deserialize_with = "null_as_default")]
    pub is_builtin: bool,
    /// The version of the built-in template this board came from.
    #[serde(default)]
    #[serde(deserialize_with = "null_as_default")]
    pub template_version: i64,
    /// Free-form properties of the board itself.
    #[serde(default, deserialize_with = "null_as_default")]
    pub properties: JsonMap,
    /// The property definitions of the board's cards, in display order.
    #[serde(default, deserialize_with = "null_as_default")]
    pub card_properties: Vec<JsonMap>,
    /// Creation time in milliseconds since the Unix epoch.
    #[serde(default)]
    #[serde(deserialize_with = "null_as_default")]
    pub create_at: i64,
    /// Last modification time in milliseconds since the Unix epoch.
    #[serde(default)]
    #[serde(deserialize_with = "null_as_default")]
    pub update_at: i64,
    /// Deletion time in milliseconds since the Unix epoch; zero if not deleted.
    #[serde(default)]
    #[serde(deserialize_with = "null_as_default")]
    pub delete_at: i64,
}

impl Board {
    /// Checks that the board may be stored.
    ///
    /// # Errors
    ///
    /// Returns [`BoardError::EmptyTeamId`] if the board names no team, or
    /// [`BoardError::InvalidCardProperty`] if a card property definition is malformed.
    pub fn validate(&self) -> Result<(), BoardError> {
        if self.team_id.is_empty() {
            return Err(BoardError::EmptyTeamId);
        }
        self.validate_card_properties()?;
        Ok(())
    }

    /// Checks the card property definitions of the board alone, for where a board is
    /// stored that came no way that checks the rest.
    ///
    /// # Errors
    ///
    /// Returns the error of [`validate_card_property_definitions`].
    pub fn validate_card_properties(&self) -> Result<(), CardPropertyError> {
        validate_card_property_definitions(&self.card_properties)
    }
}

/// A partial update of a [`Board`]. Absent members leave the board unchanged.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct BoardPatch {
    /// The new board type.
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub board_type: Option<BoardType>,
    /// The new minimum role.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub minimum_role: Option<MinimumRole>,
    /// The new title.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// The new description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// The new icon.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// Whether the description is shown.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub show_description: Option<bool>,
    /// Board properties to set.
    #[serde(deserialize_with = "null_as_default")]
    pub updated_properties: JsonMap,
    /// Keys of board properties to remove.
    #[serde(deserialize_with = "null_as_default")]
    pub deleted_properties: Vec<String>,
    /// Card property definitions to add or replace, matched by their `id`.
    #[serde(deserialize_with = "null_as_default")]
    pub updated_card_properties: Vec<JsonMap>,
    /// Identifiers of card property definitions to remove.
    #[serde(deserialize_with = "null_as_default")]
    pub deleted_card_properties: Vec<String>,
}

impl BoardPatch {
    /// Checks the card property definitions the patch sets. Applying the patch would
    /// drop a definition without an `id` silently; a client that sends one is told.
    ///
    /// # Errors
    ///
    /// Returns the error of [`validate_card_property_definitions`].
    pub fn validate(&self) -> Result<(), CardPropertyError> {
        validate_card_property_definitions(&self.updated_card_properties)
    }

    /// Applies the patch to `board`.
    ///
    /// Card properties keep their existing order; replaced definitions stay in place and
    /// new ones are appended. Deletions are applied before updates, so a definition listed
    /// in both is kept with its updated value, and then appears twice: once at its old
    /// position and once at the end. Whenever the patch touches card properties, existing
    /// definitions without a string `id` are dropped.
    pub fn apply(&self, board: &mut Board) {
        if let Some(board_type) = self.board_type {
            board.board_type = board_type;
        }
        if let Some(title) = &self.title {
            board.title.clone_from(title);
        }
        if let Some(minimum_role) = self.minimum_role {
            board.minimum_role = minimum_role;
        }
        if let Some(description) = &self.description {
            board.description.clone_from(description);
        }
        if let Some(icon) = &self.icon {
            board.icon.clone_from(icon);
        }
        if let Some(show_description) = self.show_description {
            board.show_description = show_description;
        }
        for (key, value) in &self.updated_properties {
            board.properties.insert(key.clone(), value.clone());
        }
        for key in &self.deleted_properties {
            board.properties.remove(key);
        }
        if !self.updated_card_properties.is_empty() || !self.deleted_card_properties.is_empty() {
            self.apply_card_properties(board);
        }
    }

    fn apply_card_properties(&self, board: &mut Board) {
        let mut order: Vec<String> = Vec::new();
        let mut by_id: HashMap<String, JsonMap> = HashMap::new();
        for property in std::mem::take(&mut board.card_properties) {
            if let Some(id) = property_id(&property) {
                order.push(id.clone());
                by_id.insert(id, property);
            }
        }
        for id in &self.deleted_card_properties {
            by_id.remove(id);
        }
        for property in &self.updated_card_properties {
            if let Some(id) = property_id(property) {
                if !by_id.contains_key(&id) {
                    order.push(id.clone());
                }
                by_id.insert(id, property.clone());
            }
        }
        // An identifier can be in `order` twice: the patch deletes it and adds it back.
        // The definition is emitted once, at its first place. (The original emits it once
        // per occurrence, which leaves a board with two definitions of one identifier.)
        let mut emitted = HashSet::new();
        board.card_properties = order
            .iter()
            .filter(|id| emitted.insert(id.as_str()))
            .filter_map(|id| by_id.get(id).cloned())
            .collect();
    }
}

/// Returns the string `id` of a card property definition, if it has one.
fn property_id(property: &JsonMap) -> Option<String> {
    match property.get("id") {
        Some(Value::String(id)) => Some(id.clone()),
        _ => None,
    }
}

/// A user's membership of a board.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
#[expect(
    clippy::struct_excessive_bools,
    reason = "the four scheme flags and `synthetic` are separate members of the wire format"
)]
pub struct BoardMember {
    /// The identifier of the board.
    #[serde(deserialize_with = "null_as_default")]
    pub board_id: String,
    /// The identifier of the user.
    #[serde(deserialize_with = "null_as_default")]
    pub user_id: String,
    /// The user's independent roles on the board; unused, kept for the wire format.
    #[serde(deserialize_with = "null_as_default")]
    pub roles: String,
    /// The user administers the board.
    #[serde(deserialize_with = "null_as_default")]
    pub scheme_admin: bool,
    /// The user may edit the board.
    #[serde(deserialize_with = "null_as_default")]
    pub scheme_editor: bool,
    /// The user may comment on the board's cards.
    #[serde(deserialize_with = "null_as_default")]
    pub scheme_commenter: bool,
    /// The user may view the board.
    #[serde(deserialize_with = "null_as_default")]
    pub scheme_viewer: bool,
    /// The membership was derived from a group rather than granted directly.
    #[serde(deserialize_with = "null_as_default")]
    pub synthetic: bool,
}

/// The board member that a board search matches against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BoardSearchField {
    /// Match the search term against board titles.
    Title,
    /// Match the search term against the names of card properties.
    PropertyName,
}

/// Error returned when a string names no board search field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("invalid board search field")]
pub struct ParseBoardSearchFieldError;

impl FromStr for BoardSearchField {
    type Err = ParseBoardSearchFieldError;

    fn from_str(field: &str) -> Result<Self, Self::Err> {
        match field {
            "title" => Ok(Self::Title),
            "property_name" => Ok(Self::PropertyName),
            _ => Err(ParseBoardSearchFieldError),
        }
    }
}
