//! Boards and board membership.

use std::collections::HashMap;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::JsonMap;
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum BoardRole {
    /// No role. As a board's minimum role it means members keep their own role.
    #[default]
    #[serde(rename = "")]
    None,
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
    /// Returns the wire and database form; [`BoardRole::None`] is the empty string.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "",
            Self::Viewer => "viewer",
            Self::Commenter => "commenter",
            Self::Editor => "editor",
            Self::Admin => "admin",
        }
    }
}

/// Reasons a [`Board`] is rejected before it is stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum BoardError {
    /// The board names no team.
    #[error("empty-team-id")]
    EmptyTeamId,
}

/// A board: a set of blocks and the property definitions of its cards.
///
/// `type` is required when decoding; every other member defaults to its empty value.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Board {
    /// The board's identifier.
    #[serde(default)]
    pub id: String,
    /// The identifier of the team the board belongs to.
    #[serde(default)]
    pub team_id: String,
    /// The identifier of the channel the board was created from; unused, kept for the
    /// wire format.
    #[serde(default)]
    pub channel_id: String,
    /// The identifier of the user who created the board.
    #[serde(default)]
    pub created_by: String,
    /// The identifier of the user who last modified the board.
    #[serde(default)]
    pub modified_by: String,
    /// Whether the board is open or private.
    #[serde(rename = "type")]
    pub board_type: BoardType,
    /// The role every member has at least, whatever their own role.
    #[serde(default)]
    pub minimum_role: BoardRole,
    /// The board's title.
    #[serde(default)]
    pub title: String,
    /// The board's description.
    #[serde(default)]
    pub description: String,
    /// The board's icon, usually one emoji.
    #[serde(default)]
    pub icon: String,
    /// Whether the description is shown in the interface.
    #[serde(default)]
    pub show_description: bool,
    /// Whether the board is a template.
    #[serde(default)]
    pub is_template: bool,
    /// The version of the built-in template this board came from.
    #[serde(default)]
    pub template_version: i64,
    /// Free-form properties of the board itself.
    #[serde(default, deserialize_with = "null_as_default")]
    pub properties: JsonMap,
    /// The property definitions of the board's cards, in display order.
    #[serde(default, deserialize_with = "null_as_default")]
    pub card_properties: Vec<JsonMap>,
    /// Creation time in milliseconds since the Unix epoch.
    #[serde(default)]
    pub create_at: i64,
    /// Last modification time in milliseconds since the Unix epoch.
    #[serde(default)]
    pub update_at: i64,
    /// Deletion time in milliseconds since the Unix epoch; zero if not deleted.
    #[serde(default)]
    pub delete_at: i64,
}

impl Board {
    /// Checks that the board may be stored.
    ///
    /// # Errors
    ///
    /// Returns [`BoardError::EmptyTeamId`] if the board names no team.
    pub fn validate(&self) -> Result<(), BoardError> {
        if self.team_id.is_empty() {
            return Err(BoardError::EmptyTeamId);
        }
        Ok(())
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
    pub minimum_role: Option<BoardRole>,
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
    /// The new channel identifier.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel_id: Option<String>,
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
        if let Some(channel_id) = &self.channel_id {
            board.channel_id.clone_from(channel_id);
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
        // Preserved quirk: an identifier can be in `order` twice (it was already duplicated
        // on the board, or the patch deletes it and adds it back), and the original then
        // emits the definition once per occurrence.
        board.card_properties = order
            .iter()
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
    pub board_id: String,
    /// The identifier of the user.
    pub user_id: String,
    /// The user's independent roles on the board; unused, kept for the wire format.
    pub roles: String,
    /// The board's minimum role at the time the membership was read.
    pub minimum_role: String,
    /// The user administers the board.
    pub scheme_admin: bool,
    /// The user may edit the board.
    pub scheme_editor: bool,
    /// The user may comment on the board's cards.
    pub scheme_commenter: bool,
    /// The user may view the board.
    pub scheme_viewer: bool,
    /// The membership was derived from a group rather than granted directly.
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
