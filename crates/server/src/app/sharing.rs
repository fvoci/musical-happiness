//! Public sharing of boards through read tokens.

use fvoci_model::Sharing;
use fvoci_store::StoreError;

use super::App;
use crate::error::AppError;

impl App {
    /// Returns the sharing state of a board.
    ///
    /// # Errors
    ///
    /// Returns a not-found error if no sharing state was ever stored for the board, or
    /// the store's error if it cannot be read.
    pub async fn sharing(&self, board_id: &str) -> Result<Sharing, AppError> {
        match self.store().get_sharing(board_id).await {
            Ok(sharing) => Ok(sharing),
            // Preserved quirk: the client is told what the original's database layer
            // says about an empty result (server/services/store/sqlstore/sharing.go:55).
            Err(error @ StoreError::NoRows) => Err(AppError::NotFound(error.to_string())),
            Err(error) => Err(error.into()),
        }
    }

    /// Stores the sharing state of the board that `sharing` names, replacing any earlier
    /// state. The modification time stored is the current time.
    ///
    /// # Errors
    ///
    /// Returns the store's error if the state cannot be written.
    pub async fn upsert_sharing(&self, sharing: &Sharing) -> Result<(), AppError> {
        Ok(self.store().upsert_sharing(sharing).await?)
    }
}
