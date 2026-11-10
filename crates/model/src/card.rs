//! Cards: a typed view over blocks of type `card`.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use unicode_segmentation::UnicodeSegmentation;

use crate::JsonMap;
use crate::block::{Block, BlockPatch, BlockType};
use crate::id::{IdKind, new_id};
use crate::serde_util::null_as_default;

/// Errors from validating cards and converting them to and from blocks.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CardError {
    /// The card has a missing or malformed member.
    #[error("invalid card, {0}")]
    Invalid(&'static str),
    /// The block to convert is not of type `card`.
    #[error("cannot convert block to card: not a card block")]
    NotCardBlock,
    /// A field of the block has the wrong JSON type for a card.
    #[error("invalid type for field '{field}'")]
    InvalidFieldType {
        /// The name of the offending field.
        field: &'static str,
    },
    /// The card belongs to a different board than the one named in the request.
    #[error("Board IDs do not match")]
    BoardIdMismatch,
}

/// A card: a titled group of properties and content blocks on a board.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Card {
    /// The card's identifier.
    pub id: String,
    /// The identifier of the board the card belongs to.
    pub board_id: String,
    /// The identifier of the user who created the card.
    pub created_by: String,
    /// The identifier of the user who last modified the card.
    pub modified_by: String,
    /// The display title.
    pub title: String,
    /// The identifiers of the card's content blocks, in display order.
    #[serde(deserialize_with = "null_as_default")]
    pub content_order: Vec<String>,
    /// The card's icon; at most one grapheme.
    pub icon: String,
    /// Whether the card is a template.
    pub is_template: bool,
    /// Property values keyed by property identifier.
    #[serde(deserialize_with = "null_as_default")]
    pub properties: JsonMap,
    /// Creation time in milliseconds since the Unix epoch.
    pub create_at: i64,
    /// Last modification time in milliseconds since the Unix epoch.
    pub update_at: i64,
    /// Deletion time in milliseconds since the Unix epoch; zero if not deleted.
    pub delete_at: i64,
}

impl Card {
    /// Fills in the members a new card may leave out: a fresh identifier, and `now`
    /// (milliseconds since the epoch) for a zero creation or modification time.
    pub fn populate(&mut self, now: i64) {
        if self.id.is_empty() {
            self.id = new_id(IdKind::Card);
        }
        if self.create_at == 0 {
            self.create_at = now;
        }
        if self.update_at == 0 {
            self.update_at = now;
        }
    }

    /// Checks that the card is complete.
    ///
    /// # Errors
    ///
    /// Returns [`CardError::Invalid`] naming the first problem: a missing identifier, board
    /// or timestamp, or an icon of more than one grapheme.
    pub fn validate(&self) -> Result<(), CardError> {
        if self.id.is_empty() {
            return Err(CardError::Invalid("ID is missing"));
        }
        if self.board_id.is_empty() {
            return Err(CardError::Invalid("BoardID is missing"));
        }
        check_icon(&self.icon)?;
        if self.create_at == 0 {
            return Err(CardError::Invalid("CreateAt"));
        }
        if self.update_at == 0 {
            return Err(CardError::Invalid("UpdateAt"));
        }
        Ok(())
    }

    /// Builds a card from a block of type `card`.
    ///
    /// Missing fields take their empty value, and a zero timestamp becomes `now`.
    ///
    /// # Errors
    ///
    /// Returns [`CardError::NotCardBlock`] for any other block type, and
    /// [`CardError::InvalidFieldType`] if `contentOrder` is not a list of strings, `icon`
    /// is not a string, `isTemplate` is not a boolean or `properties` is not an object.
    /// A `contentOrder` with nested lists, which the web app writes for side-by-side
    /// content, is rejected this way too.
    pub fn from_block(block: &Block, now: i64) -> Result<Self, CardError> {
        if block.block_type != BlockType::Card {
            return Err(CardError::NotCardBlock);
        }

        let content_order = match block.fields.get("contentOrder") {
            None => Vec::new(),
            Some(Value::Array(items)) => items
                .iter()
                .map(|item| match item {
                    Value::String(id) => Ok(id.clone()),
                    _ => Err(CardError::InvalidFieldType {
                        field: "contentOrder item",
                    }),
                })
                .collect::<Result<Vec<String>, CardError>>()?,
            Some(_) => {
                return Err(CardError::InvalidFieldType {
                    field: "contentOrder",
                });
            }
        };
        let icon = match block.fields.get("icon") {
            None => String::new(),
            Some(Value::String(icon)) => icon.clone(),
            Some(_) => return Err(CardError::InvalidFieldType { field: "icon" }),
        };
        let is_template = match block.fields.get("isTemplate") {
            None => false,
            Some(Value::Bool(is_template)) => *is_template,
            Some(_) => {
                return Err(CardError::InvalidFieldType {
                    field: "isTemplate",
                });
            }
        };
        let properties = match block.fields.get("properties") {
            None => JsonMap::new(),
            Some(Value::Object(properties)) => properties.clone(),
            Some(_) => {
                return Err(CardError::InvalidFieldType {
                    field: "properties",
                });
            }
        };

        let mut card = Self {
            id: block.id.clone(),
            board_id: block.board_id.clone(),
            created_by: block.created_by.clone(),
            modified_by: block.modified_by.clone(),
            title: block.title.clone(),
            content_order,
            icon,
            is_template,
            properties,
            create_at: block.create_at,
            update_at: block.update_at,
            delete_at: block.delete_at,
        };
        card.populate(now);
        Ok(card)
    }
}

impl From<Card> for Block {
    /// Converts the card to a block of type `card` whose parent is the card's board.
    fn from(card: Card) -> Self {
        let mut fields = JsonMap::new();
        fields.insert("contentOrder".to_owned(), Value::from(card.content_order));
        fields.insert("icon".to_owned(), Value::from(card.icon));
        fields.insert("isTemplate".to_owned(), Value::from(card.is_template));
        fields.insert("properties".to_owned(), Value::Object(card.properties));

        Self {
            id: card.id,
            parent_id: card.board_id.clone(),
            created_by: card.created_by,
            modified_by: card.modified_by,
            schema: 1,
            block_type: BlockType::Card,
            title: card.title,
            fields,
            create_at: card.create_at,
            update_at: card.update_at,
            delete_at: card.delete_at,
            board_id: card.board_id,
        }
    }
}

/// A partial update of a [`Card`]. Absent members leave the card unchanged.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct CardPatch {
    /// The new title.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// The new content order.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_order: Option<Vec<String>>,
    /// The new icon; at most one grapheme.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// Property values to set, keyed by property identifier.
    #[serde(deserialize_with = "null_as_default")]
    pub updated_properties: JsonMap,
}

impl CardPatch {
    /// Applies the patch to `card`, adding or replacing the updated properties.
    pub fn apply(&self, card: &mut Card) {
        if let Some(title) = &self.title {
            card.title.clone_from(title);
        }
        if let Some(content_order) = &self.content_order {
            card.content_order.clone_from(content_order);
        }
        if let Some(icon) = &self.icon {
            card.icon.clone_from(icon);
        }
        for (id, value) in &self.updated_properties {
            card.properties.insert(id.clone(), value.clone());
        }
    }

    /// Checks that the patch may be applied.
    ///
    /// # Errors
    ///
    /// Returns [`CardError::Invalid`] if the new icon is more than one grapheme.
    pub fn validate(&self) -> Result<(), CardError> {
        match &self.icon {
            Some(icon) => check_icon(icon),
            None => Ok(()),
        }
    }

    /// Converts the patch to the equivalent patch of the card's block.
    ///
    /// Preserved quirk: the block patch sets the whole `properties` field to the updated
    /// properties, so applying it to a block drops the card's other property values,
    /// whereas [`CardPatch::apply`] merges them.
    ///
    /// # Errors
    ///
    /// Returns the error from [`CardPatch::validate`] if the patch is not valid.
    pub fn to_block_patch(&self) -> Result<BlockPatch, CardError> {
        self.validate()?;

        let mut updated_fields = JsonMap::new();
        if let Some(content_order) = &self.content_order {
            updated_fields.insert(
                "contentOrder".to_owned(),
                Value::from(content_order.clone()),
            );
        }
        if let Some(icon) = &self.icon {
            updated_fields.insert("icon".to_owned(), Value::from(icon.clone()));
        }
        if !self.updated_properties.is_empty() {
            updated_fields.insert(
                "properties".to_owned(),
                Value::Object(self.updated_properties.clone()),
            );
        }

        Ok(BlockPatch {
            title: self.title.clone(),
            updated_fields,
            ..BlockPatch::default()
        })
    }
}

/// Rejects an icon of more than one grapheme cluster.
fn check_icon(icon: &str) -> Result<(), CardError> {
    if icon.graphemes(true).count() > 1 {
        return Err(CardError::Invalid("Icon can have only one grapheme"));
    }
    Ok(())
}
