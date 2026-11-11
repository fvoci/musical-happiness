//! Lists of what is in the trash: the deleted cards of a board, and the deleted boards
//! a user may bring back.

use axum::extract::{Path, RawQuery, State};
use fvoci_model::DeletedItem;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::cards::page_params;
use super::contract::params::{BoardId, Page, PerPage, TeamId};
use super::contract::tag;
use super::require_team_access;
use super::{ApiResult, RequiredSession, json_response, require_board_permission};
use crate::app::App;
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
/// view) comes back with that block, and is not listed. For the board's editors. The last deleted first; the pages
/// cut that order. A `page` or `perPage` that is no whole number is 400.
#[utoipa::path(
    get,
    path = "/boards/{boardId}/trash/cards",
    operation_id = "getDeletedCards",
    tag = tag::CARDS,
    params(BoardId, Page, PerPage),
    responses((status = 200, description = "The deleted cards of the page.", body = Vec<DeletedItem>)),
)]
async fn handle_get_deleted_cards(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    Path(board_id): Path<String>,
    RawQuery(query): RawQuery,
) -> ApiResult {
    let query = query.as_deref();
    require_board_permission(
        &app,
        &session.user_id,
        &board_id,
        BoardPermission::ManageBoardCards,
        "access denied to the trash of the board",
    )
    .await?;
    let (page, per_page) = page_params(query)?;
    Ok(json_response(
        &app.deleted_cards(&board_id, page, per_page).await?,
    ))
}

/// The deleted boards of a team
///
/// One page of the team's boards that are in the trash and that the user was an admin
/// of when they were deleted, which is who `undeleteBoard` lets bring them back: each
/// with its identifier, its title, when it was deleted and by whom. The last deleted
/// first; the pages cut that order. A `page` or `perPage` that is no whole number is
/// 400. For users of the team.
#[utoipa::path(
    get,
    path = "/teams/{teamId}/trash/boards",
    operation_id = "getDeletedBoards",
    tag = tag::BOARDS,
    params(TeamId, Page, PerPage),
    responses((status = 200, description = "The deleted boards of the page.", body = Vec<DeletedItem>)),
)]
async fn handle_get_deleted_boards(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    Path(team_id): Path<String>,
    RawQuery(query): RawQuery,
) -> ApiResult {
    let query = query.as_deref();
    let user_id = session.user_id.as_str();
    require_team_access(&app, user_id, &team_id, "access denied to the team").await?;
    let (page, per_page) = page_params(query)?;
    Ok(json_response(
        &app.deleted_boards(&team_id, user_id, page, per_page)
            .await?,
    ))
}
