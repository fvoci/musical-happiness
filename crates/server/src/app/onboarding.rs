//! A new account's first board.
//!
//! The welcome board is copied for the user from the built-in template that carries
//! the onboarding mark and is written in their language, not from a board with a
//! certain title.

use fvoci_store::DuplicateAs;

use super::App;
use super::language::Language;
use super::templates::{is_onboarding_template, written_in};
use crate::error::AppError;
use crate::metrics::ContainerWork;

impl App {
    /// Copies the welcome board into a workspace for a user, in the language the
    /// server writes in for them ([`App::language_for`], by `accept_language`, the
    /// request's `Accept-Language`), and returns the copy's identifier. The copy is a
    /// private board of the user's as any copy of a built-in template is: without the
    /// template's marks, its days moved to the day it is for the user.
    ///
    /// # Errors
    ///
    /// Returns a server error if the welcome board is missing or could not be copied, or
    /// the store's error.
    pub async fn create_welcome_board(
        &self,
        user_id: &str,
        workspace_id: &str,
        accept_language: Option<&str>,
    ) -> Result<String, AppError> {
        let now = crate::auth::now_millis();
        self.create_welcome_board_at(user_id, workspace_id, accept_language, now)
            .await
    }

    /// [`App::create_welcome_board`] at the time `now`, in milliseconds.
    ///
    /// # Errors
    ///
    /// As [`App::create_welcome_board`].
    pub async fn create_welcome_board_at(
        &self,
        user_id: &str,
        workspace_id: &str,
        accept_language: Option<&str>,
        now: i64,
    ) -> Result<String, AppError> {
        let reader = self.reader_at(user_id, accept_language, now).await?;
        let welcome_board_id = self.welcome_template_id(reader.language).await?;

        let (boards_and_blocks, _members) = self
            .copy_container(
                &welcome_board_id,
                user_id,
                workspace_id,
                DuplicateAs::Copy,
                &reader,
                ContainerWork::Welcome,
            )
            .await?;
        let [board] = boards_and_blocks.containers.as_slice() else {
            return Err(AppError::internal("new board wasn't created"));
        };
        Ok(board.id.clone())
    }

    /// The identifier of the built-in welcome board written in `language`.
    ///
    /// It is looked for among the built-in templates only. A template that a user gave
    /// the welcome board's title, or its mark, is never handed to new users as their
    /// first board.
    ///
    /// When there is none (the templates were removed, or storing them at the start
    /// failed), the built-in templates are stored again: they always hold the marked
    /// board of each language.
    async fn welcome_template_id(&self, language: Language) -> Result<String, AppError> {
        if let Some(board) = self.marked_template(language).await? {
            return Ok(board);
        }
        tracing::warn!(
            language = language.tag(),
            "no built-in template is the welcome board of the language: storing them again"
        );
        self.init_templates().await?;
        self.marked_template(language)
            .await?
            .ok_or_else(|| AppError::internal("the built-in templates hold no welcome board"))
    }

    /// The identifier of the built-in welcome board written in `language`, if there is
    /// one.
    async fn marked_template(&self, language: Language) -> Result<Option<String>, AppError> {
        let boards = self.store().get_builtin_templates().await?;
        Ok(boards
            .into_iter()
            .find(|board| is_onboarding_template(board) && written_in(board) == Some(language))
            .map(|board| board.id))
    }
}
