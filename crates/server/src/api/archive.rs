//! Exporting and importing archives of boards and wikis.
//!
//! Ports `server/api/archive.go`. The workspace-export route is registered twice in the
//! original (the standalone server answers it); the import field is `file`.

use axum::Extension;
use axum::body::{Body, Bytes};
use axum::extract::{ConnectInfo, Path, State};
use axum::response::{IntoResponse, Response};
use fvoci_model::{ContainerKinds, Session, UploadPurpose};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::body::decode_object;
use super::contract::bodies::Binary;
use super::contract::params::{ContainerId, MemberWorkspaceId, WorkspaceId};
use super::contract::tag;
use super::require_container_permission;
use super::workspace::{InContainer, InWorkspace};
use super::{ApiResult, RequiredSession, json_response};
use crate::app::{App, ExportedArchive, Slot, download};
use crate::connection::{Breaker, Connection};
use crate::error::AppError;
use crate::permissions::ContainerPermission;

/// An archive to import: one sent in parts with `createImportUpload`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ImportRequest {
    /// The upload of the archive, every part of it received. It is thrown away by the
    /// import, whether the import succeeds or not.
    upload_id: String,
}

/// What an import did beyond making the archive's boards.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct ImportResult {
    /// How many of the users who were members of the archive's boards were left out of
    /// the boards made of them, because they are not in the workspace; each counted
    /// once. Who they are is not told.
    members_left_out: usize,
}

pub(super) fn routes() -> OpenApiRouter<App> {
    OpenApiRouter::new()
        .routes(routes!(handle_export_container))
        .routes(routes!(handle_export_workspace))
        .routes(routes!(handle_import))
}

/// Enters the named workspace for a member who may import into it.
///
/// # Errors
///
/// Returns 404 when the user is not a member or is a guest of the workspace.
pub(super) async fn require_import_access(
    app: &App,
    session: Session,
    workspace_id: String,
) -> Result<InWorkspace, AppError> {
    InWorkspace::enter(app, session, workspace_id)
        .await?
        .require_no_guest()
}

/// Export a board or a wiki
///
/// An archive of the board with its blocks, its files and its members, offered as a
/// download with its length. For the board's members. The archive is a ZIP of version
/// 3: `version.json`, and per board a directory with `board.jsonl` (one `board`,
/// `block` or `boardMember` per line, in the shapes of this contract, bodies included,
/// but that a block and a member name their board or wiki `boardId`, not `containerId`)
/// and the files the blocks show, those of the images and file blocks of bodies among
/// them. A board's line also carries `fileNames`: the name of each of its files, by
/// stored name, which an import gives the files it stores. A name an upload could not
/// have (blank, with U+0000, over 255 characters, or of another extension than the
/// stored name) is passed over and its file keeps the name of its entry, and an archive
/// without the member is read as before. A file a block shows that the board does not
/// have, its record or its bytes gone, gets no entry: it is listed under `missingFiles`
/// of the board's line, by stored name. A wiki is written as a board is, its `kind` `wiki`: its
/// pages as they stand and the comments on them are its blocks (not the pages in its
/// trash, nor the versions of bodies).
///
/// A user runs one export at a time, and the server as many as its setting
/// `max_concurrent_exports` (2 unless set). Over either it is 429 with the reason
/// `export_running` or `server_exports_busy`, and is to be asked for again later. An
/// export runs until its archive is sent or the client goes away; a download that takes
/// nothing for `export_stall_seconds` (60 unless set) is given up, and its connection
/// broken before the archive's end.
#[utoipa::path(
    get,
    path = "/w/{workspaceId}/containers/{containerId}/archive/export",
    operation_id = "exportContainerArchive",
    tag = tag::ARCHIVES,
    params(MemberWorkspaceId, ContainerId),
    responses((
        status = 200,
        description = "The archive.",
        body = inline(Binary),
        content_type = "application/octet-stream",
        headers(
            ("Content-Length" = u64, description = "The size of the archive in bytes."),
            ("Content-Disposition" = String, description = "`attachment; filename=archive-YYYY-MM-DD.boardarchive`, for the day (UTC)."),
        ),
    )),
)]
async fn handle_export_container(
    InContainer(ws): InContainer,
    connection: Option<Extension<ConnectInfo<Connection>>>,
    Path((_, container_id)): Path<(String, String)>,
) -> ApiResult {
    let user_id = &ws.session.user_id;
    // The original also lets a system admin with a compliance license through; a
    // standalone server has neither, so a non-member is refused.
    require_container_permission(
        &ws.app,
        user_id,
        &container_id,
        ContainerPermission::View,
        "access denied to board",
    )
    .await?;
    let container = ws.app.container(&container_id).await?;
    let slot = ws.app.start_export(user_id)?;
    let archive = ws
        .app
        .export_archive(&container.workspace_id, std::slice::from_ref(&container.id))
        .await?;
    Ok(archive_response(
        &ws.app,
        archive,
        slot,
        breaker_of(connection),
    ))
}

/// Export the boards and wikis of a workspace
///
/// An archive of the boards and wikis of the workspace that the user is a member of
/// with a role, and of its open ones for who works in the workspace (also those they
/// have not joined), offered as a download with its length, in the format of
/// `exportContainerArchive`, and under its limits. 404 unless the user is a member of the
/// workspace; a guest's archive has the boards and wikis they were invited to.
#[utoipa::path(
    get,
    path = "/w/{workspaceId}/archive/export",
    operation_id = "exportWorkspaceArchive",
    tag = tag::ARCHIVES,
    params(MemberWorkspaceId),
    responses((
        status = 200,
        description = "The archive.",
        body = inline(Binary),
        content_type = "application/octet-stream",
        headers(
            ("Content-Length" = u64, description = "The size of the archive in bytes."),
            ("Content-Disposition" = String, description = "`attachment; filename=archive-YYYY-MM-DD.boardarchive`, for the day (UTC)."),
        ),
    )),
)]
async fn handle_export_workspace(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    connection: Option<Extension<ConnectInfo<Connection>>>,
    Path(workspace_id): Path<String>,
) -> ApiResult {
    let ws = InWorkspace::enter(&app, session, workspace_id).await?;
    let app = &ws.app.clone().reaching(ContainerKinds::Every);
    let user_id = &ws.session.user_id;
    let containers = app
        .readable_workspace_containers(user_id, &ws.workspace_id, ws.role)
        .await?;
    let ids: Vec<String> = containers
        .into_iter()
        .map(|container| container.id)
        .collect();
    let slot = app.start_export(user_id)?;
    let archive = app.export_archive(&ws.workspace_id, &ids).await?;
    Ok(archive_response(app, archive, slot, breaker_of(connection)))
}

/// Import an archive
///
/// Makes the boards and wikis of an archive in the workspace, with the user as their
/// admin. The members the archive names for a board become members of it with the roles
/// they had, where they are in the workspace. The archive is sent before in parts: open an
/// upload with `createImportUpload`, send its parts with `putUploadPart`, then name
/// the upload here, in place of `completeUpload`. A part missing is 400 and the upload
/// stays open for it; otherwise the upload ends with the import, whether it succeeds or
/// not. Another user's upload, one that has ended, and one for another workspace are
/// 404; an upload of a file is 400. A workspace the user is not in is 404 before the
/// body is read. For users of the workspace who are not guests, at the import as when
/// the upload was opened: a user who may not import any more is refused, and the upload
/// is thrown away.
///
/// The archive is of version 3, as the export routes write it. The formats of the
/// original (the line-based version 1 and the ZIP of version 2) and a ZIP without
/// `version.json` are 400, and nothing of them is stored. An archive of more than
/// 10,000 entries, whose entries unpack to more than 1 GiB together, or with a
/// `board.jsonl` of more than 64 MiB or a file larger than `maxFileBytes` is 413 with
/// the `limit` passed, and nothing of it is stored. The blocks that show files are
/// pointed at the board's new copies of them, the images and file blocks of bodies
/// included. A file the board's line lists under `missingFiles` is not stored, even when
/// the archive has an entry of its name, and what shows it ends up showing no file. A card value that does not fit its property's kind is cleared, as a copy
/// does. A wiki of the archive (`kind` `wiki`) is made as a wiki, its pages and the
/// comments on them with it. What a write would refuse of an archive's boards, wikis
/// and blocks is 400 before any of them is stored, with the message the write has: a
/// block of a type of the other kind (a page on a board, a card on a wiki), a page
/// below anything but a page of its wiki or below a page the archive does not have,
/// pages that are each other's parents, a comment on anything but a card or page, a
/// wiki that is a template, and a wiki deeper or with more pages than the
/// organization's limits allow (400 with the reason `page_too_deep` or
/// `too_many_pages`).
///
/// The boards are made one after the other, not all or none: when one of them fails,
/// those before it stay. The members of a board who are not in the workspace are left
/// out of it, a guest of the workspace keeps no role above editor; the answer says how
/// many users were left out, not who.
///
/// A user runs one import at a time, and the server as many as its setting
/// `max_concurrent_imports` (2 unless set). Over either it is 429 with the reason
/// `import_running` or `server_imports_busy`; the upload stays open, to be named again.
#[utoipa::path(
    post,
    path = "/w/{workspaceId}/archive/import",
    operation_id = "importArchive",
    tag = tag::ARCHIVES,
    params(WorkspaceId),
    request_body = ImportRequest,
    responses((
        status = 200,
        description = "The boards of the archive are made.",
        body = ImportResult,
    )),
)]
async fn handle_import(InContainer(ws): InContainer, body: Bytes) -> ApiResult {
    let ws = ws.require_no_guest()?;
    let user_id = ws.session.user_id.clone();
    // Membership is checked before reading the body; the upload belongs to this workspace.
    let ImportRequest { upload_id } = decode_object(&body)?;
    let (upload, _) = ws.app.workspace_upload(&upload_id, &user_id).await?;
    if upload.purpose != UploadPurpose::Import {
        return Err(AppError::bad_request(
            "the upload is of a file; complete it with completeUpload",
        )
        .with_field("uploadId"));
    }
    // Refused, the upload stays: the import can be asked for again.
    let _slot = ws.app.start_import(&user_id)?;
    // What is refused of the archive's boards and blocks is in the archive, not in the
    // request, which names the upload alone.
    let imported = ws
        .app
        .import_upload(&upload)
        .await
        .map_err(AppError::outside_the_request)?;
    Ok(json_response(&ImportResult {
        members_left_out: imported.members_left_out,
    }))
}

/// Sends an archive, written whole before, as the download the original sends, with its
/// length. The export's `slot` and its file are held until the last piece of the
/// archive is read, the client goes away, or the download stalls (`app::download`).
fn archive_response(
    app: &App,
    archive: ExportedArchive,
    slot: Slot,
    breaker: Option<Breaker>,
) -> Response {
    let stall = app.config().export_stall();
    let size = archive.size;
    let filename = export_filename(crate::auth::now_millis());
    (
        [
            ("content-type", "application/octet-stream".to_owned()),
            (
                "content-disposition",
                format!("attachment; filename={filename}"),
            ),
            ("content-transfer-encoding", "binary".to_owned()),
            ("content-length", size.to_string()),
        ],
        Body::from_stream(download(archive, slot, stall, cutting(breaker))),
    )
        .into_response()
}

/// What the download calls to break its connection: cuts `breaker`, if there is one.
fn cutting(breaker: Option<Breaker>) -> impl FnOnce() + Send + 'static {
    move || {
        if let Some(breaker) = breaker {
            breaker.cut();
        }
    }
}

/// The breaker of the connection a request came on, if it has one.
fn breaker_of(connection: Option<Extension<ConnectInfo<Connection>>>) -> Option<Breaker> {
    connection.and_then(|Extension(ConnectInfo(connection))| connection.breaker)
}

/// The file name an export is offered under: `archive-YYYY-MM-DD.boardarchive` for today
/// (UTC).
fn export_filename(now_millis: i64) -> String {
    let days = now_millis.div_euclid(86_400_000);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("archive-{year:04}-{month:02}-{day:02}.boardarchive")
}

#[cfg(test)]
mod tests {
    use super::{cutting, export_filename};
    use crate::connection::Breaker;

    #[test]
    fn the_download_is_handed_what_cuts_the_connection() {
        let breaker = Breaker::default();
        let cut = cutting(Some(breaker.clone()));
        assert!(!breaker.is_cut(), "not before it is called");
        cut();
        assert!(breaker.is_cut(), "the connection's breaker is cut");
        // The administration socket has no breaker: nothing to cut.
        cutting(None)();
    }

    #[test]
    fn export_filename_formats_the_date() {
        // 2026-11-10T05:15:39Z.
        assert_eq!(
            export_filename(1_794_287_739_000),
            "archive-2026-11-10.boardarchive",
            "the archive file name for a known date"
        );
    }
}
