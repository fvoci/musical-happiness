//! The versions kept of a card's body (`crates/server/src/collab`, which keeps them),
//! as the card's readers are given them.

use fvoci_model::{BodyVersion, BodyVersionContent};
use fvoci_store::KeptVersion;

use super::App;
use crate::collab::body_digest;
use crate::error::AppError;

impl App {
    /// The versions kept of a card's body, the newest first, without their bodies.
    /// Each says whether its body is the one the card has: the card is read after
    /// the versions, so that a version kept meanwhile is not taken for an older one.
    ///
    /// # Errors
    ///
    /// Returns a not-found error if the card is not there, and the store's error.
    pub async fn body_versions(&self, card_id: &str) -> Result<Vec<BodyVersion>, AppError> {
        let kept = self.store().body_versions(card_id).await?;
        let current = self.digest_of_card(card_id).await?;
        Ok(kept
            .into_iter()
            .map(|kept| described(kept, &current))
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
        let (kept, body) = self
            .store()
            .body_version(card_id, version)
            .await?
            .ok_or_else(|| AppError::not_found("version"))?;
        let described = described(kept, &self.digest_of_card(card_id).await?);
        Ok(BodyVersionContent {
            version: described.version,
            kept_at: described.kept_at,
            editors: described.editors,
            is_current: described.is_current,
            body,
        })
    }

    /// The digest of the body a card has now.
    async fn digest_of_card(&self, card_id: &str) -> Result<Vec<u8>, AppError> {
        let card = self.block(card_id).await?;
        Ok(body_digest(card.body.as_ref()))
    }
}

/// A kept version as a reader is given it, of a card whose body has the digest
/// `current`.
fn described(kept: KeptVersion, current: &[u8]) -> BodyVersion {
    BodyVersion {
        version: kept.version,
        kept_at: kept.kept_at,
        is_current: kept.body_digest == current,
        editors: kept.editors,
    }
}
