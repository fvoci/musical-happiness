//! Public sharing of boards through read tokens.

use fvoci_model::Sharing;
use fvoci_store::{SharingChange, StoreError};

use super::App;
use crate::auth::secret::new_secret;
use crate::error::AppError;

impl App {
    /// Returns the sharing state of a board. A board that was never shared is not
    /// shared.
    ///
    /// # Errors
    ///
    /// Returns the store's error if the state cannot be read.
    pub async fn sharing(&self, board_id: &str) -> Result<Sharing, AppError> {
        match self.store().get_sharing(board_id).await {
            Ok(sharing) => Ok(sharing),
            Err(StoreError::NoRows) => Ok(Sharing {
                board_id: board_id.to_owned(),
                ..Sharing::default()
            }),
            Err(error) => Err(error.into()),
        }
    }

    /// Changes whether a board is shared, on behalf of a user, and returns the sharing
    /// state as stored. A board that becomes shared, or is shared anew, gets a read
    /// token the server makes; one that stops being shared loses its token.
    ///
    /// # Errors
    ///
    /// Returns a not-found error if there is no such board.
    pub async fn set_sharing(
        &self,
        board_id: &str,
        change: SharingChange,
        user_id: &str,
    ) -> Result<Sharing, AppError> {
        match self
            .store()
            .set_sharing(board_id, change, &new_secret(), user_id)
            .await
        {
            Ok(sharing) => Ok(sharing),
            Err(error) if error.is_not_found() => Err(AppError::not_found("board")),
            Err(error) => Err(error.into()),
        }
    }
}
