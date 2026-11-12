//! Blocks: the basic unit of board content (cards, views, comments and attachments).

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::JsonMap;
use crate::body::{Body, BodyChange};
use crate::card_property::{CardPropertyError, PersonProperties, validate_block_properties};
use crate::relation::RelationError;
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
    /// A comment on a card.
    Comment,
    /// A file attachment on a card.
    Attachment,
    /// Any other type string, kept as sent, such as a content block type of the original
    /// (`text`, `image`, ...): no block is stored with one. The empty string means "no
    /// type".
    Other(String),
}

impl BlockType {
    /// Returns `true` for the types that may have a body: a card and a comment
    /// (`docs/s6-design.md` 5.7).
    #[must_use]
    pub fn has_body(&self) -> bool {
        matches!(self, Self::Card | Self::Comment)
    }

    /// Returns `true` for the types a block may be given: a card, a view, a comment and
    /// an attachment (`docs/s6-design.md` 5.7). The content blocks that were stored
    /// before a card's content became its body were deleted by a data step of migration
    /// 8.
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
            Self::View | Self::Board | Self::Other(_) => BLOCK_FIELDS_MAX_CHARS,
        }
    }

    /// Returns the wire string of this type.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Board => "board",
            Self::Card => "card",
            Self::View => "view",
            Self::Comment => "comment",
            Self::Attachment => "attachment",
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
            "comment" => Self::Comment,
            "attachment" => Self::Attachment,
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

    /// Parses one of the five known block types, ignoring ASCII case.
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
    /// A comment that is new, newly a comment or moved is not below a card of its
    /// board: a comment is on a card.
    #[error("the parent of a comment is a card of the board")]
    CommentNotOnCard,
    /// A block that is new or moved is below a comment: a comment has no blocks below
    /// it.
    #[error("a comment is the parent of no block")]
    ParentIsComment,
    /// A change of type that would leave a comment off a card: a block with blocks
    /// below it becomes a comment, a block off a card becomes a comment where it
    /// stands, or a card with comments becomes another type.
    #[error("the type would leave a comment off a card")]
    TypeStrandsComments,
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
    /// A card relation value names what it may not name
    /// ([`crate::check_relation_value`]), or a block that is no card of the board.
    #[error(transparent)]
    InvalidRelation(#[from] RelationError),
    /// A block that is neither a card nor a comment has a body.
    #[error("only a card or a comment has a body")]
    BodyNotAllowed,
    /// The title holds U+0000, which PostgreSQL cannot store in text.
    #[error("the title holds the character U+0000")]
    NulInTitle,
    /// A field of a card has a value of the wrong kind: `icon` a string, `isTemplate` a
    /// boolean (or either `null`).
    #[error("the field {field} of a card must be {expected}")]
    InvalidCardField {
        /// The field.
        field: &'static str,
        /// What its value must be.
        expected: &'static str,
    },
}

/// One rule of [`CARD_FIELD_KINDS`]: the field, its kind as a message names it, and
/// whether a value is of that kind.
type CardFieldKind = (&'static str, &'static str, fn(&Value) -> bool);

/// The fields of a card whose value has one kind. Anything else breaks the reading of
/// the card ([`crate::Card::from_block`]). `null` is taken as absent and is of every
/// kind.
const CARD_FIELD_KINDS: [CardFieldKind; 2] = [
    ("icon", "a string", Value::is_string),
    ("isTemplate", "a boolean", Value::is_boolean),
];

/// Whether `value` may be the value of the card field `field` ([`CARD_FIELD_KINDS`]).
fn is_card_field_of_its_kind(field: &str, value: &Value) -> bool {
    value.is_null()
        || CARD_FIELD_KINDS
            .iter()
            .find(|(name, _, _)| *name == field)
            .is_none_or(|(_, _, is_of_kind)| is_of_kind(value))
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
    /// The block type: `card`, `view`, `comment` or `attachment`. No request makes a
    /// block of another type or patches one into it (400 naming `type`).
    #[serde(rename = "type")]
    #[serde(deserialize_with = "null_as_default")]
    #[schema(value_type = String)]
    pub block_type: BlockType,
    /// The display title. A title with U+0000 is refused (400 naming `title`).
    #[serde(deserialize_with = "null_as_default")]
    pub title: String,
    /// Type-specific data, free-form: serialized, at most 64,000 characters for a card, a
    /// comment or an attachment and 800,000 for a view or a block of another type. The
    /// `icon` of a card is a string and its `isTemplate` a boolean, or either `null`
    /// (400 naming `fields` otherwise).
    #[serde(deserialize_with = "null_as_default")]
    #[schema(value_type = Object)]
    pub fields: JsonMap,
    /// The body of a card or a comment; `null` for a block without one. Only those two
    /// have one: a body on another block is refused (400 naming `body`). Every image of
    /// it shows a file of the block's board. Whoever reads with a read token alone gets
    /// every mention with an empty `userId`: a shape that is only sent, which a write
    /// would have refused.
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
    /// For a comment whose body was changed: when that last happened, in milliseconds
    /// since the Unix epoch. Absent for a comment never changed and for every other
    /// block. The server sets it, and ignores a client's. What the body said before is
    /// `getCommentVersions`'.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub edited_at: i64,
}

/// Whether a time is none: zero.
#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde's skip_serializing_if passes a reference"
)]
fn is_zero(time: &i64) -> bool {
    *time == 0
}

impl Block {
    /// Checks that the block may be stored.
    ///
    /// # Errors
    ///
    /// Returns a [`BlockError`] if the board identifier is empty, the title or the
    /// serialized fields exceed their size limits (the latter by type), the title holds
    /// U+0000, the fields hold `properties` that are not well-formed card property
    /// values, or the block is a card whose `icon` or `isTemplate` is of another kind
    /// (a string and a boolean, or `null`). `properties` among a block's fields always means card
    /// property values, whatever the block's type.
    pub fn validate(&self) -> Result<(), BlockError> {
        if self.board_id.is_empty() {
            return Err(BlockError::EmptyBoardId);
        }
        if self.title.chars().count() > BLOCK_TITLE_MAX_CHARS {
            return Err(BlockError::TitleSizeLimitExceeded);
        }
        if self.title.contains('\0') {
            return Err(BlockError::NulInTitle);
        }
        if self.block_type == BlockType::Card {
            for (field, expected, is_of_kind) in CARD_FIELD_KINDS {
                if self
                    .fields
                    .get(field)
                    .is_some_and(|value| !value.is_null() && !is_of_kind(value))
                {
                    return Err(BlockError::InvalidCardField { field, expected });
                }
            }
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

    /// Removes the fields that a block can no longer be stored with but that one stored
    /// before may hold: `properties` that are no object, and for a card an `icon` or
    /// `isTemplate` of another kind. Returns their names. A stored block is made so
    /// before it is read as a card, patched or copied: what is refused is what a
    /// request writes, not what was there.
    pub fn drop_malformed_fields(&mut self) -> Vec<String> {
        let is_card = self.block_type == BlockType::Card;
        let malformed: Vec<String> = self
            .fields
            .iter()
            .filter(|(field, value)| match field.as_str() {
                "properties" => !value.is_object(),
                field => is_card && !is_card_field_of_its_kind(field, value),
            })
            .map(|(field, _)| field.clone())
            .collect();
        for field in &malformed {
            self.fields.remove(field);
        }
        malformed
    }

    /// The block as it goes to whoever reads by a read token alone without being a
    /// member of the board, who is nobody the server knows (`docs/s6-design.md`,
    /// decision 7): no `createdBy` or `modifiedBy`, no user in the mentions of the body,
    /// and in `fields` none that the board's person properties name (`people`): in a
    /// card's `properties` ([`PersonProperties::hide_values`]) and in a view's filter and
    /// groups ([`PersonProperties::hide_view_fields`]). A user's identifier tells when
    /// they signed up.
    pub fn hide_people(&mut self, people: &PersonProperties) {
        self.created_by.clear();
        self.modified_by.clear();
        if let Some(body) = self.body.as_mut() {
            body.clear_mentions();
        }
        if let Some(Value::Object(values)) = self.fields.get_mut("properties") {
            people.hide_values(values);
        }
        if self.block_type == BlockType::View {
            people.hide_view_fields(&mut self.fields);
        }
    }

    /// The stored names of the files the block shows, each once: the file of an
    /// attachment block (`fileId`, or failing that `attachmentId`), and the images of the
    /// body. The store's migration of file references (`0003_file_refs.sql`) put down
    /// the same rule in SQL, with the image blocks of then.
    #[must_use]
    pub fn shown_files(&self) -> Vec<String> {
        if self.block_type == BlockType::Attachment {
            return self
                .attachment_file()
                .map(str::to_owned)
                .into_iter()
                .collect();
        }
        self.body
            .as_ref()
            .map(|body| body.file_ids().into_iter().map(str::to_owned).collect())
            .unwrap_or_default()
    }

    /// The stored name of the file of an attachment block: its `fileId`, or failing that
    /// its `attachmentId`; `None` for an empty name and for a block of another type.
    #[must_use]
    pub fn attachment_file(&self) -> Option<&str> {
        if self.block_type != BlockType::Attachment {
            return None;
        }
        ["fileId", "attachmentId"]
            .iter()
            .find_map(|key| self.fields.get(*key).and_then(serde_json::Value::as_str))
            .filter(|name| !name.is_empty())
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
    /// The new block type: `card`, `view`, `comment` or `attachment`. Any other is
    /// refused (400 naming `type`).
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
    /// Card property values to set and remove, merged into the values the block has
    /// when it is patched: what `patchCard` sends. Not a member of a block patch's JSON.
    #[serde(skip)]
    pub property_changes: PropertyChanges,
}

/// Changes to the property values of a card, by property identifier: the values to set,
/// then the ones to remove. The values that are named in neither stay.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PropertyChanges {
    /// Values to set, each replacing the value of its property.
    pub updated: JsonMap,
    /// Properties whose value is removed.
    pub deleted: Vec<String>,
}

impl PropertyChanges {
    /// Sets the updated values in `values`, then removes the deleted ones.
    pub fn apply_to(&self, values: &mut JsonMap) {
        for (id, value) in &self.updated {
            values.insert(id.clone(), value.clone());
        }
        for id in &self.deleted {
            values.remove(id);
        }
    }

    /// Applies the changes to the `properties` field of a block's fields, made an empty
    /// object first if it is absent or no object and there is a value to set.
    fn apply_to_fields(&self, fields: &mut JsonMap) {
        if self.updated.is_empty() && !fields.get("properties").is_some_and(Value::is_object) {
            return;
        }
        let properties = fields
            .entry("properties")
            .or_insert_with(|| Value::Object(JsonMap::new()));
        if !properties.is_object() {
            *properties = Value::Object(JsonMap::new());
        }
        if let Value::Object(values) = properties {
            self.apply_to(values);
        }
    }
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
        self.property_changes.apply_to_fields(&mut block.fields);
        self.body.apply_to(&mut block.body);
    }

    /// Checks the type the patch gives a block: none, or one a block may be given.
    ///
    /// # Errors
    ///
    /// Returns [`BlockError::TypeNotCurrent`] for another type.
    pub fn check_type(&self) -> Result<(), BlockError> {
        match &self.block_type {
            Some(new_type) if !new_type.is_current() => Err(BlockError::type_not_current(new_type)),
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
