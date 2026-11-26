//! Personal unit progress and course progress summaries.
//!
//! All routes live under `/w/{workspaceId}/courses/{courseId}/...` behind [`InCourse`],
//! enforcing session authentication, workspace membership, SSO policy, personal-space
//! password confirmation, and `ContainerKind::Course` isolation.

use axum::body::Bytes;
use axum::extract::Path;
use fvoci_model::{CourseProgressSummary, UnitProgress, UnitProgressUpdate};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::contract::params::{CourseId, MemberWorkspaceId, UnitId};
use super::contract::tag;
use super::workspace::InCourse;
use super::{ApiResult, decode_object, json_response, require_granted};
use crate::app::{App, ContainerAccess};
use crate::error::AppError;
use crate::permissions::{ContainerPermission, Grant};

pub(super) fn routes() -> OpenApiRouter<App> {
    OpenApiRouter::new()
        .routes(routes!(handle_get_my_course_progress))
        .routes(routes!(handle_put_unit_progress))
        .routes(routes!(handle_get_course_progress_summary))
}

/// Loads course access and requires `View` permission, returning 404 when the course is
/// hidden or does not exist.
pub(super) async fn course_view_access(
    app: &App,
    user_id: &str,
    course_id: &str,
    denial: &str,
) -> Result<ContainerAccess, AppError> {
    let access = app.container_access(user_id, course_id).await?;
    let grant = access.as_ref().map_or(Grant::NONE, |found| found.grant);
    require_granted(grant, ContainerPermission::View, denial)?;
    access.ok_or_else(|| AppError::not_found("course"))
}

/// My progress in a course
///
/// Returns the caller's own personal progress rows for every live unit in the course,
/// ordered by `unitId`. Enrolled course members (including viewers and invited guests)
/// and personal-space owners receive their own rows; an unjoined reader of an open course
/// receives an empty array `[]`. Other users' personal rows are never returned. Non-course
/// containers and unreadable private courses answer 404.
#[utoipa::path(
    get,
    path = "/w/{workspaceId}/courses/{courseId}/progress",
    operation_id = "getMyCourseProgress",
    tag = tag::COURSES,
    params(MemberWorkspaceId, CourseId),
    responses((status = 200, description = "The caller's progress rows for the course.", body = Vec<UnitProgress>)),
)]
async fn handle_get_my_course_progress(
    InCourse(ws): InCourse,
    Path((_, course_id)): Path<(String, String)>,
) -> ApiResult {
    let user_id = ws.session.user_id.as_str();
    let access = course_view_access(
        &ws.app,
        user_id,
        &course_id,
        "access denied to course progress",
    )
    .await?;
    if !access.has_membership() {
        return Ok(json_response(&Vec::<UnitProgress>::new()));
    }
    let rows = ws
        .app
        .my_course_progress(&course_id, user_id, &access.container.properties)
        .await?;
    Ok(json_response(&rows))
}

/// Record my progress on a unit
///
/// Creates or updates the caller's own progress row on a live unit of the course.
/// Enrolled members of any role (viewer, commenter, editor, admin, including invited
/// guests) and personal-space owners may record their own progress; an unjoined reader
/// of an open course is refused with 403 until they join the course.
///
/// Setting `completed: true` is idempotent and preserves an existing `completedAt`
/// timestamp. Setting `completed: false` clears `completedAt` to `null`. Omitting
/// `completed` or `lastPosition` preserves the existing value; sending `lastPosition:
/// null` clears it. Personal progress writes never broadcast over websockets, never
/// enter search indexes or archives, and emit no webhooks or audit logs. Chapters,
/// trashed units, units of another course, and non-course containers answer 404.
#[utoipa::path(
    put,
    path = "/w/{workspaceId}/courses/{courseId}/units/{unitId}/progress",
    operation_id = "putUnitProgress",
    tag = tag::COURSES,
    params(MemberWorkspaceId, CourseId, UnitId),
    request_body = UnitProgressUpdate,
    responses((status = 200, description = "The updated personal progress row.", body = UnitProgress)),
)]
async fn handle_put_unit_progress(
    InCourse(ws): InCourse,
    Path((_, course_id, unit_id)): Path<(String, String, String)>,
    body: Bytes,
) -> ApiResult {
    let user_id = ws.session.user_id.as_str();
    let access = course_view_access(
        &ws.app,
        user_id,
        &course_id,
        "access denied to update unit progress",
    )
    .await?;
    if !access.has_membership() {
        return Err(AppError::forbidden(
            "join the course before recording progress",
        ));
    }
    let update: UnitProgressUpdate = decode_object(&body)?;
    let change = update.check_request()?;
    let progress = ws
        .app
        .put_unit_progress(
            &course_id,
            &unit_id,
            user_id,
            &change,
            &access.container.properties,
        )
        .await?;
    Ok(json_response(&progress))
}

/// Progress summary of a course
///
/// Returns the total number of live units in the course (excluding chapters and trashed
/// units) and, for enrolled course members and course role managers (`ManageRoles`), the
/// completed live-unit count for each active course member ordered by `userId`. Unjoined
/// readers of an open course and non-enrolled guests receive `members: []`.
/// Accounts that have been deleted, are pending deletion, or are no longer active members
/// of the workspace or course are excluded. Never discloses which units a member
/// completed, completion timestamps, planned dates, or last positions. Unreadable private
/// courses and non-course containers answer 404.
#[utoipa::path(
    get,
    path = "/w/{workspaceId}/courses/{courseId}/progress/summary",
    operation_id = "getCourseProgressSummary",
    tag = tag::COURSES,
    params(MemberWorkspaceId, CourseId),
    responses((status = 200, description = "Aggregate completion counts for active course members.", body = CourseProgressSummary)),
)]
async fn handle_get_course_progress_summary(
    InCourse(ws): InCourse,
    Path((_, course_id)): Path<(String, String)>,
) -> ApiResult {
    let access = course_view_access(
        &ws.app,
        &ws.session.user_id,
        &course_id,
        "access denied to course progress summary",
    )
    .await?;
    let mut summary = ws.app.course_progress_summary(&course_id).await?;
    if !access.has_membership() && !access.grant.allows(ContainerPermission::ManageRoles) {
        summary.members.clear();
    }
    Ok(json_response(&summary))
}
