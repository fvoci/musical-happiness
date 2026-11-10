//! Exporting and importing board archives.
//!
//! Ports `server/api/archive.go`. The team-export route is registered twice in the
//! original (the standalone server answers it); the import field is `file`.

use axum::body::{Body, Bytes};
use axum::extract::multipart::MultipartRejection;
use axum::extract::{Multipart, Path, State};
use axum::response::{IntoResponse, Response};
use fvoci_store::PublicBoards;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::contract::bodies::{Binary, FileForm};
use super::contract::params::{BoardId, TeamId};
use super::contract::tag;

use super::files::NOT_MULTIPART;
use super::{ApiResult, RequiredSession, no_content};
use super::{require_board_permission, require_team_access, user_is_guest};
use crate::app::App;
use crate::error::AppError;
use crate::permissions::BoardPermission;

/// The multipart form field an imported archive is sent in.
const UPLOAD_FIELD: &str = "file";

pub(super) fn routes() -> OpenApiRouter<App> {
    OpenApiRouter::new()
        .routes(routes!(handle_export_board))
        .routes(routes!(handle_export_team))
}

pub(super) fn upload_routes() -> OpenApiRouter<App> {
    OpenApiRouter::new().routes(routes!(handle_import))
}

/// Export a board
///
/// An archive of the board with its blocks, its files and its members, offered as a
/// download. For the board's members. The archive is a ZIP of version 3: `version.json`,
/// and per board a directory with `board.jsonl` (one `board`, `block` or `boardMember`
/// per line, in the shapes of this contract, bodies included) and the files the blocks
/// show, the images of bodies among them.
#[utoipa::path(
    get,
    path = "/boards/{boardId}/archive/export",
    operation_id = "exportBoardArchive",
    tag = tag::ARCHIVES,
    params(BoardId),
    responses((status = 200, description = "The archive.", body = inline(Binary), content_type = "application/octet-stream")),
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
    let bytes = app
        .export_archive(&board.team_id, std::slice::from_ref(&board.id))
        .await?;
    Ok(archive_response(bytes))
}

/// Export the boards of a team
///
/// An archive of all boards of the team that `getBoards` lists for the user, offered
/// as a download, in the format of `exportBoardArchive`. A team that is not there is not
/// refused: its archive has no boards.
#[utoipa::path(
    get,
    path = "/teams/{teamId}/archive/export",
    operation_id = "exportTeamArchive",
    tag = tag::ARCHIVES,
    params(TeamId),
    responses((status = 200, description = "The archive.", body = inline(Binary), content_type = "application/octet-stream")),
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
    let bytes = app.export_archive(&team_id, &ids).await?;
    Ok(archive_response(bytes))
}

/// Import an archive
///
/// Makes the boards of an archive in the team, with the user as their admin. The
/// members the archive names for a board become members of it with the roles they had,
/// where they are users of this server. The request is a `multipart/form-data` form
/// whose part `file` carries the archive; the part needs no file name. A request
/// without that part, or with what is no archive, is 400.
///
/// The archive is of version 3, as the export routes write it. The formats of the
/// original (the line-based version 1 and the ZIP of version 2) and a ZIP without
/// `version.json` are 400, and nothing of them is stored. The blocks that show files
/// are pointed at the board's new copies of them, the images of bodies included; a
/// card value that does not fit its property's kind is cleared, as a copy does.
///
/// The boards are made one after the other, not all or none: when one of them fails,
/// those before it stay.
#[utoipa::path(
    post,
    path = "/teams/{teamId}/archive/import",
    operation_id = "importArchive",
    tag = tag::ARCHIVES,
    params(TeamId),
    request_body(content = FileForm, content_type = "multipart/form-data"),
    responses((status = 204, description = "The boards of the archive are made.")),
)]
async fn handle_import(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    Path(team_id): Path<String>,
    multipart: Result<Multipart, MultipartRejection>,
) -> ApiResult {
    let user_id = session.user_id.as_str();
    // Any signed-in, non-guest user of the team may import.
    require_team_access(&app, user_id, &team_id, "access denied to create board").await?;
    if user_is_guest(&app, user_id).await? {
        return Err(AppError::forbidden("access denied to create board"));
    }

    let mut multipart = multipart.map_err(|_| AppError::bad_request(NOT_MULTIPART))?;
    let bytes = read_field(&mut multipart)
        .await
        .map_err(|message| AppError::bad_request(message).with_field(UPLOAD_FIELD))?;

    app.import_team_archive(bytes, &team_id, user_id).await?;
    Ok(no_content())
}

/// Reads the uploaded `file` field, or returns the message the original would write.
async fn read_field(multipart: &mut Multipart) -> Result<Bytes, String> {
    loop {
        let field = multipart
            .next_field()
            .await
            .map_err(|error| error.to_string())?;
        let Some(field) = field else {
            return Err("the request has no part named `file`".to_owned());
        };
        if field.name() != Some(UPLOAD_FIELD) {
            continue;
        }
        return field.bytes().await.map_err(|error| error.to_string());
    }
}

/// Wraps archive bytes in the download response the original sends.
fn archive_response(bytes: Vec<u8>) -> Response {
    let filename = export_filename(crate::auth::now_millis());
    (
        [
            ("content-type", "application/octet-stream".to_owned()),
            (
                "content-disposition",
                format!("attachment; filename={filename}"),
            ),
            ("content-transfer-encoding", "binary".to_owned()),
        ],
        Body::from(bytes),
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
