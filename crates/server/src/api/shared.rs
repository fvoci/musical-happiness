//! A shared board as whoever holds its link reads it: `/shared/{boardId}`, with the
//! board's read token and no session.
//!
//! These routes look at nothing of the request but the board, the token and the
//! client's address: a session that comes along is not read, so who is signed in reads
//! a shared board as anybody else with the link does. What they answer names no user:
//! who made or changed the board, a block or a card, the values of person properties,
//! the people of a view's filter, and the mentions of a body are emptied.
//!
//! A token that opens nothing, a board that is not shared and a board in the trash
//! answer alike, 404. Each such answer, and a read that fails before its token is
//! looked at, counts against the client's address (`App::shared_reads`), so that
//! tokens cannot be guessed at speed.

use axum::extract::rejection::RawPathParamsRejection;
use axum::extract::{FromRequestParts, Path, RawPathParams, RawQuery};
use axum::http::HeaderMap;
use axum::http::request::Parts;
use fvoci_model::{Block, Board, Card, FileInfo};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::blocks::asked_blocks;
use super::cards::cards_page;
use super::contract::bodies::Binary;
use super::contract::params::{
    AllBlocks, BlockTypeFilter, BoardId, FileId, OneBlockId, Page, ParentId, PerPage, Preview,
    SharedReadToken,
};
use super::contract::tag;
use super::files::{board_file_info, serve_board_file};
use super::{ApiResult, json_response, query_param};
use crate::app::App;
use crate::client_address::ClientAddress;
use crate::error::AppError;

/// The path parameter that names the shared board.
const BOARD_ID: &str = "boardId";

/// The query parameter that carries the read token.
const READ_TOKEN: &str = "readToken";

pub(super) fn routes() -> OpenApiRouter<App> {
    OpenApiRouter::new()
        .routes(routes!(handle_get_shared_board))
        .routes(routes!(handle_get_shared_blocks))
        .routes(routes!(handle_get_shared_cards))
        .routes(routes!(handle_get_shared_file))
        .routes(routes!(handle_get_shared_file_info))
}

/// The board a request under `/shared/{boardId}` names, opened by the read token of the
/// request: the application behind the wall of the board's own organization.
///
/// Extraction fails with 404 unless the token opens the board, and with 429 for an
/// address that has had too many such answers of late.
#[derive(Debug)]
pub(crate) struct SharedBoard {
    /// The application, reaching the rows of the board's organization.
    app: App,
    /// The board the path names.
    board_id: String,
}

impl FromRequestParts<App> for SharedBoard {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, app: &App) -> Result<Self, Self::Rejection> {
        let address = parts
            .extensions
            .get::<ClientAddress>()
            .and_then(|address| address.0);
        app.shared_reads().admit(address)?;
        let board_id = match RawPathParams::from_request_parts(parts, app).await {
            Ok(params) => params
                .iter()
                .find(|(name, _)| *name == BOARD_ID)
                .map(|(_, value)| value.to_owned())
                .ok_or_else(|| AppError::internal("the route names no board"))?,
            // A path that is no text names no board that is shared.
            Err(RawPathParamsRejection::InvalidUtf8InPathParam(_)) => String::new(),
            Err(_) => return Err(AppError::internal("the route has no path parameters")),
        };
        let token = query_param(parts.uri.query(), READ_TOKEN);
        let Some(app) = app.enter_shared_board(&board_id, &token).await? else {
            return Err(not_shared());
        };
        app.shared_reads().opened(address);
        Ok(Self { app, board_id })
    }
}

/// What a board that is not there for the holder of the link answers.
fn not_shared() -> AppError {
    AppError::not_found("board")
}

impl SharedBoard {
    /// The board, as the holder of the link reads it: naming nobody, and not its
    /// workspace, which is there only for its members. A board that went between the
    /// token's check and this read is not there either.
    async fn board(&self) -> Result<Board, AppError> {
        let mut board = self.app.board(&self.board_id).await?;
        board.hide_people();
        board.workspace_id.clear();
        Ok(board)
    }
}

/// A shared board
///
/// The board whose read token the request has; no session is needed, and one that
/// comes along is not looked at. Who made the board or changed it last is not told
/// (`createdBy` and `modifiedBy` are empty), nor its workspace (`workspaceId` is
/// empty). 404 unless the token is the board's and the board is shared and not in the
/// trash; 429 for an address that has had many such answers within a minute.
#[utoipa::path(
    get,
    path = "/shared/{boardId}",
    operation_id = "getSharedBoard",
    tag = tag::SHARING,
    params(BoardId, SharedReadToken),
    security(()),
    responses((status = 200, description = "The board.", body = Board)),
)]
async fn handle_get_shared_board(shared: SharedBoard) -> ApiResult {
    Ok(json_response(&shared.board().await?))
}

/// The blocks of a shared board
///
/// What `getBlocks` gives of the board, with the same parameters, read with its read
/// token as `getSharedBoard` is. No block names a user: `createdBy` and `modifiedBy`
/// are empty, so is the `userId` of every mention in a body, and so is each user that
/// the board's person properties (`person`, `multiPerson`, `createdBy`, `updatedBy`)
/// name in a card's `fields.properties` and in the clauses of a view's `fields.filter`;
/// a list keeps its length, so how many people a card has still shows. A card's values
/// of properties the board does not define are left out, and a view grouped by a person
/// property keeps of `visibleOptionIds`, `hiddenOptionIds`, `collapsedOptionIds` and
/// `kanbanCalculations` only the group of the cards without a value (`""`). Oldest
/// first; the blocks that one request made come in the order that request had them in.
#[utoipa::path(
    get,
    path = "/shared/{boardId}/blocks",
    operation_id = "getSharedBlocks",
    tag = tag::SHARING,
    params(BoardId, SharedReadToken, AllBlocks, OneBlockId, ParentId, BlockTypeFilter),
    security(()),
    responses((status = 200, description = "The blocks.", body = Vec<Block>)),
)]
async fn handle_get_shared_blocks(shared: SharedBoard, RawQuery(query): RawQuery) -> ApiResult {
    let board = shared.board().await?;
    let mut blocks = asked_blocks(&shared.app, &shared.board_id, query.as_deref()).await?;
    let people = board.person_properties();
    for block in &mut blocks {
        block.hide_people(&people);
    }
    Ok(json_response(&blocks))
}

/// The cards of a shared board
///
/// What `getCards` gives of the board, read with its read token as `getSharedBoard` is.
/// No card names a user: `createdBy` and `modifiedBy` are empty, the values of person
/// properties are emptied, and values of properties the board does not define are left
/// out. The preview's text has no mentions in it, and its image `getSharedFile` gives.
/// Oldest first; the pages cut that order.
#[utoipa::path(
    get,
    path = "/shared/{boardId}/cards",
    operation_id = "getSharedCards",
    tag = tag::SHARING,
    params(BoardId, SharedReadToken, Page, PerPage, Preview),
    security(()),
    responses((status = 200, description = "The cards of the page.", body = Vec<Card>)),
)]
async fn handle_get_shared_cards(shared: SharedBoard, RawQuery(query): RawQuery) -> ApiResult {
    let board = shared.board().await?;
    let mut cards = cards_page(&shared.app, &shared.board_id, query.as_deref()).await?;
    let people = board.person_properties();
    for card in &mut cards {
        card.hide_people(&people);
    }
    Ok(json_response(&cards))
}

/// A file of a shared board
///
/// What `getFile` gives of a file of the board, read with its read token as
/// `getSharedBoard` is: a file of another board is not there.
#[utoipa::path(
    get,
    path = "/shared/{boardId}/files/{fileId}",
    operation_id = "getSharedFile",
    tag = tag::SHARING,
    params(BoardId, FileId, SharedReadToken),
    security(()),
    responses(
        (status = 200, description = "The file.", body = inline(Binary), content_type = "application/octet-stream"),
        (status = 206, description = "The range of the file that was asked for, as `getFile` answers it.", body = inline(Binary), content_type = "application/octet-stream"),
    ),
)]
async fn handle_get_shared_file(
    shared: SharedBoard,
    Path((_, file_id)): Path<(String, String)>,
    headers: HeaderMap,
) -> ApiResult {
    serve_board_file(&shared.app, &shared.board_id, &file_id, &headers).await
}

/// What is known about a file of a shared board
///
/// What `getFileInfo` gives of a file of the board, read with its read token as
/// `getSharedBoard` is.
#[utoipa::path(
    get,
    path = "/shared/{boardId}/files/{fileId}/info",
    operation_id = "getSharedFileInfo",
    tag = tag::SHARING,
    params(BoardId, FileId, SharedReadToken),
    security(()),
    responses((status = 200, description = "What is known about the file.", body = FileInfo)),
)]
async fn handle_get_shared_file_info(
    shared: SharedBoard,
    Path((_, file_id)): Path<(String, String)>,
) -> ApiResult {
    Ok(json_response(
        &board_file_info(&shared.app, &shared.board_id, &file_id).await?,
    ))
}
