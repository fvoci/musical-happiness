//! Lists of what is in the trash: the deleted cards of a board, and the deleted boards
//! a user may bring back.

use axum::extract::{Path, RawQuery};
use fvoci_model::{BoardKind, DeletedItem};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::cards::page_params;
use super::contract::params::MemberWorkspaceId;
use super::contract::params::{BoardId, Page, PerPage, TrashKind, WorkspaceId};
use super::contract::tag;
use super::workspace::TextWs;
use super::workspace::Ws;
use super::{ApiResult, json_response, query_param, require_board_permission};
use crate::app::App;
use crate::error::AppError;
use crate::permissions::BoardPermission;

pub(super) fn routes() -> OpenApiRouter<App> {
    OpenApiRouter::new()
        .routes(routes!(handle_get_deleted_cards))
        .routes(routes!(handle_get_deleted_boards))
}

/// The deleted cards of a board
///
/// One page of the board's cards that are in the trash, each with its identifier, its
/// title, when it was deleted and by whom; `undeleteBlock` brings one back until the
/// trash is emptied of it. A card that went with the block it was on (another card, a
/// view) comes back with that block, and is not listed. For the board's editors. The
/// last deleted first; the pages cut that order. A `page` or `perPage` that is no whole
/// number is 400.
#[utoipa::path(
    get,
    path = "/w/{workspaceId}/boards/{boardId}/trash/cards",
    operation_id = "getDeletedCards",
    tag = tag::CARDS,
    params(MemberWorkspaceId, BoardId, Page, PerPage),
    responses((status = 200, description = "The deleted cards of the page.", body = Vec<DeletedItem>)),
)]
async fn handle_get_deleted_cards(
    ws: Ws,
    Path((_, board_id)): Path<(String, String)>,
    RawQuery(query): RawQuery,
) -> ApiResult {
    let query = query.as_deref();
    require_board_permission(
        &ws.app,
        &ws.session.user_id,
        &board_id,
        BoardPermission::ManageBoardCards,
        "access denied to the trash of the board",
    )
    .await?;
    let (page, per_page) = page_params(query)?;
    Ok(json_response(
        &ws.app.deleted_cards(&board_id, page, per_page).await?,
    ))
}

/// The deleted boards of a workspace
///
/// One page of the workspace's boards that are in the trash and that the user was an admin
/// of when they were deleted, or every one of them for an administrator of the
/// workspace, which is who `undeleteBoard` lets bring them back: each
/// with its identifier, its title, when it was deleted and by whom. The last deleted
/// first; the pages cut that order. A `page` or `perPage` that is no whole number is
/// 400. For users of the workspace.
///
/// The list is of one kind: the boards, or with `kind=wiki` the wikis, which go to the
/// trash and come back as boards do (`deleteBoard`, `undeleteBoard`).
#[utoipa::path(
    get,
    path = "/w/{workspaceId}/trash/boards",
    operation_id = "getDeletedBoards",
    tag = tag::BOARDS,
    params(WorkspaceId, Page, PerPage, TrashKind),
    responses((status = 200, description = "The deleted boards of the page.", body = Vec<DeletedItem>)),
)]
async fn handle_get_deleted_boards(TextWs(ws): TextWs, RawQuery(query): RawQuery) -> ApiResult {
    let ws = ws.require_no_guest()?;
    let (page, per_page) = page_params(query.as_deref())?;
    let kind = match query_param(query.as_deref(), "kind").as_str() {
        "" => BoardKind::Board,
        text => BoardKind::ALL
            .into_iter()
            .find(|kind| kind.as_str() == text)
            .ok_or_else(|| {
                AppError::bad_request("the kind is `board` or `wiki`").with_parameter("kind")
            })?,
    };
    Ok(json_response(
        &ws.app
            .reaching(kind.into())
            .deleted_boards(
                &ws.workspace_id,
                &ws.session.user_id,
                ws.role,
                page,
                per_page,
            )
            .await?,
    ))
}
