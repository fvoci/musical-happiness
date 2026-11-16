//! Searching boards.

use axum::extract::RawQuery;
use fvoci_model::{Container, ContainerSearchField};
use fvoci_store::PublicContainers;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::contract::params::{SearchField, SearchTerm, WorkspaceId};
use super::contract::tag;

use super::workspace::InWorkspace;
use super::{ApiResult, json_response, query_param};
use crate::app::App;
use crate::error::AppError;

pub(super) fn routes() -> OpenApiRouter<App> {
    OpenApiRouter::new().routes(routes!(handle_search_containers))
}

/// Search the boards of a workspace
///
/// The boards of the workspace the user finds (a member of them, or open where the user
/// works), whose title matches the term: a title matches if every word of the term, as
/// spaces divide it, is somewhere in it, whatever the case of the letters (of every
/// letter, not only ASCII); `%` and `_` are characters like any other. With
/// `field=property_name` those whose own `properties` have a member of exactly that
/// name, whatever its value (`null` too). A `field` that is neither is 400. Templates are
/// never found, and an empty term finds nothing. Oldest first.
#[utoipa::path(
    get,
    path = "/w/{workspaceId}/boards/search",
    operation_id = "searchWorkspaceBoards",
    tag = tag::CONTAINERS,
    params(WorkspaceId, SearchTerm, SearchField),
    responses((status = 200, description = "The boards that match.", body = Vec<Container>)),
)]
async fn handle_search_containers(ws: InWorkspace, RawQuery(query): RawQuery) -> ApiResult {
    let ws = ws.require_no_guest()?;
    let term = query_param(query.as_deref(), "q");
    let field_text = query_param(query.as_deref(), "field");
    let search_field = if field_text.is_empty() {
        ContainerSearchField::Title
    } else {
        field_text
            .parse::<ContainerSearchField>()
            .map_err(AppError::bad_request)?
    };
    let workspace_id = &ws.workspace_id;

    let mut containers =
        search_containers(&ws.app, &ws.session.user_id, &term, search_field).await?;
    containers.retain(|container| container.workspace_id == *workspace_id);
    tracing::debug!(
        workspace_id,
        containers_count = containers.len(),
        "SearchBoards"
    );
    Ok(json_response(&containers))
}

/// Searches the boards the user may find. An empty term finds nothing.
async fn search_containers(
    app: &App,
    user_id: &str,
    term: &str,
    search_field: ContainerSearchField,
) -> Result<Vec<Container>, AppError> {
    if term.is_empty() {
        return Ok(Vec::new());
    }
    app.search_containers_for_user(term, search_field, user_id, PublicContainers::Include)
        .await
}
