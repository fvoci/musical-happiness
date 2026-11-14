//! Wikis: listing and making them, and their pages.
//!
//! A wiki is a board of the kind `wiki`. What is the same for a board and a wiki is
//! asked under `/boards/{boardId}` (`WsModule`); here is what only a wiki has, and the
//! handle these routes work with reaches wikis and no board (`WsWiki`).

use axum::body::Bytes;
use axum::extract::Path;
use fvoci_model::{
    Block, BlockType, Board, BoardKind, BoardType, MinimumRole, NewPage, Page, PageNode, PagePatch,
};
use serde::Deserialize;
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::contract::params::{MemberWorkspaceId, PageId, WikiId};
use super::contract::tag;
use super::workspace::{Ws, WsWiki};
use super::{
    ApiResult, created, decode_object, decode_with_document, json_response, no_content,
    require_board_permission,
};
use crate::app::{App, ChangeNotification};
use crate::error::AppError;
use crate::permissions::BoardPermission;

pub(super) fn routes() -> OpenApiRouter<App> {
    OpenApiRouter::new()
        .routes(routes!(handle_get_wikis, handle_create_wiki))
        .routes(routes!(handle_get_page_tree, handle_create_page))
        .routes(routes!(
            handle_get_page,
            handle_patch_page,
            handle_delete_page
        ))
}

/// A wiki to make.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct NewWiki {
    /// Whether the wiki is open (`O`), for whoever works in the workspace to find and
    /// join, or private (`P`), for whom it is shared with.
    #[serde(rename = "type")]
    board_type: BoardType,
    /// The role a user gets who joins the wiki. Left out, `editor`.
    #[serde(default)]
    minimum_role: MinimumRole,
    /// The wiki's title. Left out, empty.
    #[serde(default)]
    title: String,
    /// The wiki's description. Left out, empty.
    #[serde(default)]
    description: String,
    /// The wiki's icon, usually one emoji. Left out, none.
    #[serde(default)]
    icon: String,
}

/// The wikis of a workspace
///
/// The wikis of the workspace that the user is a member of, and the workspace's open
/// wikis; a guest of the workspace gets only the former, an administrator every wiki
/// of the workspace (the pages of a private wiki they are no member of stay 404 to
/// them). Each is a board of the `kind` `wiki`. Oldest first. 404 unless the user is a
/// member of the workspace.
#[utoipa::path(
    get,
    path = "/w/{workspaceId}/wikis",
    operation_id = "getWikis",
    tag = tag::WIKIS,
    params(MemberWorkspaceId),
    responses((status = 200, description = "The wikis.", body = Vec<Board>)),
)]
async fn handle_get_wikis(WsWiki(ws): WsWiki) -> ApiResult {
    let wikis = ws
        .app
        .workspace_boards(&ws.session.user_id, &ws.workspace_id, ws.role)
        .await?;
    Ok(json_response(&wikis))
}

/// Make a wiki
///
/// The user becomes the wiki's admin, and the wiki is filed under the user's default
/// category, as a board is. Whoever works in the workspace makes wikis in it; a guest
/// is refused (403). A member the request does not know is 400.
#[utoipa::path(
    post,
    path = "/w/{workspaceId}/wikis",
    operation_id = "createWiki",
    tag = tag::WIKIS,
    params(MemberWorkspaceId),
    request_body = NewWiki,
    responses((status = 201, description = "The wiki that was made.", body = Board)),
)]
async fn handle_create_wiki(WsWiki(ws): WsWiki, body: Bytes) -> ApiResult {
    ws.require_board_maker("access denied to create wiki")?;
    let NewWiki {
        board_type,
        minimum_role,
        title,
        description,
        icon,
    } = decode_object(&body)?;
    let wiki = Board {
        workspace_id: ws.workspace_id.clone(),
        board_type,
        kind: BoardKind::Wiki,
        minimum_role,
        title,
        description,
        icon,
        ..Board::default()
    };
    let wiki = ws
        .app
        .create_board_with_admin(wiki, &ws.session.user_id)
        .await?;
    Ok(created(&wiki))
}

/// Returns the page of the wiki for a handler of `/wikis/{wikiId}/pages/{pageId}`, if
/// the user has the permission on the wiki: its block, read once.
///
/// Who may not read the wiki is told what `require_board_permission` tells; a block
/// that is no page of this wiki is no page, whatever else it is.
async fn page_with_permission(
    ws: &Ws,
    wiki_id: &str,
    page_id: &str,
    permission: BoardPermission,
    denial: &str,
) -> Result<Block, AppError> {
    const WHAT: &str = "page";
    require_board_permission(&ws.app, &ws.session.user_id, wiki_id, permission, denial).await?;
    match ws.app.block(page_id).await {
        Ok(block) if block.board_id == wiki_id && block.block_type == BlockType::Page => Ok(block),
        Ok(_) => Err(AppError::not_found(WHAT)),
        Err(error) if error.is_not_found() => Err(AppError::not_found(WHAT)),
        Err(error) => Err(error),
    }
}

/// The pages of a wiki
///
/// Every page of the wiki without its body, for the wiki's members: each names the page
/// it is below (`parentId`, empty at the top of the wiki), which is how they are a
/// tree. By `order` as strings compare by code unit, then by `id`: the pages of one
/// parent are in their order when they are taken out of the list as it is.
#[utoipa::path(
    get,
    path = "/w/{workspaceId}/wikis/{wikiId}/pages",
    operation_id = "getPageTree",
    tag = tag::WIKIS,
    params(MemberWorkspaceId, WikiId),
    responses((status = 200, description = "The pages.", body = Vec<PageNode>)),
)]
async fn handle_get_page_tree(
    WsWiki(ws): WsWiki,
    Path((_, wiki_id)): Path<(String, String)>,
) -> ApiResult {
    require_board_permission(
        &ws.app,
        &ws.session.user_id,
        &wiki_id,
        BoardPermission::ViewBoard,
        "access denied to fetch pages",
    )
    .await?;
    Ok(json_response(&ws.app.page_tree(&wiki_id).await?))
}

/// Make a page
///
/// The server chooses the identifier, the creator, the times and the `order`. For
/// editors of the wiki. The page goes below `parentId`, a page of the same wiki (400 at
/// `/parentId` otherwise), or at the top of the wiki; among the pages of that parent it
/// goes right after the page `after` (400 at `/after` if that is no page of the same
/// parent), first if `after` is empty, and last if it is left out. Its body is written
/// by `patchPage`, or together in its room (`JOIN_DOC`).
///
/// The wiki's members are told with `UPDATE_BLOCK`, whose block of the `type` `page`
/// has the icon and the order among its `fields`.
#[utoipa::path(
    post,
    path = "/w/{workspaceId}/wikis/{wikiId}/pages",
    operation_id = "createPage",
    tag = tag::WIKIS,
    params(MemberWorkspaceId, WikiId),
    request_body = NewPage,
    responses((status = 201, description = "The page that was made.", body = Page)),
)]
async fn handle_create_page(
    WsWiki(ws): WsWiki,
    Path((_, wiki_id)): Path<(String, String)>,
    body: Bytes,
) -> ApiResult {
    let user_id = ws.session.user_id.as_str();
    require_board_permission(
        &ws.app,
        user_id,
        &wiki_id,
        BoardPermission::ManageBoardCards,
        "access denied to create page",
    )
    .await?;
    let new: NewPage = decode_object(&body)?;
    new.check_request()?;
    let page = ws.app.create_page(&wiki_id, &new, user_id).await?;
    tracing::debug!(wiki_id, page_id = %page.id, user_id, "CreatePage");
    Ok(created(&page))
}

/// A page
///
/// The page with its body, for the members of its wiki.
#[utoipa::path(
    get,
    path = "/w/{workspaceId}/wikis/{wikiId}/pages/{pageId}",
    operation_id = "getPage",
    tag = tag::WIKIS,
    params(MemberWorkspaceId, WikiId, PageId),
    responses((status = 200, description = "The page.", body = Page)),
)]
async fn handle_get_page(
    WsWiki(ws): WsWiki,
    Path((_, wiki_id, page_id)): Path<(String, String, String)>,
) -> ApiResult {
    let block = page_with_permission(
        &ws,
        &wiki_id,
        &page_id,
        BoardPermission::ViewBoard,
        "access denied to fetch page",
    )
    .await?;
    Ok(json_response(&Page::from(block)))
}

/// Change a page
///
/// Its title, its icon, or its whole body, for editors of the wiki. A new body is sent
/// with `readAt`, the `updateAt` of the page as it was read: 409 if the page has
/// changed since, and nothing is changed. Where the body is being edited together, what
/// the editors typed is written to the page first, so a writer who read it before is
/// refused and reads it again. Where the page is in the tree does not change here.
#[utoipa::path(
    patch,
    path = "/w/{workspaceId}/wikis/{wikiId}/pages/{pageId}",
    operation_id = "patchPage",
    tag = tag::WIKIS,
    params(MemberWorkspaceId, WikiId, PageId),
    request_body = PagePatch,
    responses((status = 200, description = "The page as it is now.", body = Page)),
)]
async fn handle_patch_page(
    WsWiki(ws): WsWiki,
    Path((_, wiki_id, page_id)): Path<(String, String, String)>,
    body: Bytes,
) -> ApiResult {
    let user_id = ws.session.user_id.as_str();
    let page = page_with_permission(
        &ws,
        &wiki_id,
        &page_id,
        BoardPermission::ManageBoardCards,
        "access denied to patch page",
    )
    .await?;
    let (mut patch, document): (PagePatch, _) = decode_with_document(&body)?;
    patch.body = document;
    patch.check_request()?;
    ws.app
        .check_body_files(&wiki_id, patch.body.new_body())
        .await?;
    let patched = ws
        .app
        .patch_block(
            &page.id,
            &patch.to_block_patch(),
            user_id,
            ChangeNotification::Send,
        )
        .await?;
    tracing::debug!(wiki_id, page_id, user_id, "PatchPage");
    Ok(json_response(&Page::from(patched)))
}

/// Delete a page
///
/// The page goes to the trash with every page below it, for editors of the wiki. The
/// wiki's members are told with `DELETE_BLOCK` of the page; the pages below it went
/// with it.
#[utoipa::path(
    delete,
    path = "/w/{workspaceId}/wikis/{wikiId}/pages/{pageId}",
    operation_id = "deletePage",
    tag = tag::WIKIS,
    params(MemberWorkspaceId, WikiId, PageId),
    responses((status = 204, description = "The page is deleted.")),
)]
async fn handle_delete_page(
    WsWiki(ws): WsWiki,
    Path((_, wiki_id, page_id)): Path<(String, String, String)>,
) -> ApiResult {
    let user_id = ws.session.user_id.as_str();
    let page = page_with_permission(
        &ws,
        &wiki_id,
        &page_id,
        BoardPermission::ManageBoardCards,
        "access denied to delete page",
    )
    .await?;
    ws.app
        .delete_block(&page.id, user_id, ChangeNotification::Send)
        .await?;
    tracing::debug!(wiki_id, page_id, user_id, "DeletePage");
    Ok(no_content())
}
