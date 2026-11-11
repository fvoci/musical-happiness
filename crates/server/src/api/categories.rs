//! Sidebar categories and the boards filed under them.

use axum::body::Bytes;
use axum::extract::Path;
use fvoci_model::{Category, CategoryBoards, CategoryType};
use fvoci_store::BoardVisibility;
use serde_json::Value;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::contract::bodies::{CategoryUpdate, NewCategory};
use super::contract::params::{BoardId, CategoryId, WorkspaceId};
use super::contract::tag;

use super::decode_object_members;
use super::workspace::{WorksIn, Ws};
use super::{ApiResult, decode_json, json_response, no_content, require_board_visible};
use crate::app::App;
use crate::auth::now_millis;
use crate::error::AppError;

pub(super) fn routes() -> OpenApiRouter<App> {
    OpenApiRouter::new()
        .routes(routes!(
            handle_create_category,
            handle_get_user_category_boards
        ))
        .routes(routes!(handle_reorder_categories))
        .routes(routes!(handle_update_category, handle_delete_category))
        .routes(routes!(handle_reorder_category_boards))
        .routes(routes!(handle_update_category_board))
        .routes(routes!(handle_hide_board))
        .routes(routes!(handle_unhide_board))
}

/// The path parameters of the routes that address a board in a category: the workspace, the
/// category and the board.
type BoardPlace = (String, String, String);

/// A category as a request body carries it.
struct CategoryRequest {
    category: Category,
    /// Whether the body's `type` is one a category can have. Reported late, see
    /// [`CategoryRequest::into_category`].
    type_is_valid: bool,
}

impl CategoryRequest {
    /// Decodes a category: a member that is `null` or missing leaves the default, and so
    /// does a blank `type`. A body that is no JSON object, `null` included, or whose
    /// members have the wrong types is a bad request.
    fn decode(body: &[u8]) -> Result<Self, AppError> {
        let mut members = decode_object_members(body)?;
        let unknown_type_is_blank = match members.get("type") {
            Some(Value::String(text)) if !is_category_type(text) => Some(text.trim().is_empty()),
            _ => None,
        };
        if unknown_type_is_blank.is_some() {
            members.remove("type");
        }
        let category =
            serde_json::from_value(Value::Object(members)).map_err(AppError::bad_request)?;
        Ok(Self {
            category,
            type_is_valid: unknown_type_is_blank.unwrap_or(true),
        })
    }

    /// The category, if its type is valid. If not, the answer is the bad request the
    /// original gives: it checks the type after the identifier, the name, the user and
    /// the workspace, so a category with one of those blank is refused for that.
    fn into_category(self) -> Result<Category, AppError> {
        let mut category = self.category;
        if self.type_is_valid {
            return Ok(category);
        }
        category.hydrate(now_millis());
        category.validate()?;
        Err(AppError::bad_request(format_args!(
            "category type is invalid. Allowed types: {} and {}",
            CategoryType::System.as_str(),
            CategoryType::Custom.as_str(),
        )))
    }
}

fn is_category_type(text: &str) -> bool {
    [CategoryType::System, CategoryType::Custom]
        .iter()
        .any(|category_type| category_type.as_str() == text)
}

/// Decodes the list of identifiers of a reorder request.
fn decode_order(body: &[u8]) -> Result<Vec<String>, AppError> {
    decode_json(body)
}

/// Make a category
///
/// Makes a category of the user's sidebar in the workspace. The server makes its
/// identifier, and a request that names one is 400. A `userId` that is not the user of
/// the session is 403. The category is stored with its `sortOrder` raised by 10, and
/// every other category of the user in the workspace moves by 10 as well, so that it keeps
/// its place among them.
#[utoipa::path(
    post,
    path = "/w/{workspaceId}/categories",
    operation_id = "createCategory",
    tag = tag::SIDEBAR,
    params(WorkspaceId),
    request_body = NewCategory,
    responses((status = 201, description = "The category that was made.", body = Category)),
)]
async fn handle_create_category(WorksIn(ws): WorksIn, body: Bytes) -> ApiResult {
    // The workspace of the path comes first (the extractor): what is asked of a
    // workspace that is not there is not looked at.
    let Ws {
        app,
        session,
        workspace_id,
        ..
    } = ws;
    let request = CategoryRequest::decode(&body)?;
    let category = &request.category;
    if category.user_id != session.user_id {
        return Err(
            AppError::forbidden("a user makes categories for themselves only").with_field("userId"),
        );
    }
    // The server makes the identifier. One chosen by the client would have to be
    // checked against everybody's categories, and the answer to that check would say
    // whether somebody has a category of that identifier.
    if !category.id.is_empty() {
        return Err(AppError::bad_request("a new category cannot have an ID").with_field("id"));
    }
    if category.workspace_id != workspace_id {
        return Err(AppError::bad_request("workspaceId mismatch").with_field("workspaceId"));
    }

    let created = app.create_category(request.into_category()?).await?;
    Ok(super::created(&created))
}

/// Change a category
///
/// Changes the name of one of the user's categories and whether it is collapsed.
/// Nothing else changes, and a system category keeps its name. A `userId` that is not
/// the user of the session is 400.
#[utoipa::path(
    put,
    path = "/w/{workspaceId}/categories/{categoryId}",
    operation_id = "updateCategory",
    tag = tag::SIDEBAR,
    params(WorkspaceId, CategoryId),
    request_body = CategoryUpdate,
    responses((status = 200, description = "The category as it is now.", body = Category)),
)]
async fn handle_update_category(
    WorksIn(ws): WorksIn,
    Path((_, category_id)): Path<(String, String)>,
    body: Bytes,
) -> ApiResult {
    let Ws {
        app,
        session,
        workspace_id,
        ..
    } = ws;
    // Before the body is looked at: another user's category does not exist for this one.
    app.user_category(&workspace_id, &session.user_id, &category_id)
        .await?;
    let request = CategoryRequest::decode(&body)?;
    let category = &request.category;
    if category.id != category_id {
        return Err(
            AppError::bad_request("the category of the body is not the one of the path")
                .with_field("id"),
        );
    }
    if category.user_id != session.user_id {
        return Err(
            AppError::bad_request("the user of the body is not the session's").with_field("userId"),
        );
    }
    if category.workspace_id != workspace_id {
        return Err(AppError::bad_request("workspaceId mismatch").with_field("workspaceId"));
    }

    let updated = app.update_category(request.into_category()?).await?;
    Ok(json_response(&updated))
}

/// Delete a category
///
/// Deletes one of the user's categories; its boards go to the user's default
/// category. A system category cannot be deleted. A deleted category does not come
/// back.
#[utoipa::path(
    delete,
    path = "/w/{workspaceId}/categories/{categoryId}",
    operation_id = "deleteCategory",
    tag = tag::SIDEBAR,
    params(WorkspaceId, CategoryId),
    responses((status = 204, description = "The category is deleted.")),
)]
async fn handle_delete_category(
    WorksIn(ws): WorksIn,
    Path((_, category_id)): Path<(String, String)>,
) -> ApiResult {
    ws.app
        .delete_category(&category_id, &ws.session.user_id, &ws.workspace_id)
        .await?;
    Ok(no_content())
}

/// The user's sidebar
///
/// The user's categories in the workspace, each with the boards filed under it. A user who
/// has none yet gets the default category made.
///
/// The categories come by `sortOrder`, lowest first, and among those with the same one
/// the oldest first. The boards of a category come in the order the user gave them,
/// and by identifier among those with the same place.
#[utoipa::path(
    get,
    path = "/w/{workspaceId}/categories",
    operation_id = "getCategories",
    tag = tag::SIDEBAR,
    params(WorkspaceId),
    responses((
        status = 200,
        description = "The categories with their boards.",
        body = Vec<CategoryBoards>,
    )),
)]
async fn handle_get_user_category_boards(WorksIn(ws): WorksIn) -> ApiResult {
    let category_boards = ws
        .app
        .user_category_boards(&ws.session.user_id, &ws.workspace_id)
        .await?;
    Ok(json_response(&category_boards))
}

/// File a board under a category
///
/// Puts a board the user may see under one of the user's categories; it leaves the
/// category it was under. A category takes the boards of its own workspace and the
/// built-in templates: a board of another workspace answers as one that is not there.
#[utoipa::path(
    post,
    path = "/w/{workspaceId}/categories/{categoryId}/boards/{boardId}",
    operation_id = "setBoardCategory",
    tag = tag::SIDEBAR,
    params(WorkspaceId, CategoryId, BoardId),
    responses((status = 204, description = "The board is under the category.")),
)]
async fn handle_update_category_board(
    WorksIn(ws): WorksIn,
    Path((_, category_id, board_id)): Path<BoardPlace>,
) -> ApiResult {
    let Ws {
        app,
        session,
        workspace_id,
        ..
    } = ws;
    app.user_category(&workspace_id, &session.user_id, &category_id)
        .await?;
    require_board_visible(&app, &session.user_id, &board_id).await?;
    app.add_update_user_category_board(&workspace_id, &session.user_id, &category_id, &[board_id])
        .await?;
    Ok(no_content())
}

/// Put the categories in an order
///
/// The body lists the identifiers of all of the user's categories in the workspace, in the
/// order they are to have. A list that is not as long as the user has categories, or
/// that names one that is not the user's, is refused.
#[utoipa::path(
    put,
    path = "/w/{workspaceId}/category-order",
    operation_id = "reorderCategories",
    tag = tag::SIDEBAR,
    params(WorkspaceId),
    request_body = Vec<String>,
    responses((
        status = 200,
        description = "The identifiers of the categories in their new order.",
        body = Vec<String>,
    )),
)]
async fn handle_reorder_categories(WorksIn(ws): WorksIn, body: Bytes) -> ApiResult {
    let new_order = decode_order(&body)?;
    let order = ws
        .app
        .reorder_categories(&ws.session.user_id, &ws.workspace_id, new_order)
        .await?;
    Ok(json_response(&order))
}

/// Put the boards of a category in an order
///
/// The body lists the identifiers of all boards of the category, in the order they
/// are to have. A list that is not as long as the category has boards, or that names
/// one that is not in the category, is refused.
#[utoipa::path(
    put,
    path = "/w/{workspaceId}/categories/{categoryId}/board-order",
    operation_id = "reorderCategoryBoards",
    tag = tag::SIDEBAR,
    params(WorkspaceId, CategoryId),
    request_body = Vec<String>,
    responses((
        status = 200,
        description = "The identifiers of the boards in their new order.",
        body = Vec<String>,
    )),
)]
async fn handle_reorder_category_boards(
    WorksIn(ws): WorksIn,
    Path((_, category_id)): Path<(String, String)>,
    body: Bytes,
) -> ApiResult {
    let Ws {
        app,
        session,
        workspace_id,
        ..
    } = ws;
    app.user_category(&workspace_id, &session.user_id, &category_id)
        .await?;
    let new_order = decode_order(&body)?;
    let order = app
        .reorder_category_boards(&session.user_id, &workspace_id, &category_id, new_order)
        .await?;
    Ok(json_response(&order))
}

/// Hide a board in the sidebar
///
/// Hides a board of one of the user's categories from the user's sidebar. The board
/// stays where it is filed, and nothing changes for anybody else. For a board that is
/// not filed under the category nothing changes, and the answer is 204 all the same; a
/// board of another workspace than the category's answers as one that is not there.
#[utoipa::path(
    put,
    path = "/w/{workspaceId}/categories/{categoryId}/boards/{boardId}/hide",
    operation_id = "hideBoard",
    tag = tag::SIDEBAR,
    params(WorkspaceId, CategoryId, BoardId),
    responses((status = 204, description = "The board is hidden.")),
)]
async fn handle_hide_board(WorksIn(ws): WorksIn, Path(place): Path<BoardPlace>) -> ApiResult {
    set_board_visibility(&ws, &place, BoardVisibility::Hidden).await
}

/// Show a hidden board again
///
/// Ends the hiding of a board of one of the user's categories. For a board that is not
/// filed under the category nothing changes, and the answer is 204 all the same; a
/// board of another workspace than the category's answers as one that is not there.
#[utoipa::path(
    put,
    path = "/w/{workspaceId}/categories/{categoryId}/boards/{boardId}/unhide",
    operation_id = "unhideBoard",
    tag = tag::SIDEBAR,
    params(WorkspaceId, CategoryId, BoardId),
    responses((status = 204, description = "The board is shown.")),
)]
async fn handle_unhide_board(WorksIn(ws): WorksIn, Path(place): Path<BoardPlace>) -> ApiResult {
    set_board_visibility(&ws, &place, BoardVisibility::Visible).await
}

async fn set_board_visibility(
    ws: &Ws,
    (_, category_id, board_id): &BoardPlace,
    visibility: BoardVisibility,
) -> ApiResult {
    let (app, user_id, workspace_id) = (&ws.app, ws.session.user_id.as_str(), &ws.workspace_id);
    app.user_category(workspace_id, user_id, category_id)
        .await?;
    require_board_visible(app, user_id, board_id).await?;
    app.set_board_visibility(workspace_id, user_id, category_id, board_id, visibility)
        .await?;
    Ok(no_content())
}
