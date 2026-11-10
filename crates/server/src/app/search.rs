//! Searching boards by title or by the name of a card property.

use fvoci_model::{Board, BoardSearchField};
use fvoci_store::PublicBoards;

use super::App;
use crate::error::AppError;

impl App {
    /// Returns the boards whose `search_field` matches `term`, among the boards the user
    /// is a member of and, with [`PublicBoards::Include`], the open boards. The search
    /// spans all teams and leaves out templates.
    ///
    /// # Errors
    ///
    /// Returns the store's error if the query fails.
    pub async fn search_boards_for_user(
        &self,
        term: &str,
        search_field: BoardSearchField,
        user_id: &str,
        public_boards: PublicBoards,
    ) -> Result<Vec<Board>, AppError> {
        Ok(self
            .store()
            .search_boards_for_user(term, search_field, user_id, public_boards)
            .await?)
    }

    /// Returns the boards of a team whose title matches `term`, among the team's open
    /// boards and the private ones the user is a member of. Templates are left out.
    ///
    /// No route of a standalone server gets as far as calling this: its one caller in
    /// the original, the search for boards that can be linked to a channel, needs the
    /// host application.
    ///
    /// # Errors
    ///
    /// Returns the store's error if the query fails.
    pub async fn search_boards_for_user_in_team(
        &self,
        team_id: &str,
        term: &str,
        user_id: &str,
    ) -> Result<Vec<Board>, AppError> {
        Ok(self
            .store()
            .search_boards_for_user_in_team(team_id, term, user_id)
            .await?)
    }
}
