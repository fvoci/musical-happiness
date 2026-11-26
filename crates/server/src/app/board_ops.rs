//! Atomic server board operations: pre-copied card duplication, multi-card view moves,
//! batch card delete/restore, fractional-indexed board views, verified card attachments,
//! and blank board scaffolding.

use std::collections::HashMap;

use fvoci_model::{
    Block, BlockType, BoardLocale, BoardViewDuplicate, BoardViewMove, BoardViewWrite, Card,
    CardMovesRequest, CardMovesResult, Container, ContainerKind, JsonMap, NewBoardView,
    NewCardAttachment, RestoredCards, ViewType, default_blank_board_scaffolding,
};
use fvoci_store::DuplicateAs;
use serde_json::{Value, json};

use super::blocks::{ChangeAction, hide_inactive_numbers, log_notification};
use super::container_copies::titles_in;
use super::language::Language;
use super::{App, ChangeNotification};
use crate::collab::Deletion;
use crate::error::AppError;

impl App {
    /// Atomically duplicates `card_id` on `board_id`, pre-copying referenced storage
    /// files under `loose_objects` marks first and then committing `files` records,
    /// cloned blocks (with remapped file IDs), and initial `body_versions` in a single
    /// database transaction.
    ///
    /// If copying any file in storage fails, all newly written objects are given up and
    /// an error is returned without committing any block rows.
    ///
    /// # Errors
    ///
    /// Returns 404 if the board or card does not exist, 413 (`storage_full`) if copying
    /// files would exceed `max_org_storage_bytes`, and 500 if storage copy fails.
    pub async fn duplicate_card(
        &self,
        board_id: &str,
        card_id: &str,
        user_id: &str,
        duplicate_as: DuplicateAs,
    ) -> Result<Card, AppError> {
        let container = self.container(board_id).await?;
        let mut sources = vec![card_id.to_owned()];
        sources.extend(
            self.blocks_below(card_id)
                .await?
                .into_iter()
                .filter(|child| {
                    child.container_id == board_id && child.block_type == BlockType::Card
                })
                .map(|child| child.id),
        );
        self.collab().write_bodies(self, &sources).await?;

        let prepared = self
            .store()
            .prepare_card_duplicate(board_id, card_id, duplicate_as)
            .await?;
        let file_ids: Vec<String> = prepared
            .source_files
            .iter()
            .map(|file| file.stored_name.clone())
            .collect();
        let (file_map, staged_files, written_places) = self
            .pre_copy_files_to_container(
                self,
                board_id,
                &prepared.org_id,
                board_id,
                &file_ids,
                user_id,
            )
            .await?;

        let mut blocks = prepared.blocks;
        if let Err(error) = remap_copied_block_files(&mut blocks, &file_map) {
            for place in written_places {
                place.give_up().await;
            }
            return Err(error);
        }

        let committed = {
            let _structure = self.collab().changing_structure(self, board_id).await;
            self.store()
                .commit_card_duplicate(board_id, blocks, &staged_files, user_id)
                .await
        };
        let mut stored = match committed {
            Ok(stored) => {
                for place in written_places {
                    place.settled_in_transaction();
                }
                stored
            }
            Err(error) => {
                for place in written_places {
                    place.give_up().await;
                }
                return Err(error.into());
            }
        };
        hide_inactive_numbers(&mut stored);
        let root = stored
            .first()
            .ok_or_else(|| AppError::not_found("card"))?
            .clone();
        let card = self.card_of_block(&root).await?;

        let app = self.clone();
        let ws_id = container.workspace_id;
        self.spawn_background(async move {
            for block in &stored {
                app.ws().broadcast_block_change(&ws_id, block).await;
            }
        });
        Ok(card)
    }

    /// Atomically moves `1..=200` cards in `view_id` of `board_id`, acquiring the
    /// `Collab` structure lock before the database `lock_container` lock and broadcasting
    /// the updated view before the updated cards.
    ///
    /// # Errors
    ///
    /// Returns 400 for duplicate/out-of-range `cardIds`, invalid `groupPropertyId` /
    /// `groupValue`, or invalid `afterCardId`, and 404 if the board or view is absent.
    pub async fn move_cards_in_view(
        &self,
        board_id: &str,
        view_id: &str,
        request: &CardMovesRequest,
        user_id: &str,
        notification: ChangeNotification,
    ) -> Result<CardMovesResult, AppError> {
        let container = self.container(board_id).await?;
        let (mut view, mut updated_blocks) = {
            let _structure = self.collab().changing_structure(self, board_id).await;
            self.store_telling(notification)
                .move_cards_in_view(board_id, view_id, request, user_id)
                .await?
        };
        view.hide_inactive_number();
        hide_inactive_numbers(&mut updated_blocks);
        let mut cards = Vec::with_capacity(updated_blocks.len());
        for block in &updated_blocks {
            cards.push(self.card_of_block(block).await?);
            log_notification(notification, ChangeAction::Update, &block.id);
        }
        log_notification(notification, ChangeAction::Update, &view.id);

        let app = self.clone();
        let ws_id = container.workspace_id;
        let view_for_ws = view.clone();
        self.spawn_background(async move {
            app.ws().broadcast_block_change(&ws_id, &view_for_ws).await;
            app.tell_webhooks(&view_for_ws);
            for card_block in &updated_blocks {
                app.ws().broadcast_block_change(&ws_id, card_block).await;
                app.tell_webhooks(card_block);
            }
        });
        Ok(CardMovesResult { view, cards })
    }

    /// Atomically deletes `1..=200` cards on `board_id` as individually restorable
    /// deletion families, acquiring the `Collab` container gate before the database lock.
    ///
    /// # Errors
    ///
    /// Returns 400 for invalid/duplicate `card_ids` or non-card blocks, and 404 if the
    /// board or any requested card does not exist.
    pub async fn batch_delete_cards(
        &self,
        board_id: &str,
        card_ids: &[String],
        user_id: &str,
        notification: ChangeNotification,
    ) -> Result<(), AppError> {
        let container = self.container(board_id).await?;
        let owned_board = board_id.to_owned();
        let owned_ids = card_ids.to_vec();
        let owned_user = user_id.to_owned();
        let deleted = self
            .collab()
            .remove(
                self,
                Deletion::Blocks {
                    container_id: board_id.to_owned(),
                    block_ids: card_ids.to_vec(),
                },
                async move |app| {
                    Ok(app
                        .store_telling(notification)
                        .batch_delete_cards(&owned_board, &owned_ids, &owned_user)
                        .await?)
                },
            )
            .await?;
        for card_id in card_ids {
            log_notification(notification, ChangeAction::Delete, card_id);
        }
        let deleted_card_ids: Vec<String> = deleted
            .into_iter()
            .filter(|block| block.block_type == BlockType::Card)
            .map(|block| block.id)
            .collect();
        let app = self.clone();
        let ws_id = container.workspace_id;
        let board_owned = board_id.to_owned();
        self.spawn_background(async move {
            for deleted_id in &deleted_card_ids {
                app.ws()
                    .broadcast_block_delete(&ws_id, deleted_id, &board_owned, BlockType::Card)
                    .await;
            }
        });
        Ok(())
    }

    /// Atomically restores `1..=200` deleted cards on `board_id`, acquiring the `Collab`
    /// structure lock before the database lock and returning the restored cards.
    ///
    /// # Errors
    ///
    /// Returns 400 for invalid/duplicate `card_ids`, 404 if the board or any requested
    /// card is not in the trash, and 409 if a card cannot be restored.
    pub async fn batch_restore_cards(
        &self,
        board_id: &str,
        card_ids: &[String],
        user_id: &str,
    ) -> Result<RestoredCards, AppError> {
        let container = self.container(board_id).await?;
        let mut restored = {
            let _structure = self.collab().changing_structure(self, board_id).await;
            self.store()
                .batch_restore_cards(board_id, card_ids, user_id)
                .await?
        };
        hide_inactive_numbers(&mut restored);
        let mut cards = Vec::new();
        for block in &restored {
            if block.block_type == BlockType::Card {
                cards.push(self.card_of_block(block).await?);
                log_notification(ChangeNotification::Send, ChangeAction::Add, &block.id);
            }
        }

        let app = self.clone();
        let ws_id = container.workspace_id;
        self.spawn_background(async move {
            for block in &restored {
                if block.block_type == BlockType::Card {
                    app.ws().broadcast_block_change(&ws_id, block).await;
                    app.tell_webhooks(block);
                }
            }
        });
        Ok(RestoredCards { cards })
    }

    /// Creates a fractional-indexed board view on `board_id`.
    ///
    /// # Errors
    ///
    /// Returns 404 if `board_id` does not exist and 400 if `new` is invalid.
    pub async fn create_board_view(
        &self,
        board_id: &str,
        new: &NewBoardView,
        user_id: &str,
    ) -> Result<BoardViewWrite, AppError> {
        let container = self.container(board_id).await?;
        let write = {
            let _structure = self.collab().changing_structure(self, board_id).await;
            self.store()
                .create_board_view(board_id, new, user_id)
                .await?
        };
        self.broadcast_view_write(container.workspace_id, &write);
        Ok(write)
    }

    /// Moves `view_id` on `board_id` using server-side fractional indexing.
    ///
    /// # Errors
    ///
    /// Returns 404 if `board_id` or `view_id` does not exist and 400 if `mv.after` is
    /// invalid.
    pub async fn move_board_view(
        &self,
        board_id: &str,
        view_id: &str,
        mv: &BoardViewMove,
        user_id: &str,
    ) -> Result<BoardViewWrite, AppError> {
        let container = self.container(board_id).await?;
        let write = {
            let _structure = self.collab().changing_structure(self, board_id).await;
            self.store()
                .move_board_view(board_id, view_id, mv, user_id)
                .await?
        };
        self.broadcast_view_write(container.workspace_id, &write);
        Ok(write)
    }

    /// Duplicates `view_id` on `board_id` using server-side fractional indexing.
    ///
    /// # Errors
    ///
    /// Returns 404 if `board_id` or `view_id` does not exist and 400 if `dup` is invalid.
    pub async fn duplicate_board_view(
        &self,
        board_id: &str,
        view_id: &str,
        dup: &BoardViewDuplicate,
        user_id: &str,
        accept_language: Option<&str>,
    ) -> Result<BoardViewWrite, AppError> {
        let container = self.container(board_id).await?;
        let language = self.language_for(user_id, accept_language).await?;
        let titles = titles_in(language, ContainerKind::Board);
        let write = {
            let _structure = self.collab().changing_structure(self, board_id).await;
            self.store()
                .duplicate_board_view(board_id, view_id, dup, titles.copy_of, user_id)
                .await?
        };
        self.broadcast_view_write(container.workspace_id, &write);
        Ok(write)
    }

    /// Creates a verified attachment block on `card_id` of `board_id` after locking and
    /// verifying that `request.file_id` belongs to `board_id`.
    ///
    /// # Errors
    ///
    /// Returns 404 if `board_id` or `card_id` is absent and 400 if `request.file_id` does
    /// not belong to `board_id`.
    pub async fn create_card_attachment(
        &self,
        board_id: &str,
        card_id: &str,
        request: &NewCardAttachment,
        user_id: &str,
        notification: ChangeNotification,
    ) -> Result<Block, AppError> {
        let container = self.container(board_id).await?;
        let attachment = {
            let _structure = self.collab().changing_structure(self, board_id).await;
            self.store_telling(notification)
                .create_card_attachment(board_id, card_id, &request.file_id, user_id)
                .await?
        };
        log_notification(notification, ChangeAction::Add, &attachment.id);
        let app = self.clone();
        let ws_id = container.workspace_id;
        let changed = attachment.clone();
        self.spawn_background(async move {
            app.ws().broadcast_block_change(&ws_id, &changed).await;
            app.tell_webhooks(&changed);
        });
        Ok(attachment)
    }
}

/// Whether a newly created blank board receives default properties and views.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoardScaffolding {
    /// Leave properties and views empty.
    None,
    /// Seed localized `status`/`assignee`/`due` properties plus Kanban and Table views.
    Default,
}

impl App {
    /// Creates a new board container, optionally seeding default `status`/`assignee`/`due`
    /// properties, `done_when`, `due_date_property_id`, and an initial Kanban view via
    /// [`App::create_board_view`].
    ///
    /// # Errors
    ///
    /// Returns the errors of [`App::create_container_with_admin`] or
    /// [`App::create_board_view`].
    pub async fn create_board_maybe_scaffolded(
        &self,
        mut container: Container,
        scaffolding: BoardScaffolding,
        user_id: &str,
        accept_language: Option<&str>,
    ) -> Result<Container, AppError> {
        if scaffolding == BoardScaffolding::None
            || container.is_template
            || !container.card_properties.is_empty()
        {
            return self.create_container_with_admin(container, user_id).await;
        }
        let language = self.language_for(user_id, accept_language).await?;
        let locale = if language == Language::Korean {
            BoardLocale::Korean
        } else {
            BoardLocale::English
        };
        let (props, done_when, due_id) = default_blank_board_scaffolding(locale);
        let visible_groups: Vec<String> =
            props[0].options.iter().map(|opt| opt.id.clone()).collect();
        let assignee_id = props[1].id.clone();
        let status_id = done_when.property_id.clone();
        if container.due_date_property_id.is_none() {
            container.due_date_property_id = Some(due_id.clone());
        }
        if container.done_when.is_none() {
            container.done_when = Some(done_when);
        }
        container.card_properties = props;

        let created = self.create_container_with_admin(container, user_id).await?;
        let mut fields = JsonMap::new();
        fields.insert("groupBy".to_owned(), Value::String(status_id));
        fields.insert("visibleGroups".to_owned(), json!(visible_groups));
        fields.insert("hiddenGroups".to_owned(), json!([""]));
        fields.insert("shownProperties".to_owned(), json!([assignee_id, due_id]));
        let initial_view = NewBoardView {
            title: if locale == BoardLocale::Korean {
                "보드"
            } else {
                "Board"
            }
            .to_owned(),
            view_type: ViewType::Board,
            after: None,
            fields: Some(fields),
        };
        let _ = self
            .create_board_view(&created.id, &initial_view, user_id)
            .await?;
        Ok(created)
    }

    fn broadcast_view_write(&self, workspace_id: String, write: &BoardViewWrite) {
        let app = self.clone();
        let companions = write.companion_views.clone();
        let view = write.view.clone();
        self.spawn_background(async move {
            for companion in &companions {
                app.ws()
                    .broadcast_block_change(&workspace_id, companion)
                    .await;
                app.tell_webhooks(companion);
            }
            app.ws().broadcast_block_change(&workspace_id, &view).await;
            app.tell_webhooks(&view);
        });
    }
}

pub(super) fn remap_copied_block_files(
    blocks: &mut [Block],
    file_map: &HashMap<String, String>,
) -> Result<(), AppError> {
    for block in blocks.iter_mut() {
        if block
            .body
            .as_ref()
            .is_some_and(|body| !body.file_ids().is_empty())
        {
            block.body = block
                .body
                .take()
                .and_then(|body| body.with_files_renamed(|old| file_map.get(old).cloned()));
            if let Some(body) = &block.body {
                body.check_size()
                    .map_err(|error| AppError::from_body_error("body", error))?;
            }
        }
        if let Some(old_file_id) = block.attachment_file() {
            let new_name = file_map.get(old_file_id).cloned().unwrap_or_default();
            block
                .fields
                .insert("fileId".to_owned(), Value::String(new_name));
            block.fields.remove("attachmentId");
        }
    }
    Ok(())
}
