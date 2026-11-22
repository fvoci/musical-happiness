//! Boards: listing, creating, reading, patching, deleting, duplicating and undeleting.

use axum::body::Bytes;
use axum::extract::{Path, RawQuery};
use axum::http::HeaderMap;
use fvoci_model::{
    Container, ContainerKind, ContainerPatch, ContainerType, ContainersAndBlocks, JsonMap,
    MinimumRole,
};
use serde::de::DeserializeOwned;
use serde_json::Value;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::contract::bodies::NewBoard;
use super::contract::params::{
    AcceptLanguage, AsTemplate, BoardId, ContainerId, MemberWorkspaceId, PeopleContainerId,
    ToWorkspace,
};
use super::contract::tag;
use super::workspace::{InBoardOrWiki, InContainer, InWorkspace};

use super::{
    ApiResult, accept_language, created, decode_json, json_response, no_content, query_param,
};
use super::{
    duplicate_as, json_object, require_container_permission, require_granted,
    require_workspace_access,
};
use crate::app::{App, as_managed};
use crate::error::AppError;
use crate::permissions::{ContainerPermission, Grant};

pub(super) fn routes() -> OpenApiRouter<App> {
    OpenApiRouter::new()
        .routes(routes!(handle_create_board))
        .routes(routes!(
            handle_get_container,
            handle_patch_container,
            handle_delete_container
        ))
        .routes(routes!(handle_duplicate_container))
        .routes(routes!(handle_undelete_container))
        .routes(routes!(handle_reorder_card_properties))
}

// -- what the handlers of this area share ----------------------------------------------

/// What a JSON object of a request is, for [`check_closed_members`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BoardObject {
    /// A new board: it has a `type`.
    Board,
    /// A patch of a board: each member may be left out or `null`.
    Patch,
}

/// Refuses a board or a board patch whose `type` or `minimumRole` is outside its closed
/// set, a board whose `kind` is, and a board without a `type`, naming the member below `at` (the JSON
/// Pointer of the object in the body, empty for the body itself). The refusal does not
/// repeat the value: decoding would refuse it too, in words that do.
///
/// # Errors
///
/// Returns a bad request naming the first of `type`, `minimumRole` and `kind` that is
/// refused.
pub(super) fn check_closed_members(
    object: &JsonMap,
    at: &str,
    kind: BoardObject,
) -> Result<(), AppError> {
    /// Whether `value` is there, not `null`, and no `T`.
    fn is_outside<T: DeserializeOwned>(value: Option<&Value>) -> bool {
        value.is_some_and(|value| !value.is_null() && T::deserialize(value).is_err())
    }

    let type_value = object.get("type");
    let no_type = kind == BoardObject::Board && type_value.is_none_or(Value::is_null);
    if no_type || is_outside::<ContainerType>(type_value) {
        return Err(
            AppError::bad_request("the type of a board is `O` (open) or `P` (private)")
                .with_pointer(format!("{at}/type")),
        );
    }
    if is_outside::<MinimumRole>(object.get("minimumRole")) {
        return Err(AppError::bad_request(
            "the minimum role of a board is `viewer`, `commenter` or `editor`",
        )
        .with_pointer(format!("{at}/minimumRole")));
    }
    if kind == BoardObject::Board && is_outside::<ContainerKind>(object.get("kind")) {
        return Err(
            AppError::bad_request("the kind of a board is `board`, `wiki` or `diary`")
                .with_pointer(format!("{at}/kind")),
        );
    }
    Ok(())
}

/// Does [`check_closed_members`] for each object of the list that is the member `list` of
/// `body` (`containers`).
///
/// # Errors
///
/// Returns the refusal of the first object that has one.
pub(super) fn check_listed_boards(
    body: &JsonMap,
    list: &str,
    kind: BoardObject,
) -> Result<(), AppError> {
    let objects = body
        .get(list)
        .and_then(Value::as_array)
        .into_iter()
        .flatten();
    for (index, object) in objects.enumerate() {
        if let Some(object) = object.as_object() {
            check_closed_members(object, &format!("/{list}/{index}"), kind)?;
        }
    }
    Ok(())
}

/// Decodes the body of a board patch. A type or minimum role outside its closed set is
/// refused at the member ([`check_closed_members`]).
fn decode_container_patch(body: &[u8]) -> Result<ContainerPatch, AppError> {
    let patch = json_object(decode_json(body)?)?;
    // A board is of one kind from when it is made: a patch that names one is refused,
    // whatever kind it names, where another member it does not know is left out.
    if patch.contains_key("kind") {
        return Err(AppError::bad_request("the kind of a board never changes").with_field("kind"));
    }
    check_closed_members(&patch, "", BoardObject::Patch)?;
    serde_json::from_value(Value::Object(patch)).map_err(AppError::bad_request)
}

// -- handlers -------------------------------------------------------------------------

/// Make a board
///
/// The user becomes the board's admin, and the board is filed under the user's default
/// category, unless it is a template. A board is made here and no wiki: a `kind` other
/// than `board` is 400 at `/kind` (`createWiki` makes a wiki).
#[utoipa::path(
    post,
    path = "/w/{workspaceId}/boards",
    params(MemberWorkspaceId),
    operation_id = "createBoard",
    tag = tag::CONTAINERS,
    request_body = NewBoard,
    responses((status = 201, description = "The board that was made.", body = Container)),
)]
async fn handle_create_board(ws: InWorkspace, body: Bytes) -> ApiResult {
    let user_id = &ws.session.user_id;
    // Whoever works in the workspace makes boards in it, open ones and private ones.
    ws.require_container_maker("access denied to create board")?;
    let requested = json_object(decode_json(&body)?)?;
    check_closed_members(&requested, "", BoardObject::Board)?;
    let mut board: Container =
        serde_json::from_value(Value::Object(requested)).map_err(AppError::bad_request)?;
    ws.place_container(&mut board, "workspaceId")?;
    board.validate().map_err(AppError::bad_request)?;

    let board = ws.app.create_container_with_admin(board, user_id).await?;
    Ok(created(&board))
}

/// A board
///
/// For a user who may see the board: a member of it, or for an open board anyone in
/// its workspace.
#[utoipa::path(
    get,
    path = "/w/{workspaceId}/containers/{containerId}",
    operation_id = "getContainer",
    tag = tag::CONTAINERS,
    params(MemberWorkspaceId, ContainerId),
    responses((status = 200, description = "The board.", body = Container)),
)]
async fn handle_get_container(
    InContainer(ws): InContainer,
    Path((_, container_id)): Path<(String, String)>,
) -> ApiResult {
    // An open board is for whoever works in its workspace, a private one for its
    // members, and its name for the workspace's administrators.
    let access = ws
        .app
        .container_access(&ws.session.user_id, &container_id)
        .await?
        .filter(|access| access.grant.sees())
        .ok_or_else(|| AppError::not_found("board"))?;
    let container = if access.grant.manages_only() {
        as_managed(access.container)
    } else {
        access.container
    };
    Ok(json_response(&container))
}

/// Change a board
///
/// For who may manage the board's properties. Changing `type` or `minimumRole` is for
/// the board's admins. A wiki is changed here too, by its identifier; it takes no card
/// properties (400 at `/updatedCardProperties`), and nothing changes the `kind` of a board:
/// a patch with a `kind`, even the board's own, is 400 at `/kind`. Other members it does
/// not know are left out.
#[utoipa::path(
    patch,
    path = "/w/{workspaceId}/containers/{containerId}",
    operation_id = "patchContainer",
    tag = tag::CONTAINERS,
    params(MemberWorkspaceId, ContainerId),
    request_body = ContainerPatch,
    responses((status = 200, description = "The board as it is now.", body = Container)),
)]
async fn handle_patch_container(
    InContainer(ws): InContainer,
    Path((_, container_id)): Path<(String, String)>,
    body: Bytes,
) -> ApiResult {
    let user_id = &ws.session.user_id;
    require_container_permission(
        &ws.app,
        user_id,
        &container_id,
        ContainerPermission::ManageProperties,
        "access denied to modifying board properties",
    )
    .await?;
    let patch = decode_container_patch(&body)?;
    if patch.container_type.is_some() || patch.minimum_role.is_some() {
        require_container_permission(
            &ws.app,
            user_id,
            &container_id,
            ContainerPermission::ManageType,
            "access denied to modifying board type",
        )
        .await?;
    }

    // The store names what it refuses as a board has it; the patch has two of those
    // members under other names.
    let container = ws
        .app
        .patch_container(&patch, &container_id, user_id)
        .await
        .map_err(|error| {
            error
                .moved("/properties", "/updatedProperties")
                .moved("/cardProperties", "/updatedCardProperties")
        })?;
    Ok(json_response(&container))
}

/// Put the card properties of a board in an order
///
/// The body names every card property of the board (`cardProperties`, by `id`) once,
/// in the new order, as `reorderCategories` names categories; one left out, one named
/// twice or one the board does not have is 400, and nothing changes. For who may manage
/// the board's properties, as `patchContainer`. The board's listeners get `UPDATE_CONTAINER`, as
/// for any change of the board.
#[utoipa::path(
    put,
    path = "/w/{workspaceId}/boards/{boardId}/property-order",
    operation_id = "reorderCardProperties",
    tag = tag::CONTAINERS,
    params(MemberWorkspaceId, BoardId),
    request_body = Vec<String>,
    responses((
        status = 200,
        description = "The identifiers of the card properties in their new order.",
        body = Vec<String>,
    )),
)]
async fn handle_reorder_card_properties(
    ws: InWorkspace,
    Path((_, board_id)): Path<(String, String)>,
    body: Bytes,
) -> ApiResult {
    let user_id = &ws.session.user_id;
    require_container_permission(
        &ws.app,
        user_id,
        &board_id,
        ContainerPermission::ManageProperties,
        "access denied to modifying board properties",
    )
    .await?;
    let order: Vec<String> = decode_json(&body)?;
    let order = ws
        .app
        .reorder_card_properties(&board_id, &order, user_id)
        .await?;
    Ok(json_response(&order))
}

/// Delete a board
///
/// The board goes to the trash with its blocks and its members, and `undeleteContainer`
/// brings it back until the trash is emptied of it. Team IDs and their roles are kept,
/// but grant no access while the board is in the trash. Its sharing ends, with its read
/// token, and so do its places in the users' sidebars and the subscriptions to its
/// blocks; those do not come back with the board. For the board's admins.
#[utoipa::path(
    delete,
    path = "/w/{workspaceId}/containers/{containerId}",
    operation_id = "deleteContainer",
    tag = tag::CONTAINERS,
    params(MemberWorkspaceId, ContainerId),
    responses((status = 204, description = "The board is deleted.")),
)]
async fn handle_delete_container(
    InContainer(ws): InContainer,
    Path((_, container_id)): Path<(String, String)>,
) -> ApiResult {
    require_container_permission(
        &ws.app,
        &ws.session.user_id,
        &container_id,
        ContainerPermission::Delete,
        "access denied to delete board",
    )
    .await?;
    // The board may be deleted between the two lookups; then it is gone for everybody.
    ws.app.container(&container_id).await?;

    ws.app
        .delete_container(&container_id, &ws.session.user_id)
        .await?;
    Ok(no_content())
}

/// Copy a board or a wiki
///
/// Copies a board with its blocks, except comments, as a board or as a template. The
/// user becomes the admin of the copy, which is always private. A copy of the same kind
/// has the title of the original with ` copy` after it; a template made from a board is
/// titled `New board template`, and a board made from a template keeps the title. The
/// server writes those words in English and in Korean (` 사본`, `새 보드 템플릿`), and
/// chooses by the language of the user's settings; where the settings leave the language
/// to the device, or name one the server does not write in, by the first of the two that
/// `Accept-Language` prefers; failing both, English. The board's members may copy it; an
/// open template is for everyone in its workspace, and a built-in template for everyone.
///
/// A wiki is copied as a board is, with its pages as they stand: their tree and order,
/// titles, icons and bodies, the images and file blocks of the bodies given files of
/// their own, and what a body names among the wiki's pages named in the copy by its
/// copy (what it names elsewhere stays as it was). The comments on the pages, the
/// versions of their bodies, the pages in the trash, the wiki's other members and teams
/// and its public link are not copied. A wiki is no template: `asTemplate=true` is 400.
/// A copy deeper or with more pages than the organization's limits allow, lowered since
/// the wiki grew, is 400 with the reason `page_too_deep` or `too_many_pages` and the
/// `limit`.
#[utoipa::path(
    post,
    path = "/w/{workspaceId}/containers/{containerId}/duplicate",
    operation_id = "duplicateContainer",
    tag = tag::CONTAINERS,
    params(MemberWorkspaceId, PeopleContainerId, AsTemplate, ToWorkspace, AcceptLanguage),
    responses((
        status = 201,
        description = "The copy: one board and its blocks. The blocks are in the order of the original's, oldest first.",
        body = ContainersAndBlocks,
    )),
)]
async fn handle_duplicate_container(
    InBoardOrWiki(ws): InBoardOrWiki,
    Path((_, container_id)): Path<(String, String)>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
) -> ApiResult {
    let user_id = &ws.session.user_id;
    let duplicate_as = duplicate_as(query.as_deref());
    let accept_language = accept_language(&headers);
    let mut to_workspace = query_param(query.as_deref(), "toWorkspace");

    // Whether the board is there for the user comes first: a board that cannot be seen
    // answers as one that does not exist, whatever else the request says.
    // Copying is reading: an open template is for everybody who works in its workspace
    // and for its members, a built-in one for everybody, any other board for its members.
    let access = ws
        .app
        .container_access(user_id, &container_id)
        .await?
        .ok_or_else(|| AppError::not_found("board"))?;
    require_granted(
        access.grant,
        ContainerPermission::View,
        "access denied to board",
    )?;
    if to_workspace.is_empty() {
        // A copy stays in the workspace of the path: the board's, or for a built-in
        // template, which is every workspace's, the one it is copied from.
        to_workspace.clone_from(&ws.workspace_id);
    }
    require_workspace_access(&ws.app, user_id, &to_workspace).await?;

    let (containers_and_blocks, _members) = ws
        .app
        .duplicate_container(
            &container_id,
            user_id,
            &to_workspace,
            duplicate_as,
            accept_language,
        )
        .await?;
    Ok(created(&containers_and_blocks))
}

/// Bring a deleted board back
///
/// Restores a board from the trash with its blocks and its members. Each user who had
/// filed it, and can see it again, finds it where it was in the sidebar (the same
/// category and place, hidden or not) or in the default category if that one was
/// deleted since, and is told with `UPDATE_CONTAINER_CATEGORY` and the category's
/// `REORDER_CATEGORY_CONTAINERS`. Its sharing and the subscriptions to its blocks ended with
/// the deletion and stay ended. Surviving teams regain their saved roles for their
/// current members; deleted teams are skipped. For who was an admin of the board when
/// it was deleted, and for the workspace's administrators (who, if they were no member,
/// get it without its description and properties); to anybody else there is no such board.
#[utoipa::path(
    post,
    path = "/w/{workspaceId}/containers/{containerId}/undelete",
    operation_id = "undeleteContainer",
    tag = tag::CONTAINERS,
    params(MemberWorkspaceId, ContainerId),
    responses((status = 200, description = "The board as it is again.", body = Container)),
)]
async fn handle_undelete_container(
    InContainer(ws): InContainer,
    Path((_, container_id)): Path<(String, String)>,
) -> ApiResult {
    // The trash keeps the memberships a board had, which is how it is known who may
    // bring it back, as what they are in its workspace now. To anybody else, and for a
    // board that is not in the trash, there is no such board.
    let grant = ws
        .app
        .deleted_container_access(&ws.session.user_id, &container_id)
        .await?
        .map_or(Grant::NONE, |access| access.grant);
    require_granted(
        grant,
        ContainerPermission::Delete,
        "access denied to undelete board",
    )?;

    let container = ws
        .app
        .undelete_container(&container_id, &ws.session.user_id)
        .await?;
    if grant.manages_only() {
        return Ok(json_response(&as_managed(container)));
    }
    Ok(json_response(&container))
}
