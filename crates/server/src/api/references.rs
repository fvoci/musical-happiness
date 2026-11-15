//! References: what the bodies of a workspace name.

use axum::body::Bytes;
use axum::extract::{Path, RawQuery};
use fvoci_model::{LinkedBlock, is_id};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::contract::bodies::ReferenceIds;
use super::contract::params::{BlockId, CappedPerPage, MemberWorkspaceId, Page, TitleTerm};
use super::contract::tag;
use super::org_admin::requested_page;
use super::workspace::WsModule;
use super::{ApiResult, decode_json, json_response, query_param};
use crate::app::App;
use crate::error::AppError;

pub(super) fn routes() -> OpenApiRouter<App> {
    OpenApiRouter::new()
        .routes(routes!(handle_resolve_references))
        .routes(routes!(handle_get_references_to))
        .routes(routes!(handle_search_references))
}

/// What references name
///
/// A body names a card or a page by its identifier alone (the node `reference`), and no
/// body, however it is read, says what that is called: this does, for the user who
/// asks and as things are now. Of the identifiers of the body, the cards and pages of
/// this workspace on a board or a wiki the user reads (as `getCard` and `getPage` ask),
/// each once, in the order they were first named.
///
/// Whatever else an identifier names is left out, and the answer does not tell why:
/// nothing at all, something in the trash, something of another workspace or
/// organization, a card or a page the user does not read, or a block that is neither.
/// The body names at most 100 identifiers (more is 400, as an entry that has not the
/// form of an identifier is); an empty list answers an empty list.
#[utoipa::path(
    post,
    path = "/w/{workspaceId}/references/resolve",
    operation_id = "resolveReferences",
    tag = tag::REFERENCES,
    params(MemberWorkspaceId),
    request_body = ReferenceIds,
    responses((status = 200, description = "What the user is shown of them.", body = Vec<LinkedBlock>)),
)]
async fn handle_resolve_references(WsModule(ws): WsModule, body: Bytes) -> ApiResult {
    let ids: Vec<String> = decode_json(&body)?;
    let linked = ws.app.resolve_references(&ws.session.user_id, &ids).await?;
    Ok(json_response(&linked))
}

/// What names a card or a page
///
/// The cards and pages of this workspace whose body names the block of the path with a
/// reference, or which have a comment whose body does, on a board or a wiki the user
/// reads; the block itself is never among them. Oldest first, 50 to a page unless
/// `perPage` says otherwise, at most 200. What names it from where the user does not
/// read is left out and takes no place in a page: the answer does not say how many
/// there are, nor whether there are any.
///
/// 404 unless the block is a card or a page the user reads, as `resolveReferences`
/// tells of it: for anything else, as for an identifier of nothing.
#[utoipa::path(
    get,
    path = "/w/{workspaceId}/references/to/{blockId}",
    operation_id = "getReferencesTo",
    tag = tag::REFERENCES,
    params(MemberWorkspaceId, BlockId, Page, CappedPerPage),
    responses((status = 200, description = "What names it, of the page.", body = Vec<LinkedBlock>)),
)]
async fn handle_get_references_to(
    WsModule(ws): WsModule,
    Path((_, block_id)): Path<(String, String)>,
    RawQuery(query): RawQuery,
) -> ApiResult {
    let asked = requested_page(query.as_deref())?;
    // What has not the form of an identifier names nothing, and is answered so.
    let naming = if is_id(&block_id) {
        ws.app
            .references_to(&ws.session.user_id, &ws.workspace_id, &block_id, asked)
            .await?
    } else {
        None
    };
    let naming = naming.ok_or_else(|| AppError::not_found("block"))?;
    Ok(json_response(&naming))
}

/// Find what a reference may name
///
/// The cards and pages of this workspace, on a board or a wiki the user reads, whose
/// title has every word of `q` in it, as spaces divide it and whatever the case of the
/// letters (of every letter, not only ASCII); `%` and `_` are characters like any
/// other. At most 20, the one changed last first; without `q`, the 20 changed last.
/// Bodies are not looked into. What is in the trash, what is on a template, and a card
/// that is a template of its board's cards are never found. A `q` of more than 200
/// characters is 400.
#[utoipa::path(
    get,
    path = "/w/{workspaceId}/references/search",
    operation_id = "searchReferences",
    tag = tag::REFERENCES,
    params(MemberWorkspaceId, TitleTerm),
    responses((status = 200, description = "What was found.", body = Vec<LinkedBlock>)),
)]
async fn handle_search_references(WsModule(ws): WsModule, RawQuery(query): RawQuery) -> ApiResult {
    let term = query_param(query.as_deref(), "q");
    let found = ws
        .app
        .search_references(&ws.session.user_id, &ws.workspace_id, &term)
        .await?;
    Ok(json_response(&found))
}
