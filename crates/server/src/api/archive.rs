//! Exporting and importing board archives.
//!
//! Ports `server/api/archive.go`. The team-export route is registered twice in the
//! original (the standalone server answers it); the import field is `file`.

use std::time::Duration;

use axum::Extension;
use axum::body::{Body, Bytes};
use axum::extract::{ConnectInfo, Path, State};
use axum::response::{IntoResponse, Response};
use fvoci_model::UploadPurpose;
use fvoci_store::PublicBoards;
use serde::Deserialize;
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::body::decode_object;
use super::contract::bodies::Binary;
use super::contract::params::{BoardId, TeamId};
use super::contract::tag;
use super::{ApiResult, RequiredSession, no_content};
use super::{require_board_permission, require_team_access, user_is_guest};
use crate::app::{App, ExportedArchive, Slot, download};
use crate::connection::{Breaker, Connection};
use crate::error::AppError;
use crate::permissions::BoardPermission;

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
///
/// A user runs one export at a time, and the server as many as its setting
/// `max_concurrent_exports` (2 unless set). Over either it is 429 with the reason
/// `export_running` or `server_exports_busy`, and is to be asked for again later. An
/// export runs until its archive is sent or the client goes away; a download that takes
/// nothing for `export_stall_seconds` (60 unless set) is given up, and its connection
/// broken before the archive's end.
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
    connection: Option<Extension<ConnectInfo<Connection>>>,
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
    let slot = app.start_export(user_id)?;
    let archive = app
        .export_archive(&board.team_id, std::slice::from_ref(&board.id))
        .await?;
    Ok(archive_response(
        &app,
        archive,
        slot,
        breaker_of(connection),
    ))
}

/// Export the boards of a team
///
/// An archive of all boards of the team that `getBoards` lists for the user, offered
/// as a download with its length, in the format of `exportBoardArchive`, and under its
/// limits. A team that is not there is not refused: its archive has no boards.
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
    connection: Option<Extension<ConnectInfo<Connection>>>,
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
    let slot = app.start_export(user_id)?;
    let archive = app.export_archive(&team_id, &ids).await?;
    Ok(archive_response(
        &app,
        archive,
        slot,
        breaker_of(connection),
    ))
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
///
/// A user runs one import at a time, and the server as many as its setting
/// `max_concurrent_imports` (2 unless set). Over either it is 429 with the reason
/// `import_running` or `server_imports_busy`; the upload stays open, to be named again.
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
    // Refused, the upload stays: the import can be asked for again.
    let _slot = app.start_import(user_id)?;
    // What is refused of the archive's boards and blocks is in the archive, not in the
    // request, which names the upload alone.
    app.import_upload(&upload)
        .await
        .map_err(AppError::outside_the_request)?;
    Ok(no_content())
}

/// Sends an archive, written whole before, as the download the original sends, with its
/// length. The export's `slot` and its file are held until the archive is sent, the
/// client goes away, or the download stalls (`app::download`).
fn archive_response(
    app: &App,
    archive: ExportedArchive,
    slot: Slot,
    breaker: Option<Breaker>,
) -> Response {
    let stall = Duration::from_secs(app.config().export_stall_seconds);
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
        Body::from_stream(download(archive, slot, stall, breaker)),
    )
        .into_response()
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
