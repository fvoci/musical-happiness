//! Copying a board or a wiki whole.

use fvoci_model::{
    ContainerError, ContainerKind, ContainerMember, ContainersAndBlocks, DeleteContainersAndBlocks,
};
use fvoci_store::DuplicateAs;

use super::App;
use super::pages::refused_without_member;
use crate::error::AppError;
use crate::metrics::{ContainerWork, Module};

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
    ) -> Result<(ContainersAndBlocks, Vec<ContainerMember>), AppError> {
        self.copy_container(
            container_id,
            user_id,
            to_workspace,
            duplicate_as,
            ContainerWork::Duplicate,
        )
        .await
    }

    /// [`App::duplicate_container`], counted among the boards copied whole as `work`.
    pub(super) async fn copy_container(
        &self,
        container_id: &str,
        user_id: &str,
        to_workspace: &str,
        duplicate_as: DuplicateAs,
        work: ContainerWork,
    ) -> Result<(ContainersAndBlocks, Vec<ContainerMember>), AppError> {
        let kind = self.container(container_id).await?.kind;
        if kind == ContainerKind::Wiki && duplicate_as == DuplicateAs::Template {
            return Err(
                AppError::bad_request(ContainerError::WikiIsTemplate).with_parameter("asTemplate")
            );
        }
        self.write_container_bodies(container_id).await?;
        let to_workspace_named = Some(to_workspace).filter(|workspace| !workspace.is_empty());
        let (mut copies, members) = match kind {
            ContainerKind::Board => {
                self.store()
                    .duplicate_container(container_id, user_id, to_workspace_named, duplicate_as)
                    .await?
            }
            ContainerKind::Wiki => self
                .store()
                .duplicate_wiki(
                    container_id,
                    user_id,
                    to_workspace_named,
                    self.limits().pages,
                )
                .await
                .map_err(refused_without_member)?,
        };

        // The copied blocks get files of their own.
        if let Err(error) = self
            .copy_and_update_card_files(container_id, user_id, &mut copies.blocks)
            .await
        {
            let delete = DeleteContainersAndBlocks::from(&copies);
            if let Err(delete_error) = self
                .store()
                .delete_containers_and_blocks(&delete, user_id)
                .await
            {
                tracing::error!(
                    container_id,
                    error = %delete_error,
                    "Cannot delete board after duplication error when updating block's file info"
                );
            }
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
