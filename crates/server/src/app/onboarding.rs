//! The onboarding tour.
//!
//! Ports `server/app/onboarding.go`. Onboarding duplicates the welcome template board
//! for a user and records the tour state in their preferences. The welcome board is the
//! built-in template that carries the onboarding mark, not a board with a certain title.

use std::collections::BTreeMap;

use fvoci_model::{BoardPatch, BoardType, GLOBAL_TEAM_ID, UserPreferencesPatch};
use fvoci_store::DuplicateAs;

use super::App;
use super::templates::{ONBOARDING_PROPERTY, is_onboarding_template};
use crate::error::AppError;

/// The preference that records the tour has started.
const KEY_TOUR_STARTED: &str = "onboardingTourStarted";
/// The preference that records the tour step.
const KEY_TOUR_STEP: &str = "onboardingTourStep";
/// The preference that records the tour category.
const KEY_TOUR_CATEGORY: &str = "tourCategory";
/// The first step of the tour.
const VALUE_FIRST_STEP: &str = "0";
/// The tour category value.
const VALUE_TOUR_CATEGORY: &str = "onboarding";

impl App {
    /// Duplicates the welcome board for a user and records the tour state, and returns
    /// the team and the new board's identifier.
    ///
    /// # Errors
    ///
    /// Returns a server error if the welcome board is missing or could not be copied, or
    /// the store's error.
    pub async fn prepare_onboarding_tour(
        &self,
        user_id: &str,
        team_id: &str,
    ) -> Result<(String, String), AppError> {
        let board_id = self.create_welcome_board(user_id, team_id).await?;

        let mut updated_fields = BTreeMap::new();
        updated_fields.insert(KEY_TOUR_STARTED.to_owned(), "1".to_owned());
        updated_fields.insert(KEY_TOUR_STEP.to_owned(), VALUE_FIRST_STEP.to_owned());
        updated_fields.insert(KEY_TOUR_CATEGORY.to_owned(), VALUE_TOUR_CATEGORY.to_owned());
        let patch = UserPreferencesPatch {
            updated_fields,
            ..UserPreferencesPatch::default()
        };
        self.store().patch_user_preferences(user_id, &patch).await?;

        Ok((team_id.to_owned(), board_id))
    }

    /// Duplicates the welcome board into a team for a user, makes the copy private, and
    /// returns its identifier.
    async fn create_welcome_board(&self, user_id: &str, team_id: &str) -> Result<String, AppError> {
        let onboarding_board_id = self.onboarding_board_id().await?;

        let (boards_and_blocks, _members) = self
            .duplicate_board(&onboarding_board_id, user_id, team_id, DuplicateAs::Copy)
            .await?;
        let [board] = boards_and_blocks.boards.as_slice() else {
            return Err(AppError::internal("new board wasn't created"));
        };
        let board_id = board.id.clone();

        // The copy is a user's board, not the built-in template: it loses the mark.
        let patch = BoardPatch {
            board_type: Some(BoardType::Private),
            deleted_properties: vec![ONBOARDING_PROPERTY.to_owned()],
            ..BoardPatch::default()
        };
        self.patch_board(&patch, &board_id, user_id).await?;
        Ok(board_id)
    }

    /// The identifier of the built-in template the tour copies.
    ///
    /// The original takes the first open template of the global team that has the
    /// welcome board's title (`server/app/onboarding.go:47-66`). The global team is also
    /// the team every user of a standalone server makes boards in, so a template that
    /// any user gave that title could be handed to new users as their first board.
    async fn onboarding_board_id(&self) -> Result<String, AppError> {
        let boards = self.store().get_template_boards(GLOBAL_TEAM_ID, "").await?;
        boards
            .into_iter()
            .find(is_onboarding_template)
            .map(|board| board.id)
            .ok_or_else(|| {
                AppError::internal("unable to find welcome board in newly created blocks")
            })
    }
}
