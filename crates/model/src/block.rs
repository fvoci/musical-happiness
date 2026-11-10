//! Blocks: the basic unit of board content (cards, views, text, images and so on).

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::JsonMap;
use crate::body::{Body, BodyChange};
use crate::card_property::{CardPropertyError, validate_block_properties};
use crate::serde_util::null_as_default;

/// Maximum number of characters in a block title.
///
/// A quarter of the 65535-byte limit of a `MySQL` `TEXT` column, assuming the worst-case
/// four bytes per character.
pub const BLOCK_TITLE_MAX_CHARS: usize = 65535 / 4;

/// Maximum number of characters in a block's `fields` once serialized to JSON.
pub const BLOCK_FIELDS_MAX_CHARS: usize = 800_000;

/// The type of a block.
///
/// The server stores and returns any type string a client sends (the web app has types
/// such as `h1` and `list-item` that the server knows nothing about), so unrecognized
/// strings are kept verbatim in [`BlockType::Other`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(from = "String", into = "String")]
pub enum BlockType {
    /// A legacy board block.
    Board,
    /// A card.
    Card,
    /// A board view.
    View,
    /// A text content block.
    Text,
    /// A checkbox content block.
    Checkbox,
    /// A comment on a card.
    Comment,
    /// An image content block.
    Image,
    /// A file attachment on a card.
    Attachment,
    /// A divider content block.
    Divider,
    /// Any other type string, kept as sent. The empty string means "no type".
    Other(String),
}

impl BlockType {
    /// Returns the wire string of this type.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Board => "board",
            Self::Card => "card",
            Self::View => "view",
            Self::Text => "text",
            Self::Checkbox => "checkbox",
            Self::Comment => "comment",
            Self::Image => "image",
            Self::Attachment => "attachment",
            Self::Divider => "divider",
            Self::Other(other) => other,
        }
    }

    /// Returns `true` if this is the empty type string, which a block must not have.
    pub fn is_empty(&self) -> bool {
        self.as_str().is_empty()
    }

    /// Maps an exact wire string to a known type, if it is one.
    fn known(name: &str) -> Option<Self> {
        Some(match name {
            "board" => Self::Board,
            "card" => Self::Card,
            "view" => Self::View,
            "text" => Self::Text,
            "checkbox" => Self::Checkbox,
            "comment" => Self::Comment,
            "image" => Self::Image,
            "attachment" => Self::Attachment,
            "divider" => Self::Divider,
            _ => return None,
        })
    }
}

impl Default for BlockType {
    /// The empty type string, which is what a block decoded without a `type` has.
    fn default() -> Self {
        Self::Other(String::new())
    }
}

impl fmt::Display for BlockType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl From<String> for BlockType {
    fn from(name: String) -> Self {
        Self::known(&name).unwrap_or(Self::Other(name))
    }
}

impl From<&str> for BlockType {
    fn from(name: &str) -> Self {
        Self::known(name).unwrap_or_else(|| Self::Other(name.to_owned()))
    }
}

impl From<BlockType> for String {
    fn from(block_type: BlockType) -> Self {
        match block_type {
            BlockType::Other(other) => other,
            known => known.as_str().to_owned(),
        }
    }
}

/// Error returned when a string names no block type the server recognizes.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{0} is an invalid block type.")]
pub struct ParseBlockTypeError(String);

impl FromStr for BlockType {
    type Err = ParseBlockTypeError;

    /// Parses one of the nine known block types, ignoring ASCII case.
    ///
    /// Unlike the `From<&str>` conversion, which keeps unknown strings, this rejects them.
    /// It is the strict form used for the `type` query parameter.
    fn from_str(name: &str) -> Result<Self, Self::Err> {
        Self::known(&name.to_lowercase()).ok_or_else(|| ParseBlockTypeError(name.to_owned()))
    }
}

/// Reasons a [`Block`] is rejected before it is stored.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum BlockError {
    /// The block names no board.
    #[error("boardID is empty")]
    EmptyBoardId,
    /// The block's parent is not a block of the block's board.
    #[error("the parent is not a block of the board")]
    ParentNotOnBoard,
    /// The block's new parent is the block itself or a block below it.
    #[error("the parent is the block itself or below it")]
    ParentBelowBlock,
    /// The title is longer than [`BLOCK_TITLE_MAX_CHARS`].
    #[error("block title size limit exceeded")]
    TitleSizeLimitExceeded,
    /// The serialized fields are longer than [`BLOCK_FIELDS_MAX_CHARS`].
    #[error("block fields size limit exceeded")]
    FieldsSizeLimitExceeded,
    /// The `properties` among the fields are not well-formed card property values.
    #[error(transparent)]
    InvalidCardProperties(#[from] CardPropertyError),
}

/// The basic data unit of a board.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(default, rename_all = "camelCase")]
pub struct Block {
    /// The block's identifier.
    #[serde(deserialize_with = "null_as_default")]
    pub id: String,
    /// The identifier of the parent block; empty for root blocks.
    #[serde(deserialize_with = "null_as_default")]
    pub parent_id: String,
    /// The identifier of the user who created the block.
    #[serde(deserialize_with = "null_as_default")]
    pub created_by: String,
    /// The identifier of the user who last modified the block.
    #[serde(deserialize_with = "null_as_default")]
    pub modified_by: String,
    /// The schema version of the block.
    #[serde(deserialize_with = "null_as_default")]
    pub schema: i64,
    /// The block type.
    #[serde(rename = "type")]
    #[serde(deserialize_with = "null_as_default")]
    #[schema(value_type = String)]
    pub block_type: BlockType,
    /// The display title.
    #[serde(deserialize_with = "null_as_default")]
    pub title: String,
    /// Type-specific data, free-form.
    #[serde(deserialize_with = "null_as_default")]
    #[schema(value_type = Object)]
    pub fields: JsonMap,
    /// The body of a card or a comment; `null` for a block without one. Read here; the
    /// routes of blocks refuse one that is sent (400 naming `body`): it is written
    /// through `createCard` and `patchCard`.
    pub body: Option<Body>,
    /// Creation time in milliseconds since the Unix epoch.
    #[serde(deserialize_with = "null_as_default")]
    pub create_at: i64,
    /// Last modification time in milliseconds since the Unix epoch.
    #[serde(deserialize_with = "null_as_default")]
    pub update_at: i64,
    /// Deletion time in milliseconds since the Unix epoch; zero if not deleted.
    #[serde(deserialize_with = "null_as_default")]
    pub delete_at: i64,
    /// The identifier of the board the block belongs to.
    #[serde(deserialize_with = "null_as_default")]
    pub board_id: String,
}

impl Block {
    /// Checks that the block may be stored.
    ///
    /// # Errors
    ///
    /// Returns a [`BlockError`] if the board identifier is empty, the title or the
    /// serialized fields exceed their size limits, or the fields hold `properties` that
    /// are not well-formed card property values. The last applies to blocks of every
    /// type: `properties` among a block's fields always means card property values.
    pub fn validate(&self) -> Result<(), BlockError> {
        if self.board_id.is_empty() {
            return Err(BlockError::EmptyBoardId);
        }
        if self.title.chars().count() > BLOCK_TITLE_MAX_CHARS {
            return Err(BlockError::TitleSizeLimitExceeded);
        }
        // Serializing a map with string keys cannot fail; if it ever did, the fields would
        // not be storable either.
        match serde_json::to_string(&self.fields) {
            Ok(fields_json) if fields_json.chars().count() <= BLOCK_FIELDS_MAX_CHARS => {}
            _ => return Err(BlockError::FieldsSizeLimitExceeded),
        }
        validate_block_properties(&self.fields)?;
        Ok(())
    }
}

/// A partial update of a [`Block`]. Absent members leave the block unchanged.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(default, rename_all = "camelCase")]
pub struct BlockPatch {
    /// The new parent identifier.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    /// The new schema version.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schema: Option<i64>,
    /// The new block type.
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Option<String>)]
    pub block_type: Option<BlockType>,
    /// The new title.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Fields to set, replacing any existing value under the same key.
    #[serde(deserialize_with = "null_as_default")]
    #[schema(value_type = Object)]
    pub updated_fields: JsonMap,
    /// Keys of fields to remove.
    #[serde(deserialize_with = "null_as_default")]
    pub deleted_fields: Vec<String>,
    /// Not taken yet: the routes of blocks refuse the member, even `null` (400 naming
    /// `body`). A card's body is changed through `patchCard`.
    #[serde(skip_serializing_if = "BodyChange::is_keep")]
    #[schema(value_type = Option<Body>)]
    pub body: BodyChange,
    /// The `updateAt` of the block as the client read it. If the block has changed
    /// since, the patch is refused.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read_at: Option<i64>,
}

impl BlockPatch {
    /// Applies the patch to `block`.
    ///
    /// Updated fields are written before deleted fields are removed, so a key listed in
    /// both ends up removed.
    pub fn apply(&self, block: &mut Block) {
        if let Some(parent_id) = &self.parent_id {
            block.parent_id.clone_from(parent_id);
        }
        if let Some(schema) = self.schema {
            block.schema = schema;
        }
        if let Some(block_type) = &self.block_type {
            block.block_type.clone_from(block_type);
        }
        if let Some(title) = &self.title {
            block.title.clone_from(title);
        }
        for (key, value) in &self.updated_fields {
            block.fields.insert(key.clone(), value.clone());
        }
        for key in &self.deleted_fields {
            block.fields.remove(key);
        }
        self.body.apply_to(&mut block.body);
    }
}

/// A batch of block identifiers and the patch to apply to each, matched by position.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(default, rename_all = "camelCase")]
pub struct BlockPatchBatch {
    /// The identifiers of the blocks to patch.
    #[serde(deserialize_with = "null_as_default")]
    pub block_ids: Vec<String>,
    /// The patches, one per identifier.
    #[serde(deserialize_with = "null_as_default")]
    pub block_patches: Vec<BlockPatch>,
}

/// Marks every block as modified by `user_id` at `now` (milliseconds since the epoch).
pub fn stamp_modification_metadata(user_id: &str, blocks: &mut [Block], now: i64) {
    for block in blocks {
        user_id.clone_into(&mut block.modified_by);
        block.update_at = now;
    }
}
