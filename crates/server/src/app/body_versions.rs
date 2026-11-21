//! The versions kept of a card's body (`crates/server/src/collab`, which keeps them),
//! as the card's readers are given them, and as its editors go back to one.

use fvoci_model::{Block, BlockPatch, Body, BodyChange, BodyVersion, BodyVersionContent};
use fvoci_store::{KeptVersion, body_digest};

use super::{App, ChangeNotification};
use crate::error::AppError;

impl App {
    /// The versions kept of a card's body, the newest first, without their bodies.
    /// The newest whose body is the one the card has says so. None says so while the
    /// card's editors have typed what is not written to it yet: its body is then no
    /// version's, though the card still holds an older one. That is asked before the
    /// card is read, so that a body written to the card in between is the one
    /// compared. The three are read one after another and not as one: of a card whose
    /// version is kept in between, an answer may name an older version with that body,
    /// or none, and the next names the new one.
    ///
    /// # Errors
    ///
    /// Returns a not-found error if the card is not there, and the store's error.
    pub async fn body_versions(&self, card_id: &str) -> Result<Vec<BodyVersion>, AppError> {
        let kept = self.store().body_versions(card_id).await?;
        let unwritten = self.store().has_unwritten_body(card_id).await?;
        let card = self.block(card_id).await?;
        let digest = body_digest(card.body.as_ref());
        let current = if unwritten {
            None
        } else {
            kept.iter()
                .find(|kept| kept.body_digest == digest)
                .map(|kept| kept.version)
        };
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

    /// Writes the body of a version to a card again, as `user_id` writing the whole
    /// body: the card's document is made again from it and everyone in its room is
    /// sent that, the body it is written over is kept as a version first (with what
    /// its editors typed), and the body written is the card's newest version, by
    /// `user_id`. A card that has that body already is left as it is, whatever the
    /// body. Gives the card's block as it is after; websocket listeners and webhooks
    /// are told of a change in the background.
    ///
    /// A body that is written is held to what a body a client sends is held to: what
    /// may be stored may have changed since the version was kept.
    ///
    /// # Errors
    ///
    /// Returns a not-found error if the card has no such version (any more) or is not
    /// there any more, a conflict if the version's body may not be stored any more or
    /// names a file (of an image or a file block) that is not there any more, and the
    /// errors of a write of the whole body ([`crate::collab::Collab::write_whole`]).
    pub async fn restore_body_version(
        &self,
        card: &Block,
        version: i64,
        user_id: &str,
    ) -> Result<Block, AppError> {
        let (_, body) = self
            .store()
            .body_version(&card.id, version)
            .await?
            .ok_or_else(|| AppError::not_found("version"))?;
        let container = self.store().get_container(&card.container_id).await?;
        let (id, by) = (card.id.clone(), user_id.to_owned());
        let changed = self
            .collab()
            .write_whole(self, vec![id.clone()], async move |app| {
                app.write_version(&id, body, &by).await
            })
            .await?;
        let block = self.store().get_block(&card.id).await?;
        if changed {
            self.tell_of_update(&container.workspace_id, &block, ChangeNotification::Send);
        }
        Ok(block)
    }

    /// Writes the body of a version to its card while the card's place is held, once
    /// what its editors typed is written to it: whether the card was written. A card
    /// that holds that body with nothing typed since is left alone.
    async fn write_version(
        &self,
        card_id: &str,
        body: Option<Body>,
        user_id: &str,
    ) -> Result<bool, AppError> {
        let held = self.store().get_block(card_id).await?;
        if body_digest(held.body.as_ref()) == body_digest(body.as_ref()) {
            return Ok(false);
        }
        let change = match body {
            None => BodyChange::Clear,
            Some(body) => BodyChange::parse(body.document().clone()).map_err(|error| {
                AppError::conflict(format_args!(
                    "the body of the version may not be stored any more: {error}"
                ))
            })?,
        };
        if !self
            .has_body_files(&held.container_id, change.new_body())
            .await?
        {
            return Err(AppError::conflict(
                "the version names a file (of an image or a file block) that is not there any \
                 more",
            ));
        }
        let patch = BlockPatch {
            body: change,
            ..BlockPatch::default()
        };
        // The words of a version are whose wrote them then: whom they name is not told
        // by who brings them back.
        self.store()
            .telling_nobody()
            .patch_block(card_id, &patch, user_id)
            .await?;
        Ok(true)
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
