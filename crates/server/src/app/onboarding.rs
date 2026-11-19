//! A new account's first board.
//!
//! The welcome board is copied for the user from the built-in template that carries
//! the onboarding mark and is written in their language, not from a board with a
//! certain title.

use std::time::Duration;

use fvoci_store::DuplicateAs;

use super::App;
use super::accept_language::Reader;
use super::container_copies::{Stored, titles_in};
use super::language::Language;
use super::templates::{changes_of_copy, is_onboarding_template, written_in};
use crate::error::AppError;
use crate::metrics::ContainerWork;

/// How long somebody who comes in as a member waits for their welcome board to be made.
const WELCOME_WAIT: Duration = Duration::from_secs(5);

impl App {
    /// Copies the welcome board into a workspace for a user, in the language the
    /// server writes in for them (`App::language_for`, by `accept_language`, the
    /// request's `Accept-Language`), and returns the copy's identifier. The copy is a
    /// private board of the user's as any copy of a built-in template is: without the
    /// template's marks, its days moved to the day it is for the user.
    ///
    /// Every call makes a copy: whoever starts an organization asks once. The account
    /// has its welcome board from then on, and is given none as it comes into another
    /// organization (`App::welcome_member`).
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
        let template_id = self.welcome_template_id(reader.language).await?;
        let board_id = self
            .copy_welcome_board(&template_id, user_id, workspace_id, &reader)
            .await?;
        // The board is made, whether or not that is recorded: an account whose record
        // failed may be given another as it comes into an organization.
        if let Err(error) = self.store().claim_welcome(user_id, now).await {
            tracing::warn!(user_id, %error, "an account's welcome board was not recorded");
        }
        Ok(board_id)
    }

    /// Gives somebody who came into the workspace as a member their welcome board, a
    /// private board of their own there, if their account has none yet: an account is
    /// given one once, whatever organizations and workspaces it comes into after. The
    /// callers are those that let members in (a link, a registration made by a link,
    /// a sign-in through the organization's provider); a guest is given none.
    ///
    /// Coming in does not depend on it. The board is made as a task of its own, which
    /// this waits [`WELCOME_WAIT`] for, so that whoever comes in finds their board
    /// there, and no longer: a file storage that is slow keeps nobody out, and the
    /// board is made all the same. One that could not be made is logged, and so is it
    /// on a server that holds no welcome board in the language (it was told to store
    /// no templates, or owes them, which it is then asked to try again): the account
    /// is given its board the next time it comes in somewhere.
    pub(super) async fn welcome_member(
        &self,
        user_id: &str,
        workspace_id: &str,
        accept_language: Option<&str>,
    ) {
        let app = self.clone();
        let (user_id, workspace_id) = (user_id.to_owned(), workspace_id.to_owned());
        let accept_language = accept_language.map(str::to_owned);
        let made = self.spawn_tracked(async move {
            let now = crate::auth::now_millis();
            let made = app
                .welcome_once(&user_id, &workspace_id, accept_language.as_deref(), now)
                .await;
            match made {
                Ok(Some(board_id)) => {
                    tracing::info!(
                        user_id,
                        workspace_id,
                        board_id,
                        "a new member was given their welcome board"
                    );
                }
                Ok(None) => {}
                Err(error) => {
                    tracing::warn!(user_id, workspace_id, %error, "a new member was not given their welcome board");
                }
            }
        });
        if tokio::time::timeout(WELCOME_WAIT, made).await.is_err() {
            tracing::info!("a new member's welcome board is still being made");
        }
    }

    /// [`App::welcome_member`] in the default workspace of the handle's organization,
    /// which somebody who signs in through its provider comes into.
    pub(super) async fn welcome_newcomer(&self, user_id: &str, accept_language: Option<&str>) {
        match self.store().organization().await {
            Ok(organization) => {
                self.welcome_member(user_id, &organization.default_workspace_id, accept_language)
                    .await;
            }
            Err(error) => {
                tracing::warn!(user_id, %error, "a new member was not given their welcome board");
            }
        }
    }

    /// Copies the welcome board for the user unless their account has one, which the
    /// store decides as it stores the copy (`Store::duplicate_welcome_board`): of
    /// calls made at once, one copies. `None` for the others, and where there is no
    /// welcome board to copy.
    async fn welcome_once(
        &self,
        user_id: &str,
        workspace_id: &str,
        accept_language: Option<&str>,
        now: i64,
    ) -> Result<Option<String>, AppError> {
        let reader = self.reader_at(user_id, accept_language, now).await?;
        let Some(template_id) = self.marked_template(reader.language).await? else {
            // Owed, if the file storage did not take them at the start: asked for
            // again, without this waiting for it.
            self.restock_templates_soon();
            return Ok(None);
        };
        let template = self.container(&template_id).await?;
        let changes = changes_of_copy(&template, reader.today.as_deref());
        self.write_container_bodies(&template_id).await?;
        let titles = titles_in(reader.language);
        let stored = self
            .store()
            .duplicate_welcome_board(&template_id, user_id, workspace_id, &titles, changes, now)
            .await?;
        let Some((copies, members)) = stored else {
            return Ok(None);
        };
        let [board] = copies.containers.as_slice() else {
            return Err(AppError::internal("new board wasn't created"));
        };
        let board_id = board.id.clone();
        let stored = Stored {
            source_id: &template_id,
            user_id,
            to_workspace: workspace_id,
            duplicate_as: DuplicateAs::Copy,
            kind: template.kind,
            work: ContainerWork::Welcome,
        };
        if let Err(error) = self.finish_copy(stored, copies, members).await {
            // Stored and recorded. If its files could not be copied the board was
            // deleted again: then the account has none, and the record is taken back.
            let taken_back = self
                .store()
                .get_container(&board_id)
                .await
                .is_err_and(|error| error.is_not_found());
            if taken_back && let Err(error) = self.store().forget_welcome(user_id).await {
                tracing::error!(user_id, %error, "an account that got no welcome board is recorded as given one");
            }
            return Err(error);
        }
        Ok(Some(board_id))
    }

    /// Copies the built-in welcome board `template_id` into the workspace for the
    /// user, and returns the copy's identifier.
    async fn copy_welcome_board(
        &self,
        template_id: &str,
        user_id: &str,
        workspace_id: &str,
        reader: &Reader,
    ) -> Result<String, AppError> {
        let (copies, _members) = self
            .copy_container(
                template_id,
                user_id,
                workspace_id,
                DuplicateAs::Copy,
                reader,
                ContainerWork::Welcome,
            )
            .await?;
        match copies.containers.as_slice() {
            [board] => Ok(board.id.clone()),
            _ => Err(AppError::internal("new board wasn't created")),
        }
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
