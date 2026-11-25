//! The comments on a unit of a course: listing, writing, changing, deleting and
//! restoring them, and what one said before.

use axum::body::Bytes;
use axum::extract::Path;
use fvoci_model::{Block, Body, BodyChange, CommentVersion};
use serde::Deserialize;
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::comments::{
    create_comment_on, created_comment, delete_comment_on, existing_comment_on,
    find_live_comment_on, get_comment_versions_on, list_comments_on, patch_comment_on,
    restore_comment_on,
};
use super::contract::params::{CourseId, MemberWorkspaceId, UnitCommentId, UnitId};
use super::contract::tag;
use super::courses::{READ_DENIED, content_unit_with_permission};
use super::workspace::InCourse;
use super::{ApiResult, json_response};
use crate::app::App;
use crate::permissions::ContainerPermission;

pub(super) fn routes() -> OpenApiRouter<App> {
    OpenApiRouter::new()
        .routes(routes!(
            handle_get_unit_comments,
            handle_create_unit_comment
        ))
        .routes(routes!(
            handle_patch_unit_comment,
            handle_delete_unit_comment
        ))
        .routes(routes!(handle_get_unit_comment_versions))
        .routes(routes!(handle_restore_unit_comment))
}

const CHANGES_DENIED: &str = "access denied to change the comments of the unit";

/// A comment to write on a unit.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct NewUnitComment {
    /// The comment's text, as a unit's body is written; neither empty nor `null`.
    #[serde(default)]
    #[schema(value_type = Body)]
    #[allow(dead_code)]
    body: BodyChange,
}

/// What a comment on a unit is changed to: its text, written over the comment as it was
/// read.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct UnitCommentPatch {
    /// The comment's new text, as a unit's body is written; neither empty nor `null`.
    #[serde(default)]
    #[schema(value_type = Body)]
    #[allow(dead_code)]
    body: BodyChange,
    /// The `updatedAt` of the comment as the client read it: if the comment has changed
    /// since, the patch is refused with 409 and changes nothing.
    #[schema(value_type = i64)]
    read_at: Option<i64>,
}

/// The comments on a unit
///
/// Every comment on the unit, oldest first, as blocks of the `type` `comment` whose
/// parent is the unit; each says who wrote it (`createdBy`), and one whose text was
/// changed when that last happened (`editedAt`). For who reads the course: its members,
/// and of an open course whoever works in the workspace. A chapter or a unit in the
/// trash is not there (404).
#[utoipa::path(
    get,
    path = "/w/{workspaceId}/courses/{courseId}/units/{unitId}/comments",
    operation_id = "getUnitComments",
    tag = tag::COURSES,
    params(MemberWorkspaceId, CourseId, UnitId),
    responses((status = 200, description = "The comments, oldest first.", body = Vec<Block>)),
)]
async fn handle_get_unit_comments(
    InCourse(ws): InCourse,
    Path((_, course_id, unit_id)): Path<(String, String, String)>,
) -> ApiResult {
    let unit = content_unit_with_permission(
        &ws,
        &course_id,
        &unit_id,
        ContainerPermission::View,
        READ_DENIED,
    )
    .await?;
    list_comments_on(&ws, &course_id, &unit.id).await
}

/// Write a comment on a unit
///
/// For the course's members from the commenter's role up (whoever reads an open course
/// without joining it is refused with 403). The server chooses the identifier, the
/// author and the times. Every image of the text shows a file of the course (400
/// otherwise), and the text has no file block (400). A chapter takes no comments (404).
/// Who reads the course is told with `blockChanged`, as of a comment on a card or page.
#[utoipa::path(
    post,
    path = "/w/{workspaceId}/courses/{courseId}/units/{unitId}/comments",
    operation_id = "createUnitComment",
    tag = tag::COURSES,
    params(MemberWorkspaceId, CourseId, UnitId),
    request_body = NewUnitComment,
    responses((status = 201, description = "The comment as it was stored.", body = Block)),
)]
async fn handle_create_unit_comment(
    InCourse(ws): InCourse,
    Path((_, course_id, unit_id)): Path<(String, String, String)>,
    body: Bytes,
) -> ApiResult {
    let user_id = ws.session.user_id.as_str();
    let unit = content_unit_with_permission(
        &ws,
        &course_id,
        &unit_id,
        ContainerPermission::CommentCards,
        "access denied to comment on the unit",
    )
    .await?;
    let comment = create_comment_on::<NewUnitComment>(&ws, &course_id, unit, "unit", &body).await?;
    tracing::debug!(course_id, unit_id, comment_id = %comment.id, user_id, "CreateUnitComment");
    created_comment(&comment)
}

/// Change a comment on a unit
///
/// Writes a new text over the comment, as a card's or page's comments are changed: by
/// its author from the commenter's role up, and the comment of somebody else by the
/// course's admins only (403, `access denied to change other users' comments`); an
/// editor may not change it. The text it had is kept (`getUnitCommentVersions`), and the
/// comment's `editedAt` says when it last changed. The text has no file block (400).
#[utoipa::path(
    patch,
    path = "/w/{workspaceId}/courses/{courseId}/units/{unitId}/comments/{commentId}",
    operation_id = "patchUnitComment",
    tag = tag::COURSES,
    params(MemberWorkspaceId, CourseId, UnitId, UnitCommentId),
    request_body = UnitCommentPatch,
    responses((status = 200, description = "The comment as it is now.", body = Block)),
)]
async fn handle_patch_unit_comment(
    InCourse(ws): InCourse,
    Path((_, course_id, unit_id, comment_id)): Path<(String, String, String, String)>,
    body: Bytes,
) -> ApiResult {
    let user_id = ws.session.user_id.as_str();
    let unit = content_unit_with_permission(
        &ws,
        &course_id,
        &unit_id,
        ContainerPermission::CommentCards,
        CHANGES_DENIED,
    )
    .await?;
    let comment = existing_comment_on(&ws, &unit, &comment_id).await?;
    let patched = patch_comment_on::<UnitCommentPatch>(
        &ws,
        &course_id,
        &comment,
        CHANGES_DENIED,
        &body,
        |patch| patch.read_at,
    )
    .await?;
    tracing::debug!(course_id, unit_id, comment_id, user_id, "PatchUnitComment");
    Ok(json_response(&patched))
}

/// Delete a comment on a unit
///
/// The comment goes to the trash, as a comment on a card or page does: by its author
/// from the commenter's role up, and the comment of somebody else by the course's admins
/// only (403, `access denied to delete other users' comments`). Who reads the course is
/// told with `blockChanged` with `deletedAt` set.
#[utoipa::path(
    delete,
    path = "/w/{workspaceId}/courses/{courseId}/units/{unitId}/comments/{commentId}",
    operation_id = "deleteUnitComment",
    tag = tag::COURSES,
    params(MemberWorkspaceId, CourseId, UnitId, UnitCommentId),
    responses((status = 204, description = "The comment is deleted.")),
)]
async fn handle_delete_unit_comment(
    InCourse(ws): InCourse,
    Path((_, course_id, unit_id, comment_id)): Path<(String, String, String, String)>,
) -> ApiResult {
    let user_id = ws.session.user_id.as_str();
    let unit = content_unit_with_permission(
        &ws,
        &course_id,
        &unit_id,
        ContainerPermission::CommentCards,
        CHANGES_DENIED,
    )
    .await?;
    let comment = existing_comment_on(&ws, &unit, &comment_id).await?;
    let response = delete_comment_on(&ws, &comment, CHANGES_DENIED).await?;
    tracing::debug!(course_id, unit_id, comment_id, user_id, "DeleteUnitComment");
    Ok(response)
}

/// Bring a deleted comment on a unit back
///
/// A comment deleted on its own comes back onto its unit, as `restorePageComment`
/// brings a page's comment back: for its author from the commenter's role up if they
/// deleted it themselves, and for the course's admins whoever wrote or deleted it (403,
/// `access denied to undelete other users' comments`, and for the author of a comment
/// somebody else deleted, `a comment somebody else deleted is brought back by the
/// board's admins`). A comment that went to the trash with its unit comes back with the
/// unit (`restoreUnit`), not on its own; that one, one not in the trash, and one of
/// another unit are not there (404), nor is any while the unit is in the trash. Who
/// reads the course is told with `blockChanged`.
#[utoipa::path(
    post,
    path = "/w/{workspaceId}/courses/{courseId}/units/{unitId}/comments/{commentId}/restore",
    operation_id = "restoreUnitComment",
    tag = tag::COURSES,
    params(MemberWorkspaceId, CourseId, UnitId, UnitCommentId),
    responses((status = 200, description = "The comment as it is again.", body = Block)),
)]
async fn handle_restore_unit_comment(
    InCourse(ws): InCourse,
    Path((_, course_id, unit_id, comment_id)): Path<(String, String, String, String)>,
) -> ApiResult {
    let user_id = ws.session.user_id.as_str();
    let unit = content_unit_with_permission(
        &ws,
        &course_id,
        &unit_id,
        ContainerPermission::CommentCards,
        CHANGES_DENIED,
    )
    .await?;
    let comment = restore_comment_on(&ws, &unit, &comment_id, CHANGES_DENIED).await?;
    tracing::debug!(
        course_id,
        unit_id,
        comment_id,
        user_id,
        "UndeleteUnitComment"
    );
    Ok(json_response(&comment))
}

/// What a comment on a unit said before
///
/// The earlier texts of the comment, as `getPageCommentVersions` gives a page's
/// comment's: every version but the one it has now, the first one first, with who wrote
/// it and when. For who reads the course (as `getUnitComments`), also of a comment in
/// the trash while its unit is there: the versions go only when the comment is gone for
/// good. A chapter or a unit in the trash is not there (404).
#[utoipa::path(
    get,
    path = "/w/{workspaceId}/courses/{courseId}/units/{unitId}/comments/{commentId}/versions",
    operation_id = "getUnitCommentVersions",
    tag = tag::COURSES,
    params(MemberWorkspaceId, CourseId, UnitId, UnitCommentId),
    responses((status = 200, description = "The earlier versions, the first one first.", body = Vec<CommentVersion>)),
)]
async fn handle_get_unit_comment_versions(
    InCourse(ws): InCourse,
    Path((_, course_id, unit_id, comment_id)): Path<(String, String, String, String)>,
) -> ApiResult {
    let unit = content_unit_with_permission(
        &ws,
        &course_id,
        &unit_id,
        ContainerPermission::View,
        READ_DENIED,
    )
    .await?;
    let live = find_live_comment_on(&ws, &unit, &comment_id).await?;
    get_comment_versions_on(&ws, &unit, live, &comment_id).await
}
