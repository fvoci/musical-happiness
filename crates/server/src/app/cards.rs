//! Cards: the card view of blocks of type `card`.
//!
//! A card has no storage of its own. It is created, read and patched as its block, so
//! every change is announced the way block changes are.

use fvoci_model::{Block, BlockType, Card, CardError, CardPatch, IdKind, new_id};
use fvoci_store::{BlockFilter, Paging};
use serde_json::Value;

use super::{App, ChangeNotification};
use crate::auth::now_millis;
use crate::error::AppError;

/// Which cards of a board a `page` and `per_page` pair asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CardRange {
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
    fn new(page: i64, per_page: i64) -> Self {
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
    /// Returns [`AppError::NotFound`] if the board does not exist and
    /// [`AppError::BadRequest`] if the title or the properties are too large, both
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
            id: new_id(IdKind::Card),
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
    /// all cards, whatever the page.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::Internal`] if one of the board's card blocks cannot be read
    /// as a card, because a field of it has the wrong type.
    pub async fn cards_for_board(
        &self,
        board_id: &str,
        page: i64,
        per_page: i64,
    ) -> Result<Vec<Card>, AppError> {
        let mut filter = BlockFilter::board(board_id).with_type(BlockType::Card);
        match CardRange::new(page, per_page) {
            CardRange::All => {}
            CardRange::Page(paging) => filter.paging = Some(paging),
            CardRange::Nothing => return Ok(Vec::new()),
        }
        let blocks = self.store().get_blocks(&filter).await?;

        let now = now_millis();
        blocks
            .iter()
            .map(|block| {
                // Preserved quirk: one card block that is no well-formed card fails the
                // whole list (server/app/cards.go:54-60). The web app writes such
                // blocks: a `contentOrder` with side-by-side content has nested lists.
                card_from_stored_block(block, now)
                    .map_err(|error| AppError::from(error).context("Block2Card fail"))
            })
            .collect()
    }

    /// Applies a patch to a card and returns the patched card; listeners are told as
    /// for any patched block.
    ///
    /// Preserved quirk: updated properties replace all properties of the card, they
    /// are not merged into them (server/model/card.go:306-308).
    ///
    /// # Errors
    ///
    /// Returns [`AppError::Internal`] if the patch is not valid (an icon of more than
    /// one grapheme) or the patched block is no card, and otherwise the errors of
    /// [`App::patch_block`] with the message prefixed by `cannot patch card {id}`.
    pub async fn patch_card(
        &self,
        patch: &CardPatch,
        card_id: &str,
        user_id: &str,
        notification: ChangeNotification,
    ) -> Result<Card, AppError> {
        // Preserved quirk: unlike for a new card, the invalid icon is not classified
        // as the client's mistake here, so it is a server error
        // (server/app/cards.go:66-69).
        let block_patch = patch.to_block_patch()?;
        let block = self
            .patch_block(card_id, &block_patch, user_id, notification)
            .await
            .map_err(|error| error.context(&format!("cannot patch card {card_id}")))?;
        Ok(card_from_stored_block(&block, now_millis())?)
    }

    /// Returns the card with the given identifier.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::NotFound`] if there is no block with this identifier and
    /// [`AppError::Internal`] if the block is not a card or cannot be read as one.
    pub async fn card(&self, card_id: &str) -> Result<Card, AppError> {
        let block = self.block(card_id).await?;
        Ok(card_from_stored_block(&block, now_millis())?)
    }
}

/// Builds the card view of a stored block.
///
/// Nothing checks what is written to a card block's `properties`, so a stored card may
/// hold something else than an object there. The original then fails to read that card,
/// and with it the whole card list of its board. Here such a card is given out with no
/// properties, and a warning is logged.
fn card_from_stored_block(block: &Block, now: i64) -> Result<Card, CardError> {
    match block.fields.get("properties") {
        None | Some(Value::Object(_)) => Card::from_block(block, now),
        Some(_) => {
            tracing::warn!(
                block_id = %block.id,
                board_id = %block.board_id,
                "the properties of a card are not an object; giving the card out without properties"
            );
            let mut readable = block.clone();
            readable.fields.remove("properties");
            Card::from_block(&readable, now)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(page: u32, per_page: u32) -> CardRange {
        CardRange::Page(Paging { page, per_page })
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
