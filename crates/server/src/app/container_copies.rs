//! Copying a board or a wiki whole.

use fvoci_model::{
    ContainerError, ContainerKind, ContainerMember, ContainersAndBlocks, DeleteContainersAndBlocks,
};
use fvoci_store::{CopyTitles, DuplicateAs};

use super::App;
use super::accept_language::Reader;
use super::language::Language;
use super::pages::refused_without_member;
use super::templates::changes_of_copy;
use crate::error::AppError;
use crate::metrics::{ContainerWork, Module};

/// Copies that are stored, and are still to be finished ([`App::finish_copy`]).
#[derive(Debug, Clone, Copy)]
pub(super) struct Stored<'a> {
    /// What they are copies of.
    pub(super) source_id: &'a str,
    /// Whose they are.
    pub(super) user_id: &'a str,
    /// The workspace they are in.
    pub(super) to_workspace: &'a str,
    /// As what they were made.
    pub(super) duplicate_as: DuplicateAs,
    /// The kind of what was copied.
    pub(super) kind: ContainerKind,
    /// What the copy is counted as.
    pub(super) work: ContainerWork,
}

impl App {
    /// Copies a board and its blocks, except comments, into `to_workspace` for a user, who
    /// becomes the copy's admin. Returns the copies and that membership.
    ///
    /// A wiki is copied with its pages as they stand, their tree, bodies and icons, held
    /// to the organization's limits of pages; the pages in its trash, the comments on
    /// its pages and the versions of their bodies stay behind. A copy that is not a
    /// template is filed under the category the user has filed the source under, or
    /// else under the default category.
    ///
    /// The titles the copy is given (the original's with `copy` after it, `New board
    /// template`) are in the language the server writes in for the user
    /// (`App::language_for`, by `accept_language`, the request's `Accept-Language`).
    ///
    /// A copy of a built-in template is the user's own board or template: it is made
    /// without the template's marks, and its days are moved by as many days as the day
    /// it is for the user is after the day the template was written for
    /// (`templates::changes_of_copy`).
    ///
    /// # Errors
    ///
    /// Returns a not-found error if there is no such board, a bad request for a wiki
    /// asked to be copied as a template or larger than the limits, an error if its files
    /// cannot be copied, or the store's error.
    pub async fn duplicate_container(
        &self,
        container_id: &str,
        user_id: &str,
        to_workspace: &str,
        duplicate_as: DuplicateAs,
        accept_language: Option<&str>,
    ) -> Result<(ContainersAndBlocks, Vec<ContainerMember>), AppError> {
        let now = crate::auth::now_millis();
        self.duplicate_container_at(
            container_id,
            user_id,
            to_workspace,
            duplicate_as,
            accept_language,
            now,
        )
        .await
    }

    /// [`App::duplicate_container`] at the time `now`, in milliseconds: the day the
    /// days of a built-in template are moved to is the day it is then for the user.
    ///
    /// # Errors
    ///
    /// As [`App::duplicate_container`].
    pub async fn duplicate_container_at(
        &self,
        container_id: &str,
        user_id: &str,
        to_workspace: &str,
        duplicate_as: DuplicateAs,
        accept_language: Option<&str>,
        now: i64,
    ) -> Result<(ContainersAndBlocks, Vec<ContainerMember>), AppError> {
        let reader = self.reader_at(user_id, accept_language, now).await?;
        self.copy_container(
            container_id,
            user_id,
            to_workspace,
            duplicate_as,
            &reader,
            ContainerWork::Duplicate,
        )
        .await
    }

    /// [`App::duplicate_container`] for `reader`, counted among the boards copied whole
    /// as `work`.
    pub(super) async fn copy_container(
        &self,
        container_id: &str,
        user_id: &str,
        to_workspace: &str,
        duplicate_as: DuplicateAs,
        reader: &Reader,
        work: ContainerWork,
    ) -> Result<(ContainersAndBlocks, Vec<ContainerMember>), AppError> {
        let titles = titles_in(reader.language);
        let source = self.container(container_id).await?;
        let changes = changes_of_copy(&source, reader.today.as_deref());
        let kind = source.kind;
        if kind == ContainerKind::Wiki && duplicate_as == DuplicateAs::Template {
            return Err(
                AppError::bad_request(ContainerError::WikiIsTemplate).with_parameter("asTemplate")
            );
        }
        self.write_container_bodies(container_id).await?;
        let to_workspace_named = Some(to_workspace).filter(|workspace| !workspace.is_empty());
        let (copies, members) = match kind {
            ContainerKind::Board => {
                self.store()
                    .duplicate_container(
                        container_id,
                        user_id,
                        to_workspace_named,
                        duplicate_as,
                        &titles,
                        changes,
                    )
                    .await?
            }
            ContainerKind::Wiki => self
                .store()
                .duplicate_wiki(
                    container_id,
                    user_id,
                    to_workspace_named,
                    self.limits().pages,
                    &titles,
                )
                .await
                .map_err(refused_without_member)?,
        };

        let stored = Stored {
            source_id: container_id,
            user_id,
            to_workspace,
            duplicate_as,
            kind,
            work,
        };
        self.finish_copy(stored, copies, members).await
    }

    /// Does for copies that are stored what is done past their transaction: gives
    /// their blocks files of their own, files a copy that is no template under a
    /// category, counts it, and tells the workspace.
    ///
    /// # Errors
    ///
    /// Returns the error of the files, after the copies are deleted again
    /// (`App::take_back`), or of the category, with the copies left as they are.
    pub(super) async fn finish_copy(
        &self,
        stored: Stored<'_>,
        mut copies: ContainersAndBlocks,
        members: Vec<ContainerMember>,
    ) -> Result<(ContainersAndBlocks, Vec<ContainerMember>), AppError> {
        let Stored {
            source_id: container_id,
            user_id,
            to_workspace,
            duplicate_as,
            kind,
            work,
        } = stored;
        // The copied blocks get files of their own.
        if let Err(error) = self
            .copy_and_update_card_files(container_id, user_id, &mut copies.blocks)
            .await
        {
            self.take_back(&copies, container_id, user_id).await;
            return Err(error.context(format_args!(
                "could not patch file IDs while duplicating board {container_id}"
            )));
        }

        if duplicate_as == DuplicateAs::Copy {
            for container in &copies.containers {
                self.set_container_category_from_source(
                    container_id,
                    &container.id,
                    user_id,
                    to_workspace,
                )
                .await?;
            }
        }

        self.metrics().container_copied(work, counted_as(kind));

        let app = self.clone();
        let broadcast = copies.clone();
        let new_members = members.clone();
        self.spawn_background(async move {
            let mut workspace_id = "";
            for container in &broadcast.containers {
                workspace_id = &container.workspace_id;
                app.ws()
                    .broadcast_container_change(workspace_id, container)
                    .await;
            }
            for block in &broadcast.blocks {
                app.ws().broadcast_block_change(workspace_id, block).await;
            }
            for member in &new_members {
                app.ws()
                    .broadcast_member_change(workspace_id, &member.container_id, member)
                    .await;
            }
        });

        Ok((copies, members))
    }

    /// Deletes the copies of `container_id` whose files could not be copied; a failure
    /// to is logged, and the copies stay.
    async fn take_back(&self, copies: &ContainersAndBlocks, container_id: &str, user_id: &str) {
        let delete = DeleteContainersAndBlocks::from(copies);
        if let Err(error) = self
            .store()
            .delete_containers_and_blocks(&delete, user_id)
            .await
        {
            tracing::error!(
                container_id,
                %error,
                "Cannot delete board after duplication error when updating block's file info"
            );
        }
    }

    /// Files a copy of a board under the category its source is filed under for the
    /// user in the workspace, or else under the default category.
    async fn set_container_category_from_source(
        &self,
        source_container_id: &str,
        destination_container_id: &str,
        user_id: &str,
        workspace_id: &str,
    ) -> Result<(), AppError> {
        let destination = [destination_container_id.to_owned()];
        let categories = self.user_category_containers(user_id, workspace_id).await?;
        let source_category = categories.iter().find(|entry| {
            entry
                .container_metadata
                .iter()
                .any(|metadata| metadata.container_id == source_container_id)
        });
        match source_category {
            Some(entry) => {
                self.add_update_user_category_container(
                    workspace_id,
                    user_id,
                    &entry.category.id,
                    &destination,
                )
                .await
            }
            None => {
                self.add_containers_to_default_category(user_id, workspace_id, &destination)
                    .await
            }
        }
    }
}

/// What a board of `kind` is counted as among the boards copied whole.
pub(super) fn counted_as(kind: ContainerKind) -> Module {
    match kind {
        ContainerKind::Board => Module::Board,
        ContainerKind::Wiki => Module::Wiki,
    }
}

/// The titles of a copy in a language the server writes in.
pub(super) fn titles_in(language: Language) -> CopyTitles {
    match language {
        Language::English => CopyTitles {
            copy_of: |title| format!("{title} copy"),
            new_template: "New board template",
        },
        Language::Korean => CopyTitles {
            copy_of: |title| format!("{title} 사본"),
            new_template: "새 보드 템플릿",
        },
    }
}
