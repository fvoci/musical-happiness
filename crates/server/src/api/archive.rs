//! Exporting and importing board archives.
//!
//! Ports `server/api/archive.go`. The team-export route is registered twice in the
//! original (the standalone server answers it); the import field is `file`.

use axum::body::{Body, Bytes};
use axum::extract::{Path, State};
use axum::response::{IntoResponse, Response};
use fvoci_model::UploadPurpose;
use fvoci_store::PublicBoards;
use serde::Deserialize;
use tokio_util::io::ReaderStream;
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::body::decode_object;
use super::contract::bodies::Binary;
use super::contract::params::{BoardId, TeamId};
use super::contract::tag;
use super::{ApiResult, RequiredSession, no_content};
use super::{require_board_permission, require_team_access, user_is_guest};
use crate::app::{App, ExportedArchive};
use crate::error::AppError;
use crate::permissions::BoardPermission;

/// How much of an archive is read from its file at a time to be sent.
const SEND_CHUNK_BYTES: usize = 256 * 1024;

/// What is refused to a user who may not import into the team.
const IMPORT_DENIAL: &str = "access denied to create board";

/// An archive to import: one sent in parts with `createImportUpload`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ImportRequest {
    /// The upload of the archive, every part of it received. It is thrown away by the
    /// import, whether the import succeeds or not.
    upload_id: String,
}

pub(super) fn routes() -> OpenApiRouter<App> {
    OpenApiRouter::new()
        .routes(routes!(handle_export_board))
        .routes(routes!(handle_export_team))
        .routes(routes!(handle_import))
}

/// Checks that the user may import into the team: any signed-in user of the team who is
/// not a guest.
///
/// # Errors
///
/// Returns the refusal, 403 for a guest or a user outside the team.
pub(super) async fn require_import_access(
    app: &App,
    user_id: &str,
    team_id: &str,
) -> Result<(), AppError> {
    require_team_access(app, user_id, team_id, IMPORT_DENIAL).await?;
    if user_is_guest(app, user_id).await? {
        return Err(AppError::forbidden(IMPORT_DENIAL));
    }
    Ok(())
}

/// Export a board
///
/// An archive of the board with its blocks, its files and its members, offered as a
/// download with its length. For the board's members. The archive is a ZIP of version
/// 3: `version.json`, and per board a directory with `board.jsonl` (one `board`,
/// `block` or `boardMember` per line, in the shapes of this contract, bodies included)
/// and the files the blocks show, the images of bodies among them.
#[utoipa::path(
    get,
    path = "/boards/{boardId}/archive/export",
    operation_id = "exportBoardArchive",
    tag = tag::ARCHIVES,
    params(BoardId),
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
async fn handle_export_board(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    Path(board_id): Path<String>,
) -> ApiResult {
    let user_id = &session.user_id;
    // The original also lets a system admin with a compliance license through; a
    // standalone server has neither, so a non-member is refused.
    require_board_permission(
        &app,
        user_id,
        &board_id,
        BoardPermission::ViewBoard,
        "access denied to board",
    )
    .await?;
    let board = app.board(&board_id).await?;
    let archive = app
        .export_archive(&board.team_id, std::slice::from_ref(&board.id))
        .await?;
    Ok(archive_response(archive))
}

/// Export the boards of a team
///
/// An archive of all boards of the team that `getBoards` lists for the user, offered
/// as a download with its length, in the format of `exportBoardArchive`. A team that
/// is not there is not refused: its archive has no boards.
#[utoipa::path(
    get,
    path = "/teams/{teamId}/archive/export",
    operation_id = "exportTeamArchive",
    tag = tag::ARCHIVES,
    params(TeamId),
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
async fn handle_export_team(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    Path(team_id): Path<String>,
) -> ApiResult {
    let user_id = &session.user_id;
    let public = if user_is_guest(&app, user_id).await? {
        PublicBoards::Exclude
    } else {
        PublicBoards::Include
    };
    let boards = app
        .boards_for_user_and_team(user_id, &team_id, public)
        .await?;
    let ids: Vec<String> = boards.into_iter().map(|board| board.id).collect();
    let archive = app.export_archive(&team_id, &ids).await?;
    Ok(archive_response(archive))
}

/// Import an archive
///
/// Makes the boards of an archive in the team, with the user as their admin. The
/// members the archive names for a board become members of it with the roles they had,
/// where they are users of this server. The archive is sent before in parts: open an
/// upload with `createImportUpload`, send its parts with `putUploadPart`, then name
/// the upload here, in place of `completeUpload`. A part missing is 400 and the upload
/// stays open for it; otherwise the upload ends with the import, whether it succeeds or
/// not. Another user's upload, or one that has ended, is 404; an upload of a file, or
/// one for another team, is 400. For users of the team who are not guests, at the
/// import as when the upload was opened: a user who may not import any more is
/// refused, and the upload is thrown away.
///
/// The archive is of version 3, as the export routes write it. The formats of the
/// original (the line-based version 1 and the ZIP of version 2) and a ZIP without
/// `version.json` are 400, and nothing of them is stored. An archive of more than
/// 10,000 entries, whose entries unpack to more than 1 GiB together, or with a
/// `board.jsonl` of more than 64 MiB or a file larger than `maxFileSize` is 413 with
/// the `limit` passed, and nothing of it is stored. The blocks that show files are
/// pointed at the board's new copies of them, the images of bodies included; a card
/// value that does not fit its property's kind is cleared, as a copy does.
///
/// The boards are made one after the other, not all or none: when one of them fails,
/// those before it stay.
#[utoipa::path(
    post,
    path = "/teams/{teamId}/archive/import",
    operation_id = "importArchive",
    tag = tag::ARCHIVES,
    params(TeamId),
    request_body = ImportRequest,
    responses((status = 204, description = "The boards of the archive are made.")),
)]
async fn handle_import(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    Path(team_id): Path<String>,
    body: Bytes,
) -> ApiResult {
    let user_id = session.user_id.as_str();
    let ImportRequest { upload_id } = decode_object(&body)?;
    let (upload, _) = app.upload(&upload_id, user_id).await?;
    if upload.purpose != UploadPurpose::Import {
        return Err(AppError::bad_request(
            "the upload is of a file; complete it with completeUpload",
        )
        .with_field("uploadId"));
    }
    if upload.team_id != team_id {
        return Err(
            AppError::bad_request("the upload is of an archive for another team")
                .with_field("uploadId"),
        );
    }
    if let Err(refusal) = require_import_access(&app, user_id, &team_id).await {
        // As for a completion (design 5.6, 2): the upload cannot be imported any more.
        if refusal.status().is_client_error() {
            app.discard_upload(&upload).await;
        }
        return Err(refusal);
    }
    app.import_upload(&upload).await?;
    Ok(no_content())
}

/// Sends an archive, written whole before, as the download the original sends, with its
/// length.
fn archive_response(archive: ExportedArchive) -> Response {
    let filename = export_filename(crate::auth::now_millis());
    (
        [
            ("content-type", "application/octet-stream".to_owned()),
            (
                "content-disposition",
                format!("attachment; filename={filename}"),
            ),
            ("content-transfer-encoding", "binary".to_owned()),
            ("content-length", archive.size.to_string()),
        ],
        // Read in large pieces: each read of the file is a task of its own.
        Body::from_stream(ReaderStream::with_capacity(archive.file, SEND_CHUNK_BYTES)),
    )
        .into_response()
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
    use super::export_filename;

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
