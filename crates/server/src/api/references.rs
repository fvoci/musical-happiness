//! References: what the bodies of a workspace name.

use axum::body::Bytes;
use fvoci_model::{LinkedBlock, is_id};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::contract::bodies::ReferenceIds;
use super::contract::params::MemberWorkspaceId;
use super::contract::tag;
use super::workspace::WsModule;
use super::{ApiResult, decode_json, json_response};
use crate::app::App;
use crate::error::AppError;

/// The most references one request resolves.
pub(crate) const MAX_REFERENCE_IDS: usize = 100;

pub(super) fn routes() -> OpenApiRouter<App> {
    OpenApiRouter::new().routes(routes!(handle_resolve_references))
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
    if ids.len() > MAX_REFERENCE_IDS {
        return Err(AppError::bad_request(format_args!(
            "at most {MAX_REFERENCE_IDS} references are resolved at once"
        )));
    }
    if let Some(index) = ids.iter().position(|id| !is_id(id)) {
        return Err(
            AppError::bad_request("a reference names a block by its identifier")
                .with_pointer(format!("/{index}")),
        );
    }
    let linked = ws.app.resolve_references(&ws.session.user_id, &ids).await?;
    Ok(json_response(&linked))
}
