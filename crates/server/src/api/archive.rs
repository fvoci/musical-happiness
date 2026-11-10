//! Exporting and importing board archives.
//!
//! Ports `server/api/archive.go`. The team-export route is registered twice in the
//! original (the standalone server answers it); the import field is `file`.

use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::multipart::MultipartRejection;
use axum::extract::{Multipart, Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use fvoci_store::PublicBoards;

use super::files::NOT_MULTIPART;
use super::{ApiResult, RequiredSession, empty_response};
use super::{require_team_access, user_is_guest};
use crate::app::App;
use crate::error::AppError;
use crate::permissions::BoardPermission;

/// The multipart form field an imported archive is sent in.
const UPLOAD_FIELD: &str = "file";

pub(super) fn routes() -> Router<App> {
    Router::new()
        .route(
            "/boards/{board_id}/archive/export",
            get(handle_export_board),
        )
        .route("/teams/{team_id}/archive/export", get(handle_export_team))
        .route("/teams/{team_id}/archive/import", post(handle_import))
}

/// `GET /boards/{board_id}/archive/export`: the board as an archive.
async fn handle_export_board(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    Path(board_id): Path<String>,
) -> ApiResult {
    let user_id = &session.user_id;
    // The original also lets a system admin with a compliance license through; a
    // standalone server has neither, so a non-member is refused.
    if !app
        .has_permission_to_board(user_id, &board_id, BoardPermission::ViewBoard)
        .await
    {
        return Err(AppError::forbidden("access denied to board"));
    }
    let board = app.board(&board_id).await?;
    let bytes = app
        .export_archive(&board.team_id, std::slice::from_ref(&board.id))
        .await?;
    Ok(archive_response(bytes))
}

/// `GET /teams/{team_id}/archive/export`: all the team's boards the user may see.
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

/// `POST /teams/{team_id}/archive/import`: imports an uploaded archive.
async fn handle_import(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    Path(team_id): Path<String>,
    multipart: Result<Multipart, MultipartRejection>,
) -> ApiResult {
    let user_id = session.user_id.as_str();
    // Any signed-in, non-guest user of the team may import.
    require_team_access(&app, user_id, &team_id, "access denied to create board")?;
    if user_is_guest(&app, user_id).await? {
        return Err(AppError::forbidden("access denied to create board"));
    }

    // The original writes what is wrong with the upload as a plain 200 body.
    let Ok(mut multipart) = multipart else {
        return Ok((StatusCode::OK, NOT_MULTIPART).into_response());
    };
    let bytes = match read_field(&mut multipart).await {
        Ok(bytes) => bytes,
        // The original writes the multipart error as a plain 200 body and returns.
        Err(message) => {
            return Ok((StatusCode::OK, message).into_response());
        }
    };

    app.import_team_archive(bytes, &team_id, user_id).await?;
    Ok(empty_response())
}

/// Reads the uploaded `file` field, or returns the message the original would write.
async fn read_field(multipart: &mut Multipart) -> Result<Bytes, String> {
    loop {
        let field = multipart
            .next_field()
            .await
            .map_err(|error| error.to_string())?;
        let Some(field) = field else {
            return Err("http: no such file".to_owned());
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
