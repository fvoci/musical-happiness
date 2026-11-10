//! Blocks: the basic unit of board content (cards, views, comments and attachments).

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

/// Maximum number of characters in a view's `fields` once serialized to JSON, and in
/// those of a block of a type that new blocks no longer have.
pub const BLOCK_FIELDS_MAX_CHARS: usize = 800_000;

/// Maximum number of characters in the `fields` of a card, a comment or an attachment
/// once serialized to JSON (`docs/s6-design.md` 5.7). A card's content is its body.
pub const CARD_FIELDS_MAX_CHARS: usize = 64_000;

/// The type of a block.
///
/// A new block has one of four types ([`BlockType::is_current`]). Blocks stored before a
/// card's content became its body have others, among them type strings the server knows
/// nothing about (the original's web app had `h1` and `list-item`); those are read and
/// returned verbatim, kept in [`BlockType::Other`].
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
    /// Returns `true` for the types that may have a body: a card and a comment
    /// (`docs/s6-design.md` 5.7).
    #[must_use]
    pub fn has_body(&self) -> bool {
        matches!(self, Self::Card | Self::Comment)
    }

    /// Returns `true` for the types a new block may have: a card, a view, a comment and
    /// an attachment (`docs/s6-design.md` 5.7). A block of another type was stored
    /// before a card's content became its body; it stays as it is, but no block is
    /// made one.
    #[must_use]
    pub fn is_current(&self) -> bool {
        matches!(
            self,
            Self::Card | Self::View | Self::Comment | Self::Attachment
        )
    }

    /// Returns the maximum number of characters in the serialized `fields` of a block
    /// of this type.
    #[must_use]
    pub fn fields_max_chars(&self) -> usize {
        match self {
            Self::Card | Self::Comment | Self::Attachment => CARD_FIELDS_MAX_CHARS,
            // A block stored before a card's content became its body keeps the limit it
            // was stored under.
            Self::View
            | Self::Board
            | Self::Text
            | Self::Checkbox
            | Self::Image
            | Self::Divider
            | Self::Other(_) => BLOCK_FIELDS_MAX_CHARS,
        }
    }

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
    /// The serialized fields are longer than the limit of the block's type
    /// ([`BlockType::fields_max_chars`]).
    #[error("block fields size limit exceeded")]
    FieldsSizeLimitExceeded,
    /// A new block, or a patch, gives a type that no new block has
    /// ([`BlockType::is_current`]). The type is as the client wrote it, at most
    /// [`TYPE_SHOWN_CHARS`] characters of it; see [`BlockError::type_not_current`].
    #[error("a block of the type {0:?} is not made any more: a card's content is its body")]
    TypeNotCurrent(String),
    /// The `properties` among the fields are not well-formed card property values.
    #[error(transparent)]
    InvalidCardProperties(#[from] CardPropertyError),
    /// A block that is neither a card nor a comment has a body.
    #[error("only a card or a comment has a body")]
    BodyNotAllowed,
}

/// How much of a type that is refused a [`BlockError::TypeNotCurrent`] shows: the
/// client's own text, which may be long, is not echoed whole.
pub const TYPE_SHOWN_CHARS: usize = 64;

impl BlockError {
    /// The refusal of `block_type` as the type of a new block or of a patch, with at most
    /// [`TYPE_SHOWN_CHARS`] characters of it.
    #[must_use]
    pub fn type_not_current(block_type: &BlockType) -> Self {
        let name = block_type.as_str();
        Self::TypeNotCurrent(match name.char_indices().nth(TYPE_SHOWN_CHARS) {
            Some((end, _)) => format!("{}…", &name[..end]),
            None => name.to_owned(),
        })
    }
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
    /// The block type: `card`, `view`, `comment` or `attachment`. A block stored before a
    /// card's content became its body may have another (`text`, `image`, `checkbox`,
    /// `divider` and others the web app had); it keeps it. No request makes a block of
    /// such a type or patches one into it (400 naming `type`), except that duplicating a
    /// stored one, or the card or board it is on, copies it.
    #[serde(rename = "type")]
    #[serde(deserialize_with = "null_as_default")]
    #[schema(value_type = String)]
    pub block_type: BlockType,
    /// The display title.
    #[serde(deserialize_with = "null_as_default")]
    pub title: String,
    /// Type-specific data, free-form: serialized, at most 64,000 characters for a card, a
    /// comment or an attachment and 800,000 for a view or a block of another type.
    #[serde(deserialize_with = "null_as_default")]
    #[schema(value_type = Object)]
    pub fields: JsonMap,
    /// The body of a card or a comment; `null` for a block without one. Only those two
    /// have one: a body on another block is refused (400 naming `body`). Every image of
    /// it shows a file of the block's board.
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
    /// serialized fields exceed their size limits (the latter by type), or the fields
    /// hold `properties` that
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
            Ok(fields_json)
                if fields_json.chars().count() <= self.block_type.fields_max_chars() => {}
            _ => return Err(BlockError::FieldsSizeLimitExceeded),
        }
        validate_block_properties(&self.fields)?;
        if self.body.is_some() && !self.block_type.has_body() {
            return Err(BlockError::BodyNotAllowed);
        }
        Ok(())
    }

    /// The stored names of the files the block shows, each once: the file of an image
    /// or attachment block (`fileId`, or failing that `attachmentId`), and the images
    /// of the body. The store's migration of file references
    /// (`0003_file_refs.sql`) copies this rule in SQL; the two change together.
    #[must_use]
    pub fn shown_files(&self) -> Vec<String> {
        if self.block_type == BlockType::Image || self.block_type == BlockType::Attachment {
            return ["fileId", "attachmentId"]
                .iter()
                .find_map(|key| self.fields.get(*key).and_then(serde_json::Value::as_str))
                .filter(|name| !name.is_empty())
                .map(str::to_owned)
                .into_iter()
                .collect();
        }
        self.body
            .as_ref()
            .map(|body| body.file_ids().into_iter().map(str::to_owned).collect())
            .unwrap_or_default()
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
    /// The new block type: `card`, `view`, `comment` or `attachment`, or the type the
    /// block has. Any other is refused (400 naming `type`).
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
    /// The new body of a card or a comment; `null` takes the body away. Sent only with
    /// `readAt` (400 naming `readAt` without it).
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

    /// Checks the type the patch gives a block of the type `stored`: a block may keep
    /// its type, whatever it is, or take one that a new block may have.
    ///
    /// # Errors
    ///
    /// Returns [`BlockError::TypeNotCurrent`] for a change to another type.
    pub fn check_type(&self, stored: &BlockType) -> Result<(), BlockError> {
        match &self.block_type {
            Some(new_type) if new_type != stored && !new_type.is_current() => {
                Err(BlockError::type_not_current(new_type))
            }
            _ => Ok(()),
        }
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
