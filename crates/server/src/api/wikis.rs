//! Wikis: listing and making them.
//!
//! A wiki is a board of the kind `wiki`. What is the same for a board and a wiki is
//! asked under `/boards/{boardId}` (`WsModule`); here is what only a wiki has, and the
//! handle these routes work with reaches wikis and no board (`WsWiki`).

use axum::body::Bytes;
use fvoci_model::{Board, BoardKind, BoardType, MinimumRole};
use serde::Deserialize;
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::contract::params::MemberWorkspaceId;
use super::contract::tag;
use super::workspace::WsWiki;
use super::{ApiResult, created, decode_object, json_response};
use crate::app::App;

pub(super) fn routes() -> OpenApiRouter<App> {
    OpenApiRouter::new().routes(routes!(handle_get_wikis, handle_create_wiki))
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
