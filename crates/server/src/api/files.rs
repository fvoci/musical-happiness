//! File upload and download.
//!
//! Ports `server/api/files.go`. Downloads reproduce the headers and the range and
//! conditional handling of Go's `http.ServeContent`.

use std::time::SystemTime;

use axum::Router;
use axum::body::Body;
use axum::extract::multipart::MultipartRejection;
use axum::extract::{Multipart, Path, RawQuery, State};
use axum::http::header::{
    ACCEPT_RANGES, CACHE_CONTROL, CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_RANGE, CONTENT_TYPE,
    IF_MODIFIED_SINCE, IF_NONE_MATCH, IF_RANGE, IF_UNMODIFIED_SINCE, LAST_MODIFIED, RANGE,
};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use serde_json::json;

use super::{
    ApiResult, OptionalSession, RequiredSession, board_refusal, has_valid_read_token,
    json_response, require_board_permission,
};
use crate::app::App;
use crate::app::FILE;
use crate::error::AppError;
use crate::permissions::BoardPermission;

/// Content types that must not be served as themselves, because a browser would run
/// them; they are served as plain text instead.
const UNSAFE_CONTENT_TYPES: [&str; 6] = [
    "application/javascript",
    "application/ecmascript",
    "text/javascript",
    "text/ecmascript",
    "application/x-javascript",
    "text/html",
];

/// Content types a browser can show inline; anything else is offered as a download.
const MEDIA_CONTENT_TYPES: [&str; 10] = [
    "image/jpeg",
    "image/png",
    "image/bmp",
    "image/gif",
    "image/tiff",
    "video/avi",
    "video/mpeg",
    "video/mp4",
    "audio/mpeg",
    "audio/wav",
];

/// What the original answers a request that is no multipart form with.
pub(super) const NOT_MULTIPART: &str = "request Content-Type isn't multipart/form-data";

/// The multipart form field the uploaded file is sent in.
const UPLOAD_FIELD: &str = "file";

/// The refusal of an upload that carries no part with the file.
const NO_FILE_PART: &str = "the request has no part named `file` with a file name";

/// The routes that read files.
pub(super) fn routes() -> Router<App> {
    Router::new()
        .route(
            "/files/teams/{team_id}/{board_id}/{filename}",
            get(handle_serve_file),
        )
        .route(
            "/files/teams/{team_id}/{board_id}/{filename}/info",
            get(handle_get_file_info),
        )
}

/// The route that takes a file, whose size has a limit of its own.
pub(super) fn upload_routes() -> Router<App> {
    Router::new().route(
        "/teams/{team_id}/{board_id}/files",
        post(handle_upload_file),
    )
}

/// Checks that the requester may read the board's files: a member with the view
/// permission, or the holder of a valid read token.
async fn check_read_access(
    app: &App,
    user_id: &str,
    board_id: &str,
    query: Option<&str>,
) -> Result<(), AppError> {
    let has_read_token = has_valid_read_token(app, query, board_id).await;
    if user_id.is_empty() && !has_read_token {
        return Err(AppError::unauthorized("access denied to board"));
    }
    if !has_read_token
        && !app
            .has_permission_to_board(user_id, board_id, BoardPermission::ViewBoard)
            .await?
    {
        return Err(board_refusal(app, user_id, board_id, "access denied to board").await);
    }
    Ok(())
}

/// `GET /files/teams/{team_id}/{board_id}/{filename}`: serves an uploaded file.
async fn handle_serve_file(
    State(app): State<App>,
    session: OptionalSession,
    Path((_team_id, board_id, filename)): Path<(String, String, String)>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
) -> ApiResult {
    check_read_access(&app, session.user_id(), &board_id, query.as_deref()).await?;

    // A file is its board's. Through another board it is as missing as a file that
    // does not exist: the answers must not tell the two apart.
    let Some(info) = app.file_info(&board_id, &filename).await? else {
        return Err(AppError::not_found(FILE));
    };
    if !app.files().exists(&info.path).await? {
        return Err(AppError::not_found(FILE));
    }
    let size = app.files().size(&info.path).await?.unwrap_or(0);

    serve_content(&app, &filename, &info.path, size, "", &headers).await
}

/// `GET /files/teams/{team_id}/{board_id}/{filename}/info`: the metadata of a file.
async fn handle_get_file_info(
    State(app): State<App>,
    session: OptionalSession,
    Path((_team_id, board_id, filename)): Path<(String, String, String)>,
    RawQuery(query): RawQuery,
) -> ApiResult {
    check_read_access(&app, session.user_id(), &board_id, query.as_deref()).await?;
    match app.file_info(&board_id, &filename).await? {
        Some(info) => Ok(json_response(&info)),
        // No record, or the record of another board's file.
        None => Err(AppError::not_found(FILE)),
    }
}

/// `POST /teams/{team_id}/{board_id}/files`: stores an uploaded file.
async fn handle_upload_file(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    Path((_team_id, board_id)): Path<(String, String)>,
    multipart: Result<Multipart, MultipartRejection>,
) -> ApiResult {
    let user_id = session.user_id.as_str();
    require_board_permission(
        &app,
        user_id,
        &board_id,
        BoardPermission::ManageBoardCards,
        "access denied to make board changes",
    )
    .await?;
    let max_file_size = app.config().max_file_size;

    let mut multipart = multipart.map_err(|_| AppError::bad_request(NOT_MULTIPART))?;
    let (filename, bytes) = read_upload(&mut multipart, max_file_size).await?;
    let file_id = app
        .save_file(&mut bytes.as_slice(), &board_id, &filename)
        .await?;
    tracing::debug!(filename, file_id, "uploadFile");
    Ok(json_response(&json!({ "fileId": file_id })))
}

/// Reads the uploaded file field, bounding it to `max_file_size` when that is positive.
///
/// Returns the client's file name and the bytes.
async fn read_upload(
    multipart: &mut Multipart,
    max_file_size: i64,
) -> Result<(String, Vec<u8>), AppError> {
    let limit = usize::try_from(max_file_size).unwrap_or(usize::MAX);
    loop {
        let field = multipart
            .next_field()
            .await
            .map_err(|error| AppError::bad_request(error.to_string()))?;
        let Some(mut field) = field else {
            return Err(AppError::bad_request(NO_FILE_PART).with_field(UPLOAD_FIELD));
        };
        if field.name() != Some(UPLOAD_FIELD) {
            continue;
        }
        let Some(filename) = field.file_name().map(str::to_owned) else {
            return Err(AppError::bad_request(NO_FILE_PART).with_field(UPLOAD_FIELD));
        };

        let mut bytes = Vec::new();
        while let Some(chunk) = field
            .chunk()
            .await
            .map_err(|error| AppError::bad_request(error.to_string()))?
        {
            if max_file_size > 0 && bytes.len() + chunk.len() > limit {
                return Err(AppError::payload_too_large(
                    u64::try_from(max_file_size).unwrap_or(u64::MAX),
                ));
            }
            bytes.extend_from_slice(&chunk);
        }
        return Ok((filename, bytes));
    }
}

/// Serves a file's contents with the download headers and the range and conditional
/// handling of `http.ServeContent`.
async fn serve_content(
    app: &App,
    filename: &str,
    path: &str,
    size: u64,
    mime_type: &str,
    request_headers: &HeaderMap,
) -> ApiResult {
    let modtime = SystemTime::now();
    let content_type = content_type_for(mime_type);
    let mut headers = base_headers(filename, &content_type, modtime);

    // Preconditions, in the order of Go's checkPreconditions.
    if let Some(since) = http_date(request_headers, &IF_UNMODIFIED_SINCE)
        && modtime_after(modtime, since)
    {
        headers.insert(CONTENT_LENGTH, header_value(&size.to_string()));
        return Ok((StatusCode::PRECONDITION_FAILED, headers, Body::empty()).into_response());
    }
    if if_none_match_star(request_headers) || not_modified(request_headers, modtime) {
        headers.remove(CONTENT_TYPE);
        return Ok((StatusCode::NOT_MODIFIED, headers, Body::empty()).into_response());
    }

    headers.insert(ACCEPT_RANGES, HeaderValue::from_static("bytes"));

    let range_header = request_headers
        .get(RANGE)
        .and_then(|value| value.to_str().ok())
        .filter(|_| request_headers.get(IF_RANGE).is_none());
    match range_header.map(|header| parse_range(header, size)) {
        None | Some(RangeResult::Full) => {
            headers.insert(CONTENT_LENGTH, header_value(&size.to_string()));
            let stream = app.files().read(path, None).await?;
            Ok((StatusCode::OK, headers, Body::from_stream(stream)).into_response())
        }
        Some(RangeResult::Satisfiable { start, end }) => {
            let length = end - start + 1;
            headers.insert(CONTENT_LENGTH, header_value(&length.to_string()));
            headers.insert(
                CONTENT_RANGE,
                header_value(&format!("bytes {start}-{end}/{size}")),
            );
            let stream = app.files().read(path, Some(start..end + 1)).await?;
            Ok((
                StatusCode::PARTIAL_CONTENT,
                headers,
                Body::from_stream(stream),
            )
                .into_response())
        }
        Some(RangeResult::NotSatisfiable) => {
            headers.insert(CONTENT_RANGE, header_value(&format!("bytes */{size}")));
            Ok(range_error(headers, "invalid range: failed to overlap"))
        }
        Some(RangeResult::Invalid) => Ok(range_error(headers, "invalid range")),
    }
}

/// Builds a 416 response with the error message as its body.
///
/// Deliberate difference: the original's `http.ServeContent` leaves the `Content-Length`
/// it set before the range error, so the body is shorter than the declared length. This
/// port lets the framework set `Content-Length` to the body's real length, because hyper
/// aborts a response whose body is longer than a manual `Content-Length`, which would be
/// a hazard for small files.
fn range_error(mut headers: HeaderMap, message: &str) -> Response {
    headers.insert(
        CONTENT_TYPE,
        HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    (
        StatusCode::RANGE_NOT_SATISFIABLE,
        headers,
        // The original's error writer ends the message with a line break.
        Body::from(format!("{message}\n")),
    )
        .into_response()
}

/// The headers every download carries before range and conditional handling.
fn base_headers(filename: &str, content_type: &str, modtime: SystemTime) -> HeaderMap {
    let escaped = path_escape(filename);
    let disposition = if is_media(content_type) {
        format!("inline;filename=\"{escaped}\"; filename*=UTF-8''{escaped}")
    } else {
        format!("attachment;filename=\"{escaped}\"; filename*=UTF-8''{escaped}")
    };

    let mut headers = HeaderMap::new();
    headers.insert(CACHE_CONTROL, HeaderValue::from_static("private, no-cache"));
    headers.insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(CONTENT_TYPE, header_value(content_type));
    headers.insert(CONTENT_DISPOSITION, header_value(&disposition));
    headers.insert("x-frame-options", HeaderValue::from_static("DENY"));
    // A file can be asked for with a plain link or an image tag, so a browser may open
    // it as a document of this origin. `sandbox` takes that document's scripts and its
    // origin away: an uploaded SVG or HTML file cannot act as the user.
    headers.insert(
        "content-security-policy",
        HeaderValue::from_static("sandbox; frame-ancestors 'none'"),
    );
    headers.insert(
        LAST_MODIFIED,
        header_value(&httpdate::fmt_http_date(modtime)),
    );
    headers
}

/// The content type to serve: octet-stream when unknown, plain text for an unsafe type,
/// otherwise the type itself.
fn content_type_for(mime_type: &str) -> String {
    if mime_type.is_empty() {
        return "application/octet-stream".to_owned();
    }
    if UNSAFE_CONTENT_TYPES
        .iter()
        .any(|unsafe_type| mime_type.starts_with(unsafe_type))
    {
        return "text/plain".to_owned();
    }
    mime_type.to_owned()
}

/// Whether a content type is one a browser shows inline.
fn is_media(content_type: &str) -> bool {
    MEDIA_CONTENT_TYPES
        .iter()
        .any(|media| content_type.starts_with(media))
}

/// The result of parsing a `Range` header against a file of a given size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RangeResult {
    /// Serve the whole file (no byte range, or more than one range).
    Full,
    /// Serve bytes `start..=end`.
    Satisfiable { start: u64, end: u64 },
    /// A well-formed range that lies outside the file: 416 with a `Content-Range`.
    NotSatisfiable,
    /// A malformed range header: 416 without a `Content-Range`.
    Invalid,
}

/// Parses a `Range` header for a file of `size` bytes. Only a single byte range is
/// served; more than one range serves the whole file (the original answers with a
/// multipart body, which no board client requests).
fn parse_range(header: &str, size: u64) -> RangeResult {
    let Some(set) = header.strip_prefix("bytes=") else {
        return RangeResult::Invalid;
    };
    let specs: Vec<&str> = set
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    if specs.is_empty() {
        return RangeResult::Invalid;
    }
    if specs.len() > 1 {
        return RangeResult::Full;
    }
    // An empty file has no byte a range could name. The original answers such a request
    // with the whole (empty) file rather than a 416, because some clients add a range to
    // every request.
    if size == 0 {
        return RangeResult::Full;
    }

    let spec = specs[0];
    let Some((start_text, end_text)) = spec.split_once('-') else {
        return RangeResult::Invalid;
    };

    if start_text.is_empty() {
        // A suffix range: the last N bytes.
        let Ok(suffix) = end_text.parse::<u64>() else {
            return RangeResult::Invalid;
        };
        if suffix == 0 {
            return RangeResult::NotSatisfiable;
        }
        let start = size.saturating_sub(suffix);
        return satisfiable(start, size - 1, size);
    }

    let Ok(start) = start_text.parse::<u64>() else {
        return RangeResult::Invalid;
    };
    if start >= size {
        return RangeResult::NotSatisfiable;
    }
    let end = if end_text.is_empty() {
        size - 1
    } else {
        match end_text.parse::<u64>() {
            Ok(end) => end.min(size - 1),
            Err(_) => return RangeResult::Invalid,
        }
    };
    if end < start {
        return RangeResult::Invalid;
    }
    satisfiable(start, end, size)
}

/// A satisfiable range, guarding against an empty file.
fn satisfiable(start: u64, end: u64, size: u64) -> RangeResult {
    if size == 0 {
        RangeResult::NotSatisfiable
    } else {
        RangeResult::Satisfiable { start, end }
    }
}

/// Whether an `If-None-Match: *` is present, which a GET answers with 304.
fn if_none_match_star(headers: &HeaderMap) -> bool {
    headers
        .get(IF_NONE_MATCH)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.trim() == "*")
}

/// Whether the file counts as not modified since the request's `If-Modified-Since`.
fn not_modified(headers: &HeaderMap, modtime: SystemTime) -> bool {
    if headers.contains_key(IF_NONE_MATCH) {
        return false;
    }
    match http_date(headers, &IF_MODIFIED_SINCE) {
        Some(since) => !modtime_after(modtime, since),
        None => false,
    }
}

/// Parses an HTTP-date request header.
fn http_date(headers: &HeaderMap, name: &axum::http::HeaderName) -> Option<SystemTime> {
    let text = headers.get(name)?.to_str().ok()?;
    httpdate::parse_http_date(text).ok()
}

/// Whether `modtime`, truncated to seconds, is strictly after `other`.
fn modtime_after(modtime: SystemTime, other: SystemTime) -> bool {
    truncate_to_seconds(modtime) > truncate_to_seconds(other)
}

/// Truncates a time to whole seconds since the epoch, as HTTP-date comparisons do.
fn truncate_to_seconds(time: SystemTime) -> u64 {
    time.duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}

/// Builds a header value from a string, falling back to empty on an impossible value.
fn header_value(text: &str) -> HeaderValue {
    HeaderValue::from_str(text).unwrap_or_else(|_| HeaderValue::from_static(""))
}

/// Percent-escapes a name for a URL path segment, matching Go's `url.PathEscape`, which
/// the original uses for the `Content-Disposition` file name.
fn path_escape(name: &str) -> String {
    let mut escaped = String::with_capacity(name.len());
    for &byte in name.as_bytes() {
        if is_unescaped_in_segment(byte) {
            escaped.push(char::from(byte));
        } else {
            escaped.push('%');
            escaped.push(hex_digit(byte >> 4));
            escaped.push(hex_digit(byte & 0x0f));
        }
    }
    escaped
}

/// Whether a byte is left unescaped in a URL path segment by Go's `url.PathEscape`.
fn is_unescaped_in_segment(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(byte, b'-' | b'_' | b'.' | b'~')
        || matches!(byte, b'$' | b'&' | b'+' | b':' | b'=' | b'@')
}

/// The upper-case hexadecimal digit of a nibble.
fn hex_digit(nibble: u8) -> char {
    char::from_digit(u32::from(nibble), 16).map_or('0', |digit| digit.to_ascii_uppercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_range_single_range() {
        assert_eq!(
            parse_range("bytes=0-7", 33),
            RangeResult::Satisfiable { start: 0, end: 7 },
            "a leading range"
        );
    }

    #[test]
    fn parse_range_past_the_end_is_not_satisfiable() {
        assert_eq!(
            parse_range("bytes=1000-2000", 33),
            RangeResult::NotSatisfiable,
            "a range past the end"
        );
    }

    #[test]
    fn parse_range_suffix_and_open_ends() {
        assert_eq!(
            parse_range("bytes=-5", 33),
            RangeResult::Satisfiable { start: 28, end: 32 },
            "the last five bytes"
        );
        assert_eq!(
            parse_range("bytes=10-", 33),
            RangeResult::Satisfiable { start: 10, end: 32 },
            "from an offset to the end"
        );
    }

    #[test]
    fn parse_range_clamps_the_end() {
        assert_eq!(
            parse_range("bytes=0-99999", 33),
            RangeResult::Satisfiable { start: 0, end: 32 },
            "an end past the file is clamped"
        );
    }

    #[test]
    fn parse_range_rejects_a_bad_unit_and_serves_multiple_whole() {
        assert_eq!(
            parse_range("lines=0-1", 33),
            RangeResult::Invalid,
            "a bad unit"
        );
        assert_eq!(
            parse_range("bytes=0-1,4-5", 33),
            RangeResult::Full,
            "more than one range serves the whole file"
        );
    }

    #[test]
    fn path_escape_matches_go() {
        assert_eq!(path_escape("My Pic.PNG"), "My%20Pic.PNG", "a space");
        assert_eq!(path_escape("plain.png"), "plain.png", "a plain name");
        assert_eq!(path_escape(r#"a"b.png"#), "a%22b.png", "a quote");
    }

    #[test]
    fn content_type_rules() {
        assert_eq!(
            content_type_for(""),
            "application/octet-stream",
            "unknown is octet-stream"
        );
        assert_eq!(
            content_type_for("text/html; charset=utf-8"),
            "text/plain",
            "an unsafe type is plain text"
        );
        assert_eq!(
            content_type_for("image/png"),
            "image/png",
            "a safe type is kept"
        );
    }
}
