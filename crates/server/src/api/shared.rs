//! A shared board or wiki as whoever holds its link reads it: `/shared/{boardId}`,
//! with its read token and no session. A board's link reads its views, cards and
//! comments; a wiki's its tree and pages, and neither the routes of the other. Both
//! read their files.
//!
//! These routes look at nothing of the request but the board, the token and the
//! client's address: a session that comes along is not read, so who is signed in reads
//! a shared board as anybody else with the link does. What they answer names no user:
//! who made or changed the board, a block or a card, the values of person properties,
//! the people of a view's filter, and the mentions of a body are emptied. Nor does it
//! name anything that is not of the board: a reference in a body keeps its identifier
//! for a card of the board, or a page of the wiki, and goes out with an empty one for
//! anything else.
//!
//! A token that opens nothing, a board that is not shared and a board in the trash
//! answer alike, 404. Each such answer, and a read that fails before its token is
//! looked at, counts against the client's address (`App::shared_reads`), so that
//! tokens cannot be guessed at speed.

use axum::extract::rejection::RawPathParamsRejection;
use axum::extract::{FromRequestParts, Path, RawPathParams, RawQuery};
use axum::http::HeaderMap;
use axum::http::request::Parts;
use fvoci_model::{Block, Board, BoardKind, Card, FileInfo, Page, PageNode, PersonProperties};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::blocks::asked_blocks;
use super::cards::cards_page;
use super::contract::bodies::Binary;
use super::contract::params::{
    AllBlocks, BlockTypeFilter, BoardId, FileId, OneBlockId, Page as PageNumber, PageId, ParentId,
    PerPage, Preview, SharedReadToken,
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
        .routes(routes!(handle_get_shared_page_tree))
        .routes(routes!(handle_get_shared_page))
}

/// The board or wiki a request under `/shared/{boardId}` names, opened by the read token
/// of the request: the application behind the wall of its own organization, reaching
/// its kind alone.
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
            app.shared_reads().failed(address);
            return Err(not_shared());
        };
        Ok(Self { app, board_id })
    }
}

/// What a board that is not there for the holder of the link answers.
fn not_shared() -> AppError {
    AppError::not_found("board")
}

impl SharedBoard {
    /// Whether what the link opens is of `kind`: a wiki's link reads no blocks or cards,
    /// a board's no pages, which are as not there (404) as anything not shared. Every
    /// route but the board itself and its files asks it.
    fn require_kind(&self, kind: BoardKind) -> Result<(), AppError> {
        if self.app.kinds().holds(kind) {
            Ok(())
        } else {
            Err(not_shared())
        }
    }

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

/// A shared board or wiki
///
/// The board or the wiki (its `kind` says which) whose read token the request has; no
/// session is needed, and one that comes along is not looked at. Who made it or
/// changed it last is not told (`createdBy` and `modifiedBy` are empty), nor its
/// workspace (`workspaceId` is empty). 404 unless the token is its own and it is shared
/// and not in the trash, and its organization has public links on; 429 for an address
/// that has had many such answers within a minute.
#[utoipa::path(
    get,
    path = "/shared/{boardId}",
    operation_id = "getSharedBoard",
    tag = tag::SHARING,
    params(BoardId, SharedReadToken),
    security(()),
    responses((status = 200, description = "The board or the wiki.", body = Board)),
)]
async fn handle_get_shared_board(shared: SharedBoard) -> ApiResult {
    Ok(json_response(&shared.board().await?))
}

/// The blocks of a shared board
///
/// What `getBlocks` gives of the board, with the same parameters, read with its read
/// token as `getSharedBoard` is; a wiki's token reads no blocks (404). No block names a
/// user: `createdBy` and `modifiedBy` are empty, so is the `userId` of every mention in
/// a body, and so is each user that
/// the board's person properties (`person`, `multiPerson`, `createdBy`, `updatedBy`)
/// name in a card's `fields.properties` and in the clauses of a view's `fields.filter`;
/// a list keeps its length, so how many people a card has still shows. A card's values
/// of properties the board does not define are left out, and a view grouped by a person
/// property keeps of `visibleOptionIds`, `hiddenOptionIds`, `collapsedOptionIds` and
/// `kanbanCalculations` only the group of the cards without a value (`""`). A reference
/// in a body keeps its `id` if it names a card of this board, and has an empty one for
/// anything else: of what is elsewhere in the workspace nothing is told, nor whether
/// there is anything. Oldest
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
    shared.require_kind(BoardKind::Board)?;
    let board = shared.board().await?;
    let mut blocks = asked_blocks(&shared.app, &shared.board_id, query.as_deref()).await?;
    let people = board.person_properties();
    let bodies = blocks.iter().filter_map(|block| block.body.as_ref());
    let shown = shared.app.cards_named_on(&shared.board_id, bodies).await?;
    for block in &mut blocks {
        block.hide_people(&people);
        block.clear_references_but(&shown);
    }
    Ok(json_response(&blocks))
}

/// The cards of a shared board
///
/// What `getCards` gives of the board, read with its read token as `getSharedBoard` is;
/// a wiki's token reads no cards (404).
/// No card names a user: `createdBy` and `modifiedBy` are empty, the values of person
/// properties are emptied, and values of properties the board does not define are left
/// out. The preview's text has no mentions in it, and its image `getSharedFile` gives.
/// A card comes without its body, so with nothing a body names. Oldest first; the
/// pages cut that order.
#[utoipa::path(
    get,
    path = "/shared/{boardId}/cards",
    operation_id = "getSharedCards",
    tag = tag::SHARING,
    params(BoardId, SharedReadToken, PageNumber, PerPage, Preview),
    security(()),
    responses((status = 200, description = "The cards of the page.", body = Vec<Card>)),
)]
async fn handle_get_shared_cards(shared: SharedBoard, RawQuery(query): RawQuery) -> ApiResult {
    shared.require_kind(BoardKind::Board)?;
    let board = shared.board().await?;
    let mut cards = cards_page(&shared.app, &shared.board_id, query.as_deref()).await?;
    let people = board.person_properties();
    for card in &mut cards {
        card.hide_people(&people);
    }
    Ok(json_response(&cards))
}

/// A file of a shared board or wiki
///
/// What `getFile` gives of a file of the board or the wiki, read with its read token as
/// `getSharedBoard` is: a file of another board or wiki is not there.
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

/// What is known about a file of a shared board or wiki
///
/// What `getFileInfo` gives of a file of the board or the wiki, read with its read
/// token as `getSharedBoard` is.
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

/// The pages of a shared wiki
///
/// What `getPageTree` gives of the wiki, read with its read token as `getSharedBoard`
/// is: every page without its body, naming nobody. By `order` as strings compare by
/// code unit, then by `id`. A board's token reads no pages (404).
#[utoipa::path(
    get,
    path = "/shared/{boardId}/pages",
    operation_id = "getSharedPageTree",
    tag = tag::SHARING,
    params(BoardId, SharedReadToken),
    security(()),
    responses((status = 200, description = "The pages.", body = Vec<PageNode>)),
)]
async fn handle_get_shared_page_tree(shared: SharedBoard) -> ApiResult {
    shared.require_kind(BoardKind::Wiki)?;
    Ok(json_response(
        &shared.app.page_tree(&shared.board_id).await?,
    ))
}

/// A page of a shared wiki
///
/// What `getPage` gives of a page of the wiki, read with its read token as
/// `getSharedBoard` is. No page names a user: `createdBy` and `modifiedBy` are empty,
/// and so is the `userId` of every mention in its body. A reference in the body keeps
/// its `id` if it names a page of this wiki, and has an empty one for anything else: of
/// what is elsewhere in the workspace nothing is told, nor whether there is anything.
/// Its comments and its versions are not read by the link. A page of another wiki, one
/// in the trash, and a board's token are 404.
#[utoipa::path(
    get,
    path = "/shared/{boardId}/pages/{pageId}",
    operation_id = "getSharedPage",
    tag = tag::SHARING,
    params(BoardId, PageId, SharedReadToken),
    security(()),
    responses((status = 200, description = "The page.", body = Page)),
)]
async fn handle_get_shared_page(
    shared: SharedBoard,
    Path((_, page_id)): Path<(String, String)>,
) -> ApiResult {
    shared.require_kind(BoardKind::Wiki)?;
    let mut page = shared.app.page_of(&shared.board_id, &page_id).await?;
    let shown = shared
        .app
        .pages_named_on(&shared.board_id, page.body.as_ref())
        .await?;
    // A wiki has no properties of people: the writers and the mentions are what name one.
    page.hide_people(&PersonProperties::default());
    page.clear_references_but(&shown);
    Ok(json_response(&Page::from(page)))
}
