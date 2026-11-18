//! Boards and blocks created, patched and deleted together in one request.

use std::collections::HashSet;

use axum::body::Bytes;
use fvoci_model::{
    Block, ContainersAndBlocks, DeleteContainersAndBlocks, PatchContainersAndBlocks,
    generate_containers_and_blocks_ids,
};

use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::contract::bodies::NewContainersAndBlocks;
use super::contract::params::MemberWorkspaceId;
use super::contract::tag;
use super::workspace::InWorkspace;

use super::containers::{
    InvalidContainerMember, take_invalid_container_members, take_invalid_patch_members,
};
use super::decode_object_with;
use super::{ApiResult, json_response, no_content};
use super::{decode_object, require_container_permission};
use super::{element_pointer, objects_mut, put_patch_bodies, take_block_bodies, take_patch_bodies};

/// The JSON Pointers to the blocks of a `ContainersAndBlocks` and to the block patches of a
/// `PatchContainersAndBlocks`.
const BLOCKS: &str = "/blocks";
const PATCHES: &str = "/blockPatches";
use super::require_delete_permission;
use super::{BlockAction, require_block_patch_permission, require_block_permission};
use crate::app::App;
use crate::error::AppError;
use crate::permissions::ContainerPermission;

/// The refusal of a change to a block of a board the user may not change cards on.
const MODIFYING_CARDS_DENIED: &str = "access denied to modifying cards";

pub(super) fn routes() -> OpenApiRouter<App> {
    OpenApiRouter::new().routes(routes!(
        handle_create_boards_and_blocks,
        handle_patch_boards_and_blocks,
        handle_delete_boards_and_blocks
    ))
}

/// Make boards with their blocks
///
/// Makes boards and their blocks together, all or none, under identifiers the server
/// makes; the identifiers of the request only say which block belongs to which board
/// and to which other block. The request needs at least one board and one block; every
/// board needs an `id`, and all boards are of one workspace. The user becomes the admin of
/// each board, and each board that is no template is filed under the user's default
/// category. They are boards: one whose `kind` is not `board` is 400 at its `kind`.
#[utoipa::path(
    post,
    path = "/w/{workspaceId}/boards-and-blocks",
    params(MemberWorkspaceId),
    operation_id = "createBoardsAndBlocks",
    tag = tag::CONTAINERS,
    request_body = NewContainersAndBlocks,
    responses((
        status = 201,
        description = "The boards in the order of the request, and the blocks board by board in that order, each board's in the order of the request.",
        body = ContainersAndBlocks,
    )),
)]
async fn handle_create_boards_and_blocks(ws: InWorkspace, body: Bytes) -> ApiResult {
    let user_id = &ws.session.user_id;
    // Whoever works in the workspace makes boards in it.
    ws.require_container_maker("access denied to create board")?;
    let (mut request, (invalid, bodies)): (ContainersAndBlocks, _) =
        decode_object_with(&body, |body| {
            let invalid: Option<InvalidContainerMember> = objects_mut(body, "containers")
                .flatten()
                .find_map(take_invalid_container_members);
            (
                invalid,
                take_block_bodies(objects_mut(body, "blocks"), Some(BLOCKS)),
            )
        })?;
    // Not as in the original, which validates neither the type nor the minimum role of
    // these boards and stores whatever it is given. The model has no way to hold such a
    // board, so it is refused as `POST /boards` refuses it.
    if let Some(invalid) = invalid {
        return Err(invalid.into_error());
    }
    // The boards are new and have no files, so a body that names a file is refused, without
    // looking up anything the request names.
    for (index, (block, body)) in request.blocks.iter_mut().zip(bodies?).enumerate() {
        if body
            .as_ref()
            .is_some_and(|body| !body.file_ids().is_empty())
        {
            return Err(AppError::bad_request(
                "an image or a file block of the body names no file of the board: a new board has none",
            )
            .with_field("body")
            .under(&element_pointer(Some(BLOCKS), index)));
        }
        block.body = body;
    }

    if request.containers.is_empty() {
        return Err(AppError::bad_request("at least one board is required"));
    }

    for (index, container) in request.containers.iter_mut().enumerate() {
        ws.place_container(container, "workspaceId")
            .map_err(|error| error.under(&element_pointer(Some("/containers"), index)))?;
        if container.id.is_empty() {
            return Err(AppError::bad_request(
                "boards need an ID to be referenced from the blocks",
            ));
        }
    }
    let container_ids: HashSet<&str> = request
        .containers
        .iter()
        .map(|container| container.id.as_str())
        .collect();

    let problem = request
        .blocks
        .iter()
        .enumerate()
        .find_map(|(index, block)| {
            new_block_problem(block, &container_ids).map(|problem| (index, problem))
        });
    if let Some((index, (field, problem))) = problem {
        return Err(AppError::bad_request(problem)
            .with_field(field)
            .under(&element_pointer(Some(BLOCKS), index)));
    }

    let request = generate_containers_and_blocks_ids(request).map_err(AppError::bad_request)?;
    // The store names a block it refuses by its identifier.
    let ids: Vec<String> = request
        .blocks
        .iter()
        .map(|block| block.id.clone())
        .collect();
    let created = ws
        .app
        .create_containers_and_blocks_with_admin(request, user_id)
        .await
        .map_err(|error| error.among_blocks(ids.iter().map(String::as_str), BLOCKS))?;
    Ok(super::created(&created))
}

/// Returns what is wrong with a block that is to be created with the boards that have
/// the given identifiers, if anything, in the original's order of checking: the member
/// of the block that is wrong, and the message.
fn new_block_problem(
    block: &Block,
    container_ids: &HashSet<&str>,
) -> Option<(&'static str, String)> {
    if block.block_type.is_empty() {
        Some(("type", format!("missing type for block id {}", block.id)))
    } else if !block.block_type.is_current() {
        Some((
            "type",
            fvoci_model::BlockError::type_not_current(&block.block_type).to_string(),
        ))
    } else if block.create_at < 1 {
        Some((
            "createAt",
            format!("invalid createAt for block id {}", block.id),
        ))
    } else if block.update_at < 1 {
        Some((
            "updateAt",
            format!("invalid updateAt for block id {}", block.id),
        ))
    } else if !container_ids.contains(block.container_id.as_str()) {
        Some((
            "containerId",
            format!(
                "invalid containerId {} (no board of the request has that id)",
                block.container_id
            ),
        ))
    } else {
        None
    }
}

/// Change boards and blocks together
///
/// Applies a patch to each of several boards of one workspace and to blocks of those
/// boards, all or none. The request names at least one board, and every block it names
/// is on one of its boards: blocks alone are not changed this way. Each board needs
/// what `patchContainer` needs, each block what `patchBlock` needs; a board's patch that
/// names a `kind` is 400 at `/containerPatches/{index}/kind`.
#[utoipa::path(
    patch,
    path = "/w/{workspaceId}/boards-and-blocks",
    params(MemberWorkspaceId),
    operation_id = "patchBoardsAndBlocks",
    tag = tag::CONTAINERS,
    request_body = PatchContainersAndBlocks,
    responses((
        status = 200,
        description = "The boards and the blocks as they are now, each in the order of the request.",
        body = ContainersAndBlocks,
    )),
)]
async fn handle_patch_boards_and_blocks(ws: InWorkspace, body: Bytes) -> ApiResult {
    let user_id = &ws.session.user_id;
    // The patches' types and minimum roles are validated board by board below, as in the
    // original, not while decoding.
    let (mut patches, (invalid_members, changes)): (PatchContainersAndBlocks, _) =
        decode_object_with(&body, |body| {
            let invalid: Vec<(Option<InvalidContainerMember>, bool)> =
                objects_mut(body, "containerPatches")
                    .map(|patch| match patch {
                        Some(patch) => (
                            take_invalid_patch_members(patch),
                            patch.contains_key("kind"),
                        ),
                        None => (None, false),
                    })
                    .collect();
            (
                invalid,
                take_patch_bodies(objects_mut(body, "blockPatches"), Some(PATCHES)),
            )
        })?;
    patches.validate().map_err(AppError::bad_request)?;
    put_patch_bodies(&mut patches.block_patches, changes?, Some(PATCHES))?;

    let mut workspace_id = String::new();
    let mut container_ids = HashSet::new();
    for (index, (container_id, patch)) in patches
        .container_ids
        .iter()
        .zip(&patches.container_patches)
        .enumerate()
    {
        container_ids.insert(container_id.as_str());

        let (invalid, names_a_kind) = invalid_members.get(index).copied().unwrap_or_default();
        if let Some(invalid) = invalid {
            return Err(invalid.into_error());
        }
        require_container_permission(
            &ws.app,
            user_id,
            container_id,
            ContainerPermission::ManageProperties,
            "access denied to modifying board properties",
        )
        .await?;
        // As `patchContainer`: the kind of a board never changes.
        if names_a_kind {
            return Err(AppError::bad_request("the kind of a board never changes")
                .with_pointer(format!("/containerPatches/{index}/kind")));
        }
        if patch.container_type.is_some() || patch.minimum_role.is_some() {
            require_container_permission(
                &ws.app,
                user_id,
                container_id,
                ContainerPermission::ManageType,
                "access denied to modifying board type",
            )
            .await?;
        }

        let container = ws.app.container(container_id).await?;
        if workspace_id.is_empty() {
            workspace_id.clone_from(&container.workspace_id);
        }
        if workspace_id != container.workspace_id {
            return Err(AppError::bad_request("mismatched workspace ID"));
        }
    }

    require_block_patches(&ws.app, user_id, &patches, &container_ids).await?;
    let block_patches = patches.block_ids.iter().zip(&patches.block_patches);
    for (index, (block_id, patch)) in block_patches.enumerate() {
        if let Some(body) = patch.body.new_body() {
            let block = ws.app.block(block_id).await?;
            ws.app
                .check_body_files(&block.container_id, Some(body))
                .await
                .map_err(|error| error.under(&element_pointer(Some(PATCHES), index)))?;
        }
    }

    let result = ws
        .app
        .patch_containers_and_blocks(&patches, user_id)
        .await
        .map_err(|error| {
            error
                .moved(super::blocks::BLOCK_FIELDS, super::blocks::PATCH_FIELDS)
                .among_blocks(patches.block_ids.iter().map(String::as_str), PATCHES)
        })?;
    Ok(json_response(&result))
}

/// The one answer for a block that does not exist and for one of a board the request
/// does not list.
fn not_a_block_of_the_containers() -> AppError {
    AppError::bad_request("the request names a block that is not on its boards")
}

/// Checks that every block is on one of the boards, and that the user may delete it as
/// `deleteBlock` would. The same answer for a block that does not exist and for one on
/// a board the request does not list, so that nothing is said about blocks of other
/// boards.
async fn require_blocks_to_delete(
    app: &App,
    user_id: &str,
    block_ids: &[String],
    container_ids: &HashSet<&str>,
) -> Result<(), AppError> {
    for block_id in block_ids {
        let block = block_of_containers(app, block_id, container_ids).await?;
        require_delete_permission(app, user_id, &block, MODIFYING_CARDS_DENIED).await?;
    }
    Ok(())
}

/// Checks that every block the request patches is on one of its boards, and that the
/// user may apply its patch to it as `patchBlock` would.
async fn require_block_patches(
    app: &App,
    user_id: &str,
    patches: &PatchContainersAndBlocks,
    container_ids: &HashSet<&str>,
) -> Result<(), AppError> {
    for (block_id, patch) in patches.block_ids.iter().zip(&patches.block_patches) {
        let block = block_of_containers(app, block_id, container_ids).await?;
        require_block_permission(
            app,
            user_id,
            &block,
            BlockAction::Change,
            MODIFYING_CARDS_DENIED,
        )
        .await?;
        require_block_patch_permission(app, user_id, &block, patch, MODIFYING_CARDS_DENIED).await?;
    }
    Ok(())
}

/// The block, if it is on one of the boards; [`not_a_block_of_the_containers`] if it does
/// not exist or is on another board.
async fn block_of_containers(
    app: &App,
    block_id: &str,
    container_ids: &HashSet<&str>,
) -> Result<Block, AppError> {
    match app.block(block_id).await {
        Ok(block) if container_ids.contains(block.container_id.as_str()) => Ok(block),
        Ok(_) => Err(not_a_block_of_the_containers()),
        Err(error) if error.is_not_found() => Err(not_a_block_of_the_containers()),
        Err(error) => Err(error),
    }
}

/// Delete boards and blocks together
///
/// Deletes several boards of one workspace, all or none. The request names at least one
/// board, and every block it names must be on one of its boards and be the user's to
/// delete as `deleteBlock` has it; the blocks are checked and nothing more, since a
/// board takes all of its blocks along. The boards go to the trash as `deleteContainer`
/// puts them there, and `undeleteContainer` brings each back with its blocks. For the
/// admins of every board named.
#[utoipa::path(
    delete,
    path = "/w/{workspaceId}/boards-and-blocks",
    params(MemberWorkspaceId),
    operation_id = "deleteBoardsAndBlocks",
    tag = tag::CONTAINERS,
    request_body = DeleteContainersAndBlocks,
    responses((status = 204, description = "The boards and blocks are deleted.")),
)]
async fn handle_delete_boards_and_blocks(ws: InWorkspace, body: Bytes) -> ApiResult {
    let user_id = &ws.session.user_id;
    let request: DeleteContainersAndBlocks = decode_object(&body)?;
    request.validate().map_err(AppError::bad_request)?;

    let mut workspace_id = String::new();
    let mut container_ids = HashSet::new();
    for container_id in &request.containers {
        container_ids.insert(container_id.as_str());
        require_container_permission(
            &ws.app,
            user_id,
            container_id,
            ContainerPermission::Delete,
            "access denied to delete board",
        )
        .await?;
        let container = ws.app.container(container_id).await?;
        if workspace_id.is_empty() {
            workspace_id.clone_from(&container.workspace_id);
        }
        if workspace_id != container.workspace_id {
            return Err(AppError::bad_request(
                "all boards should belong to the same workspace",
            ));
        }
    }

    require_blocks_to_delete(&ws.app, user_id, &request.blocks, &container_ids).await?;

    ws.app
        .delete_containers_and_blocks(&request, user_id)
        .await?;
    Ok(no_content())
}
