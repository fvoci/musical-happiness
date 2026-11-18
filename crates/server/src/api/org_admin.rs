//! The routes an organization's administrators manage it by, under `/org/{orgId}`: the
//! organization, its members' roles, its workspaces and their trash, joining a
//! workspace as its administrator, and the organization's audit log. Each takes
//! [`OrgAdmin`]: for anybody but an administrator of the organization it is not there
//! (404).

use axum::body::Bytes;
use axum::extract::{Path, RawQuery};
use fvoci_model::{
    AuditRecord, OrgMember, OrgRoleChange, OrgWorkspace, Organization, OrganizationPatch,
    WorkspaceName,
};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::contract::params::{CappedPerPage, OrgId, OrgWorkspaceId, Page, UserId};
use super::contract::tag;
use super::org::OrgAdmin;
use super::paging::paging_params;
use super::{ApiResult, created, decode_object, json_response, no_content};
use crate::app::{App, DEFAULT_PER_PAGE, PageAsked};
use crate::error::AppError;

pub(super) fn routes() -> OpenApiRouter<App> {
    OpenApiRouter::new()
        .routes(routes!(handle_get_organization, handle_patch_organization))
        .routes(routes!(handle_get_org_members))
        .routes(routes!(handle_put_org_role))
        .routes(routes!(
            handle_get_org_workspaces,
            handle_create_org_workspace
        ))
        .routes(routes!(
            handle_rename_org_workspace,
            handle_delete_org_workspace
        ))
        .routes(routes!(handle_restore_org_workspace))
        .routes(routes!(handle_join_org_workspace))
        .routes(routes!(handle_leave_org_workspace))
        .routes(routes!(handle_get_org_audit))
}

/// The page a request asks for with `page` and `perPage`, read as every list reads
/// them ([`paging_params`]).
pub(super) fn requested_page(query: Option<&str>) -> Result<PageAsked, AppError> {
    let (page, per_page) = paging_params(query, DEFAULT_PER_PAGE)?;
    Ok(PageAsked { page, per_page })
}

/// An organization
///
/// Its name, whether its boards may be read by a public link, and its default
/// workspace.
#[utoipa::path(
    get,
    path = "/org/{orgId}",
    operation_id = "getOrganization",
    tag = tag::ORGANIZATION,
    params(OrgId),
    responses((status = 200, description = "The organization.", body = Organization)),
)]
async fn handle_get_organization(org: OrgAdmin) -> ApiResult {
    Ok(json_response(&org.app.organization().await?))
}

/// Change an organization
///
/// Changes its name (at most 100 characters, spaces around it dropped; it may be
/// empty) and whether its boards may be read by a public link. What the body leaves
/// out stays. Turned off, the public links of its boards answer 404 and no board is
/// shared anew (403, `public_links_off`); turned on again, the links made before open
/// again. A change of the public links is recorded in the organization's audit log.
#[utoipa::path(
    patch,
    path = "/org/{orgId}",
    operation_id = "patchOrganization",
    tag = tag::ORGANIZATION,
    params(OrgId),
    request_body = OrganizationPatch,
    responses((status = 200, description = "The organization as it is now.", body = Organization)),
)]
async fn handle_patch_organization(org: OrgAdmin, body: Bytes) -> ApiResult {
    let patch: OrganizationPatch = decode_object(&body)?;
    let organization = org
        .app
        .update_organization(&patch, &org.session.user_id)
        .await?;
    Ok(json_response(&organization))
}

/// The members of an organization
///
/// One page of them with their roles, the oldest member first. 50 to a page unless
/// `perPage` says otherwise, at most 200.
#[utoipa::path(
    get,
    path = "/org/{orgId}/members",
    operation_id = "getOrgMembers",
    tag = tag::ORGANIZATION,
    params(OrgId, Page, CappedPerPage),
    responses((status = 200, description = "The members of the page.", body = Vec<OrgMember>)),
)]
async fn handle_get_org_members(org: OrgAdmin, RawQuery(query): RawQuery) -> ApiResult {
    let paging = requested_page(query.as_deref())?;
    Ok(json_response(&org.app.org_members(paging).await?))
}

/// Change a member's role in an organization
///
/// Makes a member of the organization its administrator, or a member. The last
/// administrator stays one: 409 with the reason `last_org_admin`. Who stops being an
/// administrator is taken out of every workspace they joined as one
/// (`joinOrgWorkspace`): they are what they were in it before, or no member, and each
/// workspace records it. 404 for a user who is no member. Recorded in the
/// organization's audit log.
#[utoipa::path(
    put,
    path = "/org/{orgId}/members/{userId}/role",
    operation_id = "putOrgRole",
    tag = tag::ORGANIZATION,
    params(OrgId, UserId),
    request_body = OrgRoleChange,
    responses((status = 204, description = "The member has the role.")),
)]
async fn handle_put_org_role(
    org: OrgAdmin,
    Path((_, user_id)): Path<(String, String)>,
    body: Bytes,
) -> ApiResult {
    let change: OrgRoleChange = decode_object(&body)?;
    org.app
        .set_org_role(&user_id, change.role, &org.session.user_id)
        .await?;
    Ok(no_content())
}

/// The workspaces of an organization
///
/// Every workspace of it, in use (`deletedAt` 0) and in the trash, the oldest first:
/// its name, how many members it has and what the administrator who asks is in it.
/// Nothing of what is in it: that is read by joining it.
#[utoipa::path(
    get,
    path = "/org/{orgId}/workspaces",
    operation_id = "getOrgWorkspaces",
    tag = tag::ORGANIZATION,
    params(OrgId),
    responses((status = 200, description = "The workspaces.", body = Vec<OrgWorkspace>)),
)]
async fn handle_get_org_workspaces(org: OrgAdmin) -> ApiResult {
    Ok(json_response(
        &org.app.org_workspaces(&org.session.user_id).await?,
    ))
}

/// Make a workspace
///
/// Makes a workspace of the organization with the name: 1 to 100 characters, spaces
/// around it dropped. A name another workspace in use has is 409 with the reason
/// `workspace_name_taken`. Nobody is a member of the new workspace, its maker neither.
/// Recorded in the organization's audit log.
#[utoipa::path(
    post,
    path = "/org/{orgId}/workspaces",
    operation_id = "createOrgWorkspace",
    tag = tag::ORGANIZATION,
    params(OrgId),
    request_body = WorkspaceName,
    responses((status = 201, description = "The workspace.", body = OrgWorkspace)),
)]
async fn handle_create_org_workspace(org: OrgAdmin, body: Bytes) -> ApiResult {
    let request: WorkspaceName = decode_object(&body)?;
    let workspace = org
        .app
        .create_org_workspace(&request.name, &org.session.user_id)
        .await?;
    Ok(created(&workspace))
}

/// Rename a workspace
///
/// Renames a workspace of the organization that is in use, as `createOrgWorkspace`
/// takes a name. Recorded in the organization's audit log.
#[utoipa::path(
    patch,
    path = "/org/{orgId}/workspaces/{workspaceId}",
    operation_id = "renameOrgWorkspace",
    tag = tag::ORGANIZATION,
    params(OrgId, OrgWorkspaceId),
    request_body = WorkspaceName,
    responses((status = 200, description = "The workspace.", body = OrgWorkspace)),
)]
async fn handle_rename_org_workspace(
    org: OrgAdmin,
    Path((_, workspace_id)): Path<(String, String)>,
    body: Bytes,
) -> ApiResult {
    let request: WorkspaceName = decode_object(&body)?;
    let workspace = org
        .app
        .rename_org_workspace(&workspace_id, &request.name, &org.session.user_id)
        .await?;
    Ok(json_response(&workspace))
}

/// Delete a workspace
///
/// Puts a workspace of the organization in the trash: to its members too it is not
/// there (404), with its boards, their files and their public links. Brought back, it is
/// as it was. It is removed for good, with everything in it, when it has been in the
/// trash as long as a deleted board is kept (`trash_retention_days`). The
/// organization's default workspace is not deleted: 409 with the reason
/// `default_workspace`. Recorded in the organization's audit log.
#[utoipa::path(
    delete,
    path = "/org/{orgId}/workspaces/{workspaceId}",
    operation_id = "deleteOrgWorkspace",
    tag = tag::ORGANIZATION,
    params(OrgId, OrgWorkspaceId),
    responses((status = 204, description = "The workspace is in the trash.")),
)]
async fn handle_delete_org_workspace(
    org: OrgAdmin,
    Path((_, workspace_id)): Path<(String, String)>,
) -> ApiResult {
    org.app
        .trash_org_workspace(&workspace_id, &org.session.user_id)
        .await?;
    Ok(no_content())
}

/// Bring a workspace back
///
/// Brings a workspace of the organization back from the trash, with its boards, members,
/// categories and files as they were. 404 unless it is in the trash; 409 with the reason
/// `workspace_name_taken` if a workspace in use took its name meanwhile. Recorded in the
/// organization's audit log.
#[utoipa::path(
    post,
    path = "/org/{orgId}/workspaces/{workspaceId}/restore",
    operation_id = "restoreOrgWorkspace",
    tag = tag::ORGANIZATION,
    params(OrgId, OrgWorkspaceId),
    responses((status = 200, description = "The workspace.", body = OrgWorkspace)),
)]
async fn handle_restore_org_workspace(
    org: OrgAdmin,
    Path((_, workspace_id)): Path<(String, String)>,
) -> ApiResult {
    let workspace = org
        .app
        .restore_org_workspace(&workspace_id, &org.session.user_id)
        .await?;
    Ok(json_response(&workspace))
}

/// Join a workspace as its administrator
///
/// Makes the administrator of the organization who asks an administrator of a
/// workspace of it in use: what is in it is theirs to read from then on, which is
/// recorded in the workspace's audit log, read by its members. A member of it keeps
/// what they were, to be it again when they leave. One who is its administrator already
/// is left as they are, and nothing is recorded.
#[utoipa::path(
    post,
    path = "/org/{orgId}/workspaces/{workspaceId}/join",
    operation_id = "joinOrgWorkspace",
    tag = tag::ORGANIZATION,
    params(OrgId, OrgWorkspaceId),
    responses((status = 200, description = "The workspace as the administrator is in it now.", body = OrgWorkspace)),
)]
async fn handle_join_org_workspace(
    org: OrgAdmin,
    Path((_, workspace_id)): Path<(String, String)>,
) -> ApiResult {
    let workspace = org
        .app
        .join_org_workspace(&workspace_id, &org.session.user_id)
        .await?;
    Ok(json_response(&workspace))
}

/// Leave a workspace joined as its administrator
///
/// Takes the administrator of the organization who asks out of a workspace they joined
/// as one (`joinOrgWorkspace`): they are what they were in it before, or no member, and
/// lose the roles on its boards they gave themselves since they joined. Recorded in the
/// workspace's audit log. One who did not join it so is left as they are.
#[utoipa::path(
    post,
    path = "/org/{orgId}/workspaces/{workspaceId}/leave",
    operation_id = "leaveOrgWorkspace",
    tag = tag::ORGANIZATION,
    params(OrgId, OrgWorkspaceId),
    responses((status = 200, description = "The workspace as the administrator is in it now.", body = OrgWorkspace)),
)]
async fn handle_leave_org_workspace(
    org: OrgAdmin,
    Path((_, workspace_id)): Path<(String, String)>,
) -> ApiResult {
    let workspace = org
        .app
        .leave_org_workspace(&workspace_id, &org.session.user_id)
        .await?;
    Ok(json_response(&workspace))
}

/// The audit log of an organization
///
/// One page of what was done to the organization itself, the newest first: who made a
/// member an administrator or a member, made, renamed, deleted or brought back a
/// workspace, and turned the public links off or on. 50 to a page unless `perPage` says
/// otherwise, at most 200.
#[utoipa::path(
    get,
    path = "/org/{orgId}/audit",
    operation_id = "getOrgAudit",
    tag = tag::ORGANIZATION,
    params(OrgId, Page, CappedPerPage),
    responses((status = 200, description = "The records of the page.", body = Vec<AuditRecord>)),
)]
async fn handle_get_org_audit(org: OrgAdmin, RawQuery(query): RawQuery) -> ApiResult {
    let paging = requested_page(query.as_deref())?;
    Ok(json_response(&org.app.org_audit(paging).await?))
}
