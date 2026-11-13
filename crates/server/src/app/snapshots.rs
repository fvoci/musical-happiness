//! Writing pending collaborative bodies before copying or exporting their cards.

use fvoci_model::BlockType;
use fvoci_store::{BlockFilter, Previews};

use super::App;
use crate::error::AppError;

impl App {
    /// Writes every card's pending body before a board's stored blocks are copied.
    /// Listing without bodies avoids reading them once here and again for the copy.
    pub(super) async fn write_board_bodies(&self, board_id: &str) -> Result<(), AppError> {
        let cards = self
            .store()
            .get_listed_blocks(
                &BlockFilter::board(board_id).with_type(BlockType::Card),
                Previews::Without,
            )
            .await?;
        let card_ids: Vec<String> = cards.into_iter().map(|card| card.block.id).collect();
        self.collab().write_bodies(self, &card_ids).await
    }
}
