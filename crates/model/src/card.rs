//! Cards: a typed view over blocks of type `card`.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use unicode_segmentation::UnicodeSegmentation;

use crate::JsonMap;
use crate::block::{Block, BlockPatch, BlockType, PropertyChanges};
use crate::body::{Body, BodyChange, BodySummary};
use crate::card_property::{PersonProperties, check_card_property_values};
use crate::id::new_id;
use crate::refusal::{RefusalReason, Refusals};
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
    /// A new body is sent without the time the card was read at.
    #[error("a new body is sent with readAt, the updateAt of the card as it was read")]
    ReadAtMissing,
}

/// A card: a titled group of properties with a body on a board.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(default, rename_all = "camelCase")]
pub struct Card {
    /// The card's identifier.
    #[serde(deserialize_with = "null_as_default")]
    pub id: String,
    /// The identifier of the board the card belongs to.
    #[serde(deserialize_with = "null_as_default")]
    pub board_id: String,
    /// The identifier of the user who created the card.
    #[serde(deserialize_with = "null_as_default")]
    pub created_by: String,
    /// The identifier of the user who last modified the card.
    #[serde(deserialize_with = "null_as_default")]
    pub modified_by: String,
    /// The display title.
    #[serde(deserialize_with = "null_as_default")]
    pub title: String,
    /// The card's icon; at most one grapheme.
    #[serde(deserialize_with = "null_as_default")]
    pub icon: String,
    /// Whether the card is a template.
    #[serde(deserialize_with = "null_as_default")]
    pub is_template: bool,
    /// Property values keyed by property identifier: a string, a list of strings, or
    /// `null`, and for a property of the type `date` a date value (`{"from":
    /// "2026-03-05"}`, `{"from": "2026-03-05", "to": "2026-03-09"}` or `{"from":
    /// 1772703000000}`) or `null`.
    #[serde(deserialize_with = "null_as_default")]
    #[schema(value_type = Object)]
    pub properties: JsonMap,
    /// The card's body. `null` for a card without one, and in a list of cards, which
    /// says only whether there is one (`hasBody`): a card is read alone for its body.
    pub body: Option<Body>,
    /// Whether the card has a body.
    #[serde(deserialize_with = "null_as_default")]
    pub has_body: bool,
    /// How many task items the body has.
    #[serde(deserialize_with = "null_as_default")]
    pub tasks_total: u32,
    /// How many task items of the body are checked.
    #[serde(deserialize_with = "null_as_default")]
    pub tasks_done: u32,
    /// Creation time in milliseconds since the Unix epoch.
    #[serde(deserialize_with = "null_as_default")]
    pub create_at: i64,
    /// Last modification time in milliseconds since the Unix epoch.
    #[serde(deserialize_with = "null_as_default")]
    pub update_at: i64,
    /// Deletion time in milliseconds since the Unix epoch; zero if not deleted.
    #[serde(deserialize_with = "null_as_default")]
    pub delete_at: i64,
    /// When the card's last comment was posted (the `createAt` of the newest of its
    /// comments, the blocks of the type `comment` directly on it), in milliseconds since
    /// the Unix epoch; zero if it has none. Changing a comment does not change it; a
    /// deleted comment does not count, and counts again with its time if restored. A
    /// comment does not change the card's `updateAt`. A new comment comes as
    /// `UPDATE_BLOCK`, so a client can keep the newer time; after a deletion the card is
    /// read again for it. A number sent here in a request is ignored.
    #[serde(deserialize_with = "null_as_default")]
    pub last_comment_at: i64,
    /// What a gallery shows of the card's body: only in a list asked for it
    /// (`getCards` with `preview=true`), and left out otherwise. A request need not send
    /// it; what it sends is not stored.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview: Option<CardPreview>,
}

/// What a gallery shows of a card's body.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(default, rename_all = "camelCase")]
pub struct CardPreview {
    /// The file the body's first image shows (`getFile` gives it, and `getSharedFile` to
    /// the holder of the board's link); empty if the body has no image.
    pub image_file_id: String,
    /// The beginning of the body's text: its plain text as it is searched (a line per
    /// paragraph, heading, code block and formula block, separated by `\n`; a formula
    /// gives its source, an image and a mention nothing), at most 200 UTF-16 code units,
    /// cut between characters as they are seen (a flag or a letter with its accent stays
    /// whole), without white space at its end. Nothing says it was cut. Empty for a card
    /// without text.
    pub text: String,
}

/// The longest text of a [`CardPreview`], in UTF-16 code units.
pub const CARD_PREVIEW_TEXT_LENGTH: usize = 200;

impl CardPreview {
    /// The preview of a body whose plain text is `text` and whose first image shows
    /// `image_file_id` (empty for none).
    #[must_use]
    pub fn new(image_file_id: String, text: &str) -> Self {
        let mut units = 0;
        let end = text
            .grapheme_indices(true)
            .find(|&(_, grapheme)| {
                units += grapheme.encode_utf16().count();
                units > CARD_PREVIEW_TEXT_LENGTH
            })
            .map_or(text.len(), |(at, _)| at);
        Self {
            image_file_id,
            text: text[..end].trim_end().to_owned(),
        }
    }
}

impl Card {
    /// Empties the users the card names, as a block's are for whoever reads with a read
    /// token alone ([`Block::hide_people`]): its creator and last writer, the users of
    /// its person properties (by `people`, the board's), and the mentions of its body.
    pub fn hide_people(&mut self, people: &PersonProperties) {
        self.created_by.clear();
        self.modified_by.clear();
        people.hide_values(&mut self.properties);
        if let Some(body) = self.body.as_mut() {
            body.clear_mentions();
        }
    }

    /// The card as a list shows it: without its body, with what the list says about
    /// the body instead.
    #[must_use]
    pub fn listed(self, summary: BodySummary) -> Self {
        Self {
            body: None,
            has_body: summary.has_body,
            tasks_total: summary.tasks.total,
            tasks_done: summary.tasks.done,
            ..self
        }
    }

    /// Fills in the members a new card may leave out: a fresh identifier, and `now`
    /// (milliseconds since the epoch) for a zero creation or modification time.
    pub fn populate(&mut self, now: i64) {
        if self.id.is_empty() {
            self.id = new_id();
        }
        if self.create_at == 0 {
            self.create_at = now;
        }
        if self.update_at == 0 {
            self.update_at = now;
        }
    }

    /// Checks the shape of the card as a request to make it sends it, all of it.
    ///
    /// # Errors
    ///
    /// Returns every member that is refused: an icon of more than one grapheme, and
    /// each value of `properties` that has not the shape of one.
    pub fn check_request(&self) -> Result<(), Refusals> {
        let mut refused = Refusals::new();
        refuse_icon_and_values(
            &mut refused,
            Some(&self.icon),
            &self.properties,
            "properties",
        );
        refused.into_result()
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
    /// Missing fields take their empty value, and a zero timestamp becomes `now`. Its
    /// `last_comment_at` is zero: a block does not know its comments, and whoever gives
    /// the card out reads them (the app's `card_of_block`, the card list).
    ///
    /// # Errors
    ///
    /// Returns [`CardError::NotCardBlock`] for any other block type, and
    /// [`CardError::InvalidFieldType`] if `icon` is not a string, `isTemplate` is not a
    /// boolean or `properties` is not an object (`null` as `icon` or `isTemplate` is
    /// taken as absent). A card's content is its body; a
    /// `contentOrder` that a card stored before has is left among the block's fields and
    /// is not read (`docs/s6-design.md` 5.7).
    pub fn from_block(block: &Block, now: i64) -> Result<Self, CardError> {
        if block.block_type != BlockType::Card {
            return Err(CardError::NotCardBlock);
        }

        let icon = match block.fields.get("icon") {
            None | Some(Value::Null) => String::new(),
            Some(Value::String(icon)) => icon.clone(),
            Some(_) => return Err(CardError::InvalidFieldType { field: "icon" }),
        };
        let is_template = match block.fields.get("isTemplate") {
            None | Some(Value::Null) => false,
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

        let summary = block.body.as_ref().map(Body::summary).unwrap_or_default();
        let mut card = Self {
            id: block.id.clone(),
            board_id: block.board_id.clone(),
            created_by: block.created_by.clone(),
            modified_by: block.modified_by.clone(),
            title: block.title.clone(),
            icon,
            is_template,
            properties,
            body: block.body.clone(),
            has_body: summary.has_body,
            tasks_total: summary.tasks.total,
            tasks_done: summary.tasks.done,
            create_at: block.create_at,
            update_at: block.update_at,
            delete_at: block.delete_at,
            // A block does not know its comments.
            last_comment_at: 0,
            preview: None,
        };
        card.populate(now);
        Ok(card)
    }
}

impl From<Card> for Block {
    /// Converts the card to a block of type `card` directly on the card's board: a block
    /// without a parent.
    fn from(card: Card) -> Self {
        let mut fields = JsonMap::new();
        fields.insert("icon".to_owned(), Value::from(card.icon));
        fields.insert("isTemplate".to_owned(), Value::from(card.is_template));
        fields.insert("properties".to_owned(), Value::Object(card.properties));

        Self {
            id: card.id,
            parent_id: String::new(),
            created_by: card.created_by,
            modified_by: card.modified_by,
            schema: 1,
            block_type: BlockType::Card,
            title: card.title,
            fields,
            body: card.body,
            create_at: card.create_at,
            update_at: card.update_at,
            delete_at: card.delete_at,
            board_id: card.board_id,
            edited_at: 0,
        }
    }
}

/// What a card is changed to. A member that is left out stays as it is.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(default, rename_all = "camelCase")]
pub struct CardPatch {
    /// The new title.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// The new icon; at most one grapheme.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// Property values to set, by property identifier, each replacing the card's value
    /// for that property. The card's other values stay as they are: two patches that
    /// set different properties of a card at about the same time both keep theirs, and
    /// of two that set the same property the later one wins.
    #[serde(deserialize_with = "null_as_default")]
    #[schema(value_type = Object)]
    pub updated_properties: JsonMap,
    /// Identifiers of properties whose value the card is to lose. A property named here
    /// and in `updatedProperties` loses its value.
    #[serde(
        deserialize_with = "null_as_default",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub deleted_properties: Vec<String>,
    /// The new body; `null` takes the body away. Sent only with `readAt`.
    #[serde(skip_serializing_if = "BodyChange::is_keep")]
    #[schema(value_type = Option<Body>)]
    pub body: BodyChange,
    /// The `updateAt` of the card as the client read it: if the card has changed since,
    /// the patch is refused with 409 and changes nothing. Needed with `body`, and
    /// checked whenever it is sent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read_at: Option<i64>,
}

impl CardPatch {
    /// Checks the shape of the patch as a request sends it, all of it.
    ///
    /// # Errors
    ///
    /// Returns every member that is refused: an icon of more than one grapheme, a new
    /// body without `readAt`, and each value of `updatedProperties` that has not the
    /// shape of one. A value of a property the patch also deletes is not looked at: it
    /// is gone when the patch is applied.
    pub fn check_request(&self) -> Result<(), Refusals> {
        let mut refused = Refusals::new();
        if !self.body.is_keep() && self.read_at.is_none() {
            refused.refuse(
                "readAt",
                RefusalReason::Required,
                CardError::ReadAtMissing.to_string(),
            );
        }
        let kept = self
            .updated_properties
            .iter()
            .filter(|(id, _)| !self.deleted_properties.contains(id));
        refuse_icon_and_values(
            &mut refused,
            self.icon.as_deref(),
            kept,
            "updatedProperties",
        );
        refused.into_result()
    }

    /// Applies the patch to `card`: the updated properties are set, then the deleted ones
    /// removed.
    pub fn apply(&self, card: &mut Card) {
        if let Some(title) = &self.title {
            card.title.clone_from(title);
        }
        if let Some(icon) = &self.icon {
            card.icon.clone_from(icon);
        }
        self.property_changes().apply_to(&mut card.properties);
        self.body.apply_to(&mut card.body);
    }

    /// The changes of the patch to the card's property values.
    fn property_changes(&self) -> PropertyChanges {
        PropertyChanges {
            updated: self.updated_properties.clone(),
            deleted: self.deleted_properties.clone(),
        }
    }

    /// Checks that the patch may be applied.
    ///
    /// # Errors
    ///
    /// Returns [`CardError::Invalid`] if the new icon is more than one grapheme, and
    /// [`CardError::ReadAtMissing`] for a new body without the time it was read at.
    pub fn validate(&self) -> Result<(), CardError> {
        if !self.body.is_keep() && self.read_at.is_none() {
            return Err(CardError::ReadAtMissing);
        }
        match &self.icon {
            Some(icon) => check_icon(icon),
            None => Ok(()),
        }
    }

    /// Converts the patch to the equivalent patch of the card's block. The property
    /// values go in [`BlockPatch::property_changes`], to be merged into the values the
    /// block has when it is patched.
    ///
    /// # Errors
    ///
    /// Returns the error from [`CardPatch::validate`] if the patch is not valid.
    pub fn to_block_patch(&self) -> Result<BlockPatch, CardError> {
        self.validate()?;

        let mut updated_fields = JsonMap::new();
        if let Some(icon) = &self.icon {
            updated_fields.insert("icon".to_owned(), Value::from(icon.clone()));
        }

        Ok(BlockPatch {
            title: self.title.clone(),
            updated_fields,
            property_changes: self.property_changes(),
            body: self.body.clone(),
            read_at: self.read_at,
            ..BlockPatch::default()
        })
    }
}

/// What an icon of more than one grapheme cluster is refused with.
pub(crate) const ICON_OF_SEVERAL: &str = "Icon can have only one grapheme";

/// Whether the text may be an icon, of a card or of a page: at most one grapheme
/// cluster.
pub(crate) fn is_icon(icon: &str) -> bool {
    icon.graphemes(true).count() <= 1
}

/// Rejects an icon of more than one grapheme cluster.
fn check_icon(icon: &str) -> Result<(), CardError> {
    if !is_icon(icon) {
        return Err(CardError::Invalid(ICON_OF_SEVERAL));
    }
    Ok(())
}

/// Gathers what [`check_icon`] and the check of property values refuse of a request
/// whose values are its member `values_member`.
fn refuse_icon_and_values<'v>(
    refused: &mut Refusals,
    icon: Option<&str>,
    values: impl IntoIterator<Item = (&'v String, &'v Value)>,
    values_member: &str,
) {
    if icon.is_some_and(|icon| check_icon(icon).is_err()) {
        refused.refuse(
            "icon",
            RefusalReason::Invalid,
            CardError::Invalid(ICON_OF_SEVERAL).to_string(),
        );
    }
    if let Err(of_values) = check_card_property_values(values, values_member) {
        for refusal in of_values {
            refused.push(refusal);
        }
    }
}
