//! The versions kept of a card's body (`crates/server/src/collab`, which keeps them),
//! as the card's readers are given them, and as its editors go back to one.

use fvoci_model::{Block, BlockPatch, BlockType, BodyChange, BodyVersion, BodyVersionContent};
use fvoci_store::{KeptVersion, body_digest};

use super::{App, ChangeNotification};
use crate::error::AppError;

impl App {
    /// The versions kept of a card's body, the newest first, without their bodies.
    /// The newest whose body is the one the card has says so: the card is read after
    /// the versions, so that a version kept meanwhile is not taken for an older one.
    ///
    /// # Errors
    ///
    /// Returns a not-found error if the card is not there, and the store's error.
    pub async fn body_versions(&self, card_id: &str) -> Result<Vec<BodyVersion>, AppError> {
        let kept = self.store().body_versions(card_id).await?;
        let card = self.block(card_id).await?;
        let digest = body_digest(card.body.as_ref());
        let current = kept
            .iter()
            .find(|kept| kept.body_digest == digest)
            .map(|kept| kept.version);
        Ok(kept
            .into_iter()
            .map(|kept| described(kept, current))
            .collect())
    }

    /// A version kept of a card's body, with its body.
    ///
    /// # Errors
    ///
    /// Returns a not-found error if the card has no version of that number (any
    /// more) or is not there, and the store's error.
    pub async fn body_version(
        &self,
        card_id: &str,
        version: i64,
    ) -> Result<BodyVersionContent, AppError> {
        let (_, body) = self
            .store()
            .body_version(card_id, version)
            .await?
            .ok_or_else(|| AppError::not_found("version"))?;
        // As the list has it: which version is the card's body is said of one.
        let listed = self
            .body_versions(card_id)
            .await?
            .into_iter()
            .find(|listed| listed.version == version)
            .ok_or_else(|| AppError::not_found("version"))?;
        Ok(BodyVersionContent {
            version: listed.version,
            kept_at: listed.kept_at,
            editors: listed.editors,
            is_current: listed.is_current,
            body,
        })
    }

    /// Writes the body of a version to its card again, as `user_id` writing the whole
    /// body: the card's document is made again from it and everyone in its room is
    /// sent that, the body it is written over is kept as a version first (with what
    /// its editors typed), and the body written is the card's newest version, by
    /// `user_id`. A card that has that body already is left as it is. Gives the card's
    /// block as it is after; websocket listeners and webhooks are told of a change in
    /// the background.
    ///
    /// The body is held to what a body a client sends is held to: what may be stored
    /// may have changed since the version was kept.
    ///
    /// # Errors
    ///
    /// Returns a not-found error if the card has no such version (any more) or is not
    /// there, a conflict if the version's body may not be stored any more or shows an
    /// image whose file the board no longer has, and the errors of a write of the
    /// whole body ([`crate::collab::Collab::write_whole`]).
    pub async fn restore_body_version(
        &self,
        card_id: &str,
        version: i64,
        user_id: &str,
    ) -> Result<Block, AppError> {
        let (_, body) = self
            .store()
            .body_version(card_id, version)
            .await?
            .ok_or_else(|| AppError::not_found("version"))?;
        let change = match body {
            None => BodyChange::Clear,
            Some(body) => BodyChange::parse(body.document().clone()).map_err(|error| {
                AppError::conflict(format_args!(
                    "the body of the version may not be stored any more: {error}"
                ))
            })?,
        };
        let card = self.store().get_block(card_id).await?;
        if card.block_type != BlockType::Card {
            return Err(AppError::not_found("card"));
        }
        let board = self.store().get_board(&card.board_id).await?;
        for file_id in change
            .new_body()
            .map(|body| body.file_ids())
            .unwrap_or_default()
        {
            if self.file_info(&card.board_id, file_id).await?.is_none() {
                return Err(AppError::conflict(
                    "the version shows an image whose file the board does not have any more",
                ));
            }
        }
        let wanted = body_digest(change.new_body());
        let patch = BlockPatch {
            body: change,
            ..BlockPatch::default()
        };
        let (id, by) = (card_id.to_owned(), user_id.to_owned());
        let changed = self
            .collab()
            .write_whole(self, vec![id.clone()], async move |app| {
                // Read once what its editors typed is written to it: a card that
                // holds the version's body with nothing typed since is left alone.
                let held = app.store().get_block(&id).await?;
                if body_digest(held.body.as_ref()) == wanted {
                    return Ok(false);
                }
                app.store().patch_block(&id, &patch, &by).await?;
                Ok(true)
            })
            .await?;
        let block = self.store().get_block(card_id).await?;
        if changed {
            self.tell_of_update(&board.workspace_id, &block, ChangeNotification::Send);
        }
        Ok(block)
    }
}

/// A kept version as a reader is given it, of a card whose body is that of the
/// version `current`, if of any.
fn described(kept: KeptVersion, current: Option<i64>) -> BodyVersion {
    BodyVersion {
        version: kept.version,
        kept_at: kept.kept_at,
        is_current: current == Some(kept.version),
        editors: kept.editors,
    }
}
