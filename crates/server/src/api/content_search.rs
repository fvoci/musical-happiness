//! Searching what the cards and the pages of a workspace say.

use axum::extract::RawQuery;
use fvoci_model::{SearchHit, SearchKind};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::cards::int_query_param;
use super::contract::params::{ContentKind, ContentLimit, ContentText, MemberWorkspaceId};
use super::contract::tag;
use super::workspace::WsModule;
use super::{ApiResult, json_response, query_param};
use crate::app::App;
use crate::error::AppError;
use crate::search::{Asked, AskedError};

/// How many hits a search answers with unless it asks for another number.
const DEFAULT_LIMIT: i64 = 20;

pub(super) fn routes() -> OpenApiRouter<App> {
    OpenApiRouter::new().routes(routes!(handle_search_content))
}

/// Search what the cards and pages of a workspace say
///
/// The cards of the boards and the pages of the wikis of the workspace that the user
/// has a role on, their own or a team's (an open board or wiki they have not joined is
/// not searched), whose title and body together hold every word of `q`. Korean is
/// matched by its words, whatever particles and endings they carry, and a compound by
/// its parts too: "환불 규정" finds "환불규정을". Comments, property values and attached
/// files are not searched, and neither are templates. Each hit is the card or the page
/// as it is now: its `kind`, its `title`, the board or the wiki it is in (`containerId`,
/// and `containerKind`, by which a client tells a card's address from a page's), and
/// `snippet`, a piece of its body in stretches of plain text, those that matched
/// marked; the snippet is empty for a body that changed a moment ago, or has nothing
/// to show. The best match first. Nothing says how many match in all: fewer than
/// `limit` means that no more of what the user reads matches (for a user with a role
/// on more than 1,000 boards and wikis: among the 10,000 best matches of the
/// workspace).
///
/// A `q` of more than 200 characters, a `kind` that is none and a `limit` outside 1
/// to 50 are 400; a `q` that is empty finds nothing. A user who searches more often
/// than the server lets one is told to wait (429, `Retry-After`). Where the server
/// has no search server (`searchEnabled` of `getClientConfig` is false) the answer is
/// 404 with the reason `search_not_configured`, and while the search server does not
/// answer, 503 with the reason `search_unavailable`: nothing else of the server waits
/// for it.
#[utoipa::path(
    get,
    path = "/w/{workspaceId}/search",
    operation_id = "searchWorkspaceContent",
    tag = tag::WORKSPACES,
    params(MemberWorkspaceId, ContentText, ContentKind, ContentLimit),
    responses((status = 200, description = "What was found.", body = Vec<SearchHit>)),
)]
async fn handle_search_content(WsModule(ws): WsModule, RawQuery(query): RawQuery) -> ApiResult {
    let query = query.as_deref();
    let kind = match query_param(query, "kind").as_str() {
        "" => None,
        word => Some(
            word.parse::<SearchKind>()
                .map_err(|error| AppError::bad_request(error).with_parameter("kind"))?,
        ),
    };
    let limit = int_query_param(query, "limit", DEFAULT_LIMIT)?;
    // A number that is no count of hits is out of range as one too large is.
    let limit = usize::try_from(limit).unwrap_or(0);
    let asked = Asked::new(&query_param(query, "q"), kind, limit).map_err(|error| {
        AppError::bad_request(error).with_parameter(match error {
            AskedError::TextTooLong => "q",
            AskedError::LimitOutOfRange => "limit",
        })
    })?;
    let found = ws
        .app
        .search_workspace(&ws.session.user_id, &ws.workspace_id, &asked)
        .await?;
    tracing::debug!(found = found.len(), "SearchContent");
    Ok(json_response(&found))
}
