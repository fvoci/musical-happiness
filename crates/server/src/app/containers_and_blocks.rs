//! Boards and blocks created and deleted together.

use std::collections::HashMap;

use fvoci_model::{ContainerMember, ContainersAndBlocks, DeleteContainersAndBlocks};

use super::{App, Changes, Former};
use crate::collab::{AccessScope, Deletion};
use crate::error::AppError;
use crate::permissions::ContainerScope;

/// Who read a board the request did not list: nobody (the store refused it).
const NOBODY: Former<'static> = Former {
    members: &[],
    scope: ContainerScope::InvitedOnly,
};

impl App {
    /// Stores boards and their blocks, identifiers included, tells the boards' members
    /// and the webhooks, makes `user_id` an admin and editor of each board, and files each
    /// board that is not a template under the default category of `user_id`. Each image
    /// and file block of a body names a file of its board
    /// ([`fvoci_store::NewBodyFiles::Recorded`]).
    ///
    /// # Errors
    ///
    /// Returns a bad request if a block is too large, the store's error if nothing could
    /// be stored, or an error if a board cannot be filed under the user's default
    /// category; in the last case the boards and blocks exist nonetheless.
    pub async fn create_containers_and_blocks_with_admin(
        &self,
        mut created: ContainersAndBlocks,
        user_id: &str,
    ) -> Result<ContainersAndBlocks, AppError> {
        for (index, container) in created.containers.iter().enumerate() {
            self.require_makes(container)
                .map_err(|error| error.under(&format!("/containers/{index}")))?;
        }
        let members = {
            let _structure = self
                .collab()
                .changing_structures(self, created.containers.iter().map(|c| c.id.as_str()))
                .await;
            self.store()
                .create_containers_and_blocks_with_admin(&mut created, user_id)
                .await?
        };

        if !created.containers.is_empty() {
            let made = created.containers.iter().map(|container| &container.id);
            self.collab()
                .recheck_access(self, AccessScope::containers(made))
                .await;
        }
        self.announce_created(&created, &members).await;

        for container in &created.containers {
            if !container.is_template {
                self.add_containers_to_default_category(
                    user_id,
                    &container.workspace_id,
                    std::slice::from_ref(&container.id),
                )
                .await?;
            }
        }
        Ok(created)
    }

    /// Tells the boards' members and the webhooks about boards and blocks that were
    /// stored together, and about the memberships made with them, before answering.
    pub(crate) async fn announce_created(
        &self,
        created: &ContainersAndBlocks,
        members: &[ContainerMember],
    ) {
        // All new boards belong to one workspace; if none were created there is nobody to tell.
        let workspace_id = created
            .containers
            .first()
            .map_or("", |container| container.workspace_id.as_str());
        let changes = Changes::in_workspace(workspace_id)
            .made(created.containers.clone(), members.to_vec())
            .changed(created.blocks.iter().cloned());
        self.tell_now(changes).await;
    }

    /// Deletes blocks and boards, all or none, and tells the boards' members.
    ///
    /// # Errors
    ///
    /// Returns a bad request if no board is listed, a not-found error if a board or
    /// block does not exist, or the store's error.
    pub async fn delete_containers_and_blocks(
        &self,
        request: &DeleteContainersAndBlocks,
        user_id: &str,
    ) -> Result<(), AppError> {
        let Some(first_container_id) = request.containers.first() else {
            return Err(AppError::bad_request(
                fvoci_model::ContainersAndBlocksError::NoContainers,
            ));
        };
        let workspace_id = self
            .store()
            .get_container(first_container_id)
            .await?
            .workspace_id;

        // The memberships go with the boards, so who is told is who was a member; and
        // the blocks are gone afterwards, so which board each was on is read first.
        let mut readers_before = HashMap::with_capacity(request.containers.len());
        for container_id in &request.containers {
            let members = self
                .store()
                .effective_container_members(container_id)
                .await?;
            let scope = ContainerScope::of(&self.store().get_container(container_id).await?);
            readers_before.insert(container_id.as_str(), (members, scope));
        }
        let mut deleted_blocks = Vec::with_capacity(request.blocks.len());
        for block_id in &request.blocks {
            let block = self.store().get_block(block_id).await?;
            deleted_blocks.push((block.id, block.container_id, block.block_type));
        }

        let (removed, by) = (request.clone(), user_id.to_owned());
        self.collab()
            .remove(
                self,
                Deletion::Containers(removed.containers.clone()),
                async move |app| {
                    Ok(app
                        .store()
                        .delete_containers_and_blocks(&removed, &by)
                        .await?)
                },
            )
            .await?;

        if !request.containers.is_empty() {
            self.collab()
                .recheck_access(self, AccessScope::containers(&request.containers))
                .await;
        }
        let former = |container_id: &str| {
            readers_before
                .get(container_id)
                .map_or(NOBODY, |(members, scope)| Former {
                    members,
                    scope: *scope,
                })
        };
        for (block_id, container_id, block_type) in &deleted_blocks {
            // A block of a board the request does not list was refused by the store.
            self.ws()
                .broadcast_block_delete_with_container(
                    &workspace_id,
                    block_id,
                    container_id,
                    *block_type,
                    former(container_id),
                )
                .await;
        }
        for container_id in &request.containers {
            self.ws()
                .broadcast_container_delete(&workspace_id, container_id, former(container_id))
                .await;
            // The board's sharing went with it; so does whoever followed it by its link,
            // as with `deleteContainer`.
            self.ws().end_readers_of_container(container_id);
        }
        Ok(())
    }
}
