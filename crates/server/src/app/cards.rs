//! Cards: the card view of blocks of type `card`.
//!
//! A card has no storage of its own. It is created, read and patched as its block, so
//! every change is announced the way block changes are.

use fvoci_model::{Block, BlockType, Card, CardError, CardPatch, CardPreview, new_id};
use fvoci_store::{BlockFilter, Paging, Previews};

use super::{App, ChangeNotification};
use crate::auth::now_millis;
use crate::error::AppError;

/// Whether a list of cards gives each card its preview.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardPreviews {
    /// It does not.
    Without,
    /// Each card has the file of its body's first image and the beginning of its
    /// plain text.
    With,
}

impl From<CardPreviews> for Previews {
    fn from(previews: CardPreviews) -> Self {
        match previews {
            CardPreviews::Without => Self::Without,
            CardPreviews::With => Self::With,
        }
    }
}

/// Which cards of a board a `page` and `per_page` pair asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CardRange {
    /// Every card.
    All,
    /// One page of the cards.
    Page(Paging),
    /// A page before the first one or beyond any card there can be.
    Nothing,
}

impl CardRange {
    /// Reads the paging parameters the way the original's query does
    /// (server/services/store/sqlstore/blocks.go:73-79): a page size of zero or less
    /// does not limit, and a page before the first one finds nothing.
    ///
    /// Where the original's query fails, this does not: a page other than the first
    /// with a page size of zero or less is every card (the original's SQLite refuses an
    /// offset without a limit), and an offset beyond the range of the database's
    /// integers finds nothing (the original's arithmetic wraps around).
    pub(super) fn new(page: i64, per_page: i64) -> Self {
        if per_page <= 0 {
            return Self::All;
        }
        let (Ok(page_number), Some(_offset)) = (u32::try_from(page), page.checked_mul(per_page))
        else {
            return Self::Nothing;
        };
        Self::Page(Paging {
            page: page_number,
            // More cards per page than the store can count are every card of the page.
            per_page: u32::try_from(per_page).unwrap_or(u32::MAX),
        })
    }
}

impl App {
    /// Creates a card on a board and returns it as stored; listeners are told as for
    /// any inserted block.
    ///
    /// The card gets a new identifier, `board_id` as its board, `user_id` as its
    /// creator and modifier and the current time as creation and modification time,
    /// whatever `card` says about them.
    ///
    /// # Errors
    ///
    /// Returns a not-found error if the board does not exist and
    /// a bad request if the title or the properties are too large, both
    /// with the message prefixed by `cannot create card`.
    pub async fn create_card(
        &self,
        card: Card,
        board_id: &str,
        user_id: &str,
        notification: ChangeNotification,
    ) -> Result<Card, AppError> {
        let now = now_millis();
        let card = Card {
            id: new_id(),
            board_id: board_id.to_owned(),
            created_by: user_id.to_owned(),
            modified_by: user_id.to_owned(),
            create_at: now,
            update_at: now,
            delete_at: 0,
            ..card
        };

        let mut blocks = [Block::from(card)];
        self.insert_blocks(&mut blocks, user_id, notification)
            .await
            .map_err(|error| error.context("cannot create card"))?;
        let [block] = blocks;
        Ok(Card::from_block(&block, now)?)
    }

    /// Returns cards of a board, in no particular order: page number `page`, counted
    /// from zero, of pages of `per_page` cards. A `per_page` of zero or less asks for
    /// all cards, whatever the page. With [`CardPreviews::With`] each card has its preview.
    ///
    /// # Errors
    ///
    /// Returns an internal error if one of the board's card blocks cannot be read
    /// as a card, because a field of it has the wrong type.
    pub async fn cards_for_board(
        &self,
        board_id: &str,
        page: i64,
        per_page: i64,
        previews: CardPreviews,
    ) -> Result<Vec<Card>, AppError> {
        let mut filter = BlockFilter::container(board_id).with_type(BlockType::Card);
        match CardRange::new(page, per_page) {
            CardRange::All => {}
            CardRange::Page(paging) => filter.paging = Some(paging),
            CardRange::Nothing => return Ok(Vec::new()),
        }
        let blocks = self
            .store()
            .get_listed_blocks(&filter, previews.into())
            .await?;

        let now = now_millis();
        blocks
            .iter()
            .map(|listed| {
                // The fields of a card that are not of their kind are left out
                // (`card_from_stored_block`), so no stored card fails the list; an
                // error here would be the server's.
                // A list says whether a card has a body; the body is read with the card.
                card_from_stored_block(&listed.block, now)
                    .map(|card| Card {
                        last_comment_at: listed.last_comment_at,
                        preview: listed.preview.as_ref().map(|preview| {
                            CardPreview::new(
                                preview.first_image.clone().unwrap_or_default(),
                                &preview.text,
                            )
                        }),
                        ..card.listed(listed.summary)
                    })
                    .map_err(|error| AppError::from(error).context("Block2Card fail"))
            })
            .collect()
    }

    /// Applies a patch to a card and returns the patched card; listeners are told as
    /// for any patched block.
    ///
    /// The property values are merged into the stored ones in the store's transaction
    /// ([`fvoci_model::BlockPatch::property_changes`]). (The original replaces them all,
    /// server/model/card.go:306-308.)
    ///
    /// # Errors
    ///
    /// Returns a bad request if the patch is not valid (an icon of more than one
    /// grapheme), an internal error if the patched block is no card, and otherwise the
    /// errors of [`App::patch_block`] with the message prefixed by `cannot patch card
    /// {id}`.
    pub async fn patch_card(
        &self,
        patch: &CardPatch,
        card_id: &str,
        user_id: &str,
        notification: ChangeNotification,
    ) -> Result<Card, AppError> {
        // An invalid icon is the client's mistake, as for a new card. (In the original
        // it is a server error here, `server/app/cards.go:66-69`.)
        let block_patch = patch.to_block_patch()?;
        let block = self
            .patch_block(card_id, &block_patch, user_id, notification)
            .await
            .map_err(|error| error.context(format_args!("cannot patch card {card_id}")))?;
        self.with_last_comment_at(card_from_stored_block(&block, now_millis())?)
            .await
    }

    /// Returns the card with the given identifier.
    ///
    /// # Errors
    ///
    /// Returns a not-found error if there is no block with this identifier and
    /// an internal error if the block is not a card or cannot be read as one.
    pub async fn card(&self, card_id: &str) -> Result<Card, AppError> {
        let block = self.block(card_id).await?;
        self.card_of_block(&block).await
    }

    /// Returns the card of a card block that was read before, with when its last
    /// comment was posted.
    ///
    /// # Errors
    ///
    /// Returns an internal error if the block cannot be read as a card, and the store's
    /// error if its comments cannot be read.
    pub async fn card_of_block(&self, block: &Block) -> Result<Card, AppError> {
        self.with_last_comment_at(card_from_stored_block(block, now_millis())?)
            .await
    }

    /// The card with when its last comment was posted.
    async fn with_last_comment_at(&self, card: Card) -> Result<Card, AppError> {
        let last_comment_at = self.store().get_last_comment_at(&card.id).await?;
        Ok(Card {
            last_comment_at,
            ..card
        })
    }
}

/// Builds the card view of a stored block.
///
/// A card cannot be stored with `properties` that are no object, or an `icon` or
/// `isTemplate` of another kind, any more, but one stored before, or written straight to
/// the database, may still hold such a field. The original fails to read such a card,
/// and with it the whole card list of its board. Here the card is given out without the
/// field ([`Block::drop_malformed_fields`]), and a warning is logged.
fn card_from_stored_block(block: &Block, now: i64) -> Result<Card, CardError> {
    let mut readable = block.clone();
    let malformed = readable.drop_malformed_fields();
    if !malformed.is_empty() {
        tracing::warn!(
            block_id = %block.id,
            board_id = %block.container_id,
            fields = ?malformed,
            "fields of a card are not of their kind; giving the card out without them"
        );
    }
    Card::from_block(&readable, now)
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::*;

    fn page(page: u32, per_page: u32) -> CardRange {
        CardRange::Page(Paging { page, per_page })
    }

    /// The API and the store refuse to write such a block, so this is the one place
    /// that can still show what reading one does.
    #[test]
    fn a_stored_card_whose_properties_are_no_object_is_read_without_properties() {
        use serde_json::json;

        let card_block = |properties: Value| {
            let mut block = Block {
                id: "c-card".to_owned(),
                container_id: "b-board".to_owned(),
                block_type: fvoci_model::BlockType::Card,
                title: "a card".to_owned(),
                create_at: 1,
                update_at: 1,
                ..Block::default()
            };
            block.fields.insert("properties".to_owned(), properties);
            block.fields.insert("icon".to_owned(), json!("x"));
            block
        };

        let fine = card_from_stored_block(&card_block(json!({"p1": "v1"})), 5)
            .expect("a well-formed card is read");
        assert_eq!(
            fine.properties.get("p1"),
            Some(&json!("v1")),
            "a well-formed card keeps its properties"
        );

        for properties in [json!("a string"), json!([1, 2]), json!(42), json!(null)] {
            let card = card_from_stored_block(&card_block(properties.clone()), 5)
                .expect("the malformed card is read all the same");
            assert!(
                card.properties.is_empty(),
                "without properties: {properties}"
            );
            assert_eq!(card.title, "a card", "the rest of the card: {properties}");
            assert_eq!(card.icon, "x", "the rest of the card: {properties}");
        }
    }

    /// A card stored before `icon` and `isTemplate` were checked is read without the
    /// field that is not of its kind.
    #[test]
    fn a_stored_card_whose_icon_or_template_flag_is_malformed_is_read_without_it() {
        use serde_json::json;

        for (field, value) in [
            ("icon", json!(5)),
            ("icon", json!({"x": 1})),
            ("isTemplate", json!("yes")),
        ] {
            let mut block = Block {
                id: "c-card".to_owned(),
                container_id: "b-board".to_owned(),
                block_type: fvoci_model::BlockType::Card,
                title: "a card".to_owned(),
                create_at: 1,
                update_at: 1,
                ..Block::default()
            };
            block.fields.insert(field.to_owned(), value.clone());
            block
                .fields
                .insert("properties".to_owned(), json!({"p1": "v1"}));
            let card = card_from_stored_block(&block, 5)
                .unwrap_or_else(|error| panic!("{field} {value}: {error}"));
            assert_eq!(card.icon, "", "{field} {value}");
            assert!(!card.is_template, "{field} {value}");
            assert_eq!(
                card.properties.len(),
                1,
                "the rest is kept: {field} {value}"
            );
        }
    }

    #[test]
    fn card_range_reads_paging_like_the_original() {
        let cases = [
            ((0, 100), page(0, 100)),
            ((2, 10), page(2, 10)),
            ((0, 0), CardRange::All),
            ((0, -1), CardRange::All),
            ((3, 0), CardRange::All),
            ((-1, -1), CardRange::All),
            ((-1, 10), CardRange::Nothing),
            ((i64::from(u32::MAX) + 1, 1), CardRange::Nothing),
            ((i64::from(u32::MAX), i64::MAX), CardRange::Nothing),
            ((0, i64::MAX), page(0, u32::MAX)),
            ((1, i64::MAX), page(1, u32::MAX)),
        ];
        for ((page, per_page), expected) in cases {
            assert_eq!(
                CardRange::new(page, per_page),
                expected,
                "page {page}, per_page {per_page}"
            );
        }
    }
}
