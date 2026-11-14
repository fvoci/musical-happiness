//! The versions kept of a card's body (`crates/server/src/collab`, which keeps them),
//! as the card's readers are given them.

use fvoci_model::{BodyVersion, BodyVersionContent};
use fvoci_store::{KeptVersion, body_digest};

use super::App;
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
