//! File upload and download.
//!
//! Ports `server/api/files.go`. Downloads reproduce the headers and the range and
//! conditional handling of Go's `http.ServeContent`.

use axum::body::Body;
use std::io;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::Duration;
use std::time::SystemTime;

use axum::extract::multipart::{Field, MultipartRejection};
use axum::extract::{Multipart, Path};
use axum::http::header::{
    ACCEPT_RANGES, CACHE_CONTROL, CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_RANGE, CONTENT_TYPE,
    EXPECT, IF_MODIFIED_SINCE, IF_NONE_MATCH, IF_RANGE, IF_UNMODIFIED_SINCE, LAST_MODIFIED, RANGE,
};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::IntoResponse;
use futures_util::StreamExt as _;
use fvoci_model::{FileInfo, Problem};
use serde::Serialize;
use tokio_util::io::StreamReader;
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::contract::bodies::{Binary, FileForm};
use super::contract::params::MemberWorkspaceId;
use super::contract::params::{BoardId, FileId};
use super::contract::tag;
use super::workspace::InContainer;

use super::{ApiResult, created, json_response, require_board_permission, require_granted};
use crate::app::App;
use crate::app::FILE;
use crate::error::{AppError, Reason};
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

/// What an upload answers: the name the file is asked for by.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(super) struct UploadedFile {
    /// The identifier of the file: what `getFile` and `getFileInfo` take, and what an
    /// attachment block or an image of a body names in its `fileId`.
    pub(super) file_id: String,
}

/// The routes that read files.
pub(super) fn routes() -> OpenApiRouter<App> {
    OpenApiRouter::new()
        .routes(routes!(handle_serve_file))
        .routes(routes!(handle_get_file_info))
}

/// The route that takes a file, whose size has a limit of its own.
pub(super) fn upload_routes() -> OpenApiRouter<App> {
    OpenApiRouter::new().routes(routes!(handle_upload_file))
}

/// A file
///
/// The content of a file of the board, for the board's members. A file is asked for
/// through its own board only: through another board it is not there.
///
/// Every file is sent as `application/octet-stream` and offered as a download
/// (`Content-Disposition: attachment`) under its `fileId`, not under the name it was
/// uploaded with, which `getFileInfo` has. The answer honours `Range` with one range,
/// unless the request has `If-Range`. It may be cached by the browser of who asked but
/// must be revalidated (`Cache-Control: private, no-cache`), and revalidating saves no
/// download: the `Last-Modified` of an answer is the time of that answer, so a request
/// with the `If-Modified-Since` of an earlier answer gets the whole file again.
#[utoipa::path(
    get,
    path = "/w/{workspaceId}/boards/{boardId}/files/{fileId}",
    operation_id = "getFile",
    tag = tag::FILES,
    params(MemberWorkspaceId, BoardId, FileId),
    responses(
        (status = 200, description = "The file.", body = inline(Binary), content_type = "application/octet-stream"),
        (status = 206, description = "The range of the file that was asked for.", body = inline(Binary), content_type = "application/octet-stream"),
        (status = 304, description = "Only for a request with `If-None-Match: *`, or with an `If-Modified-Since` that is not before the time of the answer. No body."),
        (status = 412, description = "The request has an `If-Unmodified-Since` before the time of the answer.", body = Problem, content_type = "application/problem+json"),
        (status = 416, description = "The range is not in the file, or is no range. A range that is not in the file is answered with a `Content-Range` that tells the file's length.", body = Problem, content_type = "application/problem+json"),
    ),
)]
async fn handle_serve_file(
    InContainer(ws): InContainer,
    Path((_, board_id, file_id)): Path<(String, String, String)>,
    headers: HeaderMap,
) -> ApiResult {
    require_file_reader(&ws.app, &ws.session.user_id, &board_id).await?;
    serve_board_file(&ws.app, &board_id, &file_id, &headers).await
}

/// Checks that the user may read the files of the board.
async fn require_file_reader(app: &App, user_id: &str, board_id: &str) -> Result<(), AppError> {
    let grant = app.board_grant(user_id, board_id).await?;
    require_granted(grant, BoardPermission::ViewBoard, "access denied to board")
}

/// Sends a file of the board, as `getFile` describes it.
pub(super) async fn serve_board_file(
    app: &App,
    board_id: &str,
    file_id: &str,
    headers: &HeaderMap,
) -> ApiResult {
    // A file is its board's. Through another board it is as missing as a file that
    // does not exist: the answers must not tell the two apart.
    let Some(info) = app.file_info(board_id, file_id).await? else {
        return Err(AppError::not_found(FILE));
    };
    if !app.files().exists(&info.path).await? {
        return Err(AppError::not_found(FILE));
    }
    let size = app.files().size(&info.path).await?.unwrap_or(0);
    serve_content(app, file_id, &info.path, size, "", headers).await
}

/// What is known about a file
///
/// The name a file was uploaded under, its size and when it was uploaded. For the
/// board's members.
#[utoipa::path(
    get,
    path = "/w/{workspaceId}/boards/{boardId}/files/{fileId}/info",
    operation_id = "getFileInfo",
    tag = tag::FILES,
    params(MemberWorkspaceId, BoardId, FileId),
    responses((status = 200, description = "What is known about the file.", body = FileInfo)),
)]
async fn handle_get_file_info(
    InContainer(ws): InContainer,
    Path((_, board_id, file_id)): Path<(String, String, String)>,
) -> ApiResult {
    require_file_reader(&ws.app, &ws.session.user_id, &board_id).await?;
    Ok(json_response(
        &board_file_info(&ws.app, &board_id, file_id.as_str()).await?,
    ))
}

/// What is known about a file of the board; 404 for no record, and for the record of
/// another board's file.
pub(super) async fn board_file_info(
    app: &App,
    board_id: &str,
    file_id: &str,
) -> Result<FileInfo, AppError> {
    app.file_info(board_id, file_id)
        .await?
        .ok_or_else(|| AppError::not_found(FILE))
}

/// Upload a file
///
/// Stores a file with the board. The request is a `multipart/form-data` form whose
/// part `file` carries the file and a file name. The file is limited to one part of an
/// upload in parts (8 MiB unless the server is configured otherwise), and to the
/// `maxFileSize` of `getClientConfig` where that is less; a larger one is 413. Where
/// the limit is the part, the refusal has the reason `upload_in_parts`: a file within
/// `maxFileSize` can be sent with `createUpload`. A file that is refused or cut off is
/// not kept. The `fileId` of the answer ends in the extension of the file name, in lower
/// case, with `.jpeg` written `.jpg`; an extension of more than 17 bytes, or with
/// anything but letters and digits in it, is left off. For editors.
#[utoipa::path(
    post,
    path = "/w/{workspaceId}/boards/{boardId}/files",
    operation_id = "uploadFile",
    tag = tag::FILES,
    params(MemberWorkspaceId, BoardId),
    request_body(content = FileForm, content_type = "multipart/form-data"),
    responses((status = 201, description = "The file is stored.", body = UploadedFile)),
)]
async fn handle_upload_file(
    InContainer(ws): InContainer,
    Path((_, board_id)): Path<(String, String)>,
    headers: HeaderMap,
    multipart: Result<Multipart, MultipartRejection>,
) -> ApiResult {
    let user_id = ws.session.user_id.as_str();
    require_board_permission(
        &ws.app,
        user_id,
        &board_id,
        BoardPermission::ManageBoardCards,
        "access denied to make board changes",
    )
    .await?;
    let limit = SingleUploadLimit::of(&ws.app);
    let mut multipart = multipart.map_err(|_| AppError::bad_request(NOT_MULTIPART))?;
    let declared = headers
        .get(CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok());
    if declared.is_some_and(|length| length > limit.bytes.saturating_add(FORM_ALLOWANCE)) {
        // A client that waits for `100 Continue` has sent nothing yet, and reading the
        // form would invite it to. It gets its answer at once.
        let waits_to_send = headers
            .get(EXPECT)
            .is_some_and(|value| value.as_bytes().eq_ignore_ascii_case(b"100-continue"));
        if !waits_to_send {
            drain_form(&mut multipart).await;
        }
        return Err(limit.refusal());
    }

    let stored = loop {
        let field = multipart
            .next_field()
            .await
            .map_err(|error| AppError::bad_request(error.to_string()))?;
        let Some(field) = field else {
            break Err(AppError::bad_request(NO_FILE_PART).with_parameter(UPLOAD_FIELD));
        };
        if field.name() != Some(UPLOAD_FIELD) {
            continue;
        }
        let Some(filename) = field.file_name().map(str::to_owned) else {
            break Err(AppError::bad_request(NO_FILE_PART).with_parameter(UPLOAD_FIELD));
        };
        // The stored name is what a reader asks for the file by; it stays out of the log.
        tracing::debug!(filename, "uploadFile");
        break store_field(&ws.app, &board_id, &filename, field, limit).await;
    };
    match stored {
        Ok(file_id) => Ok(created(&UploadedFile { file_id })),
        Err(error) => {
            // The client may still be sending; it reads the refusal once it is done.
            drain_form(&mut multipart).await;
            Err(error)
        }
    }
}

/// What a form may carry besides the file: the boundaries and the part's headers. A
/// request that declares more than the file's limit and this is refused unread.
const FORM_ALLOWANCE: u64 = 64 * 1024;

/// How much of a refused form is read and thrown away before the refusal is sent, and
/// for how long, as [`super::limits`] does with other bodies.
const FORM_DISCARD_LIMIT: u64 = 8 * 1024 * 1024;
const FORM_DISCARD_TIMEOUT: Duration = Duration::from_secs(5);

/// How the reading of the file went, for telling why storing it failed. Kept in an
/// [`AtomicU8`]: the reader the storage takes must be `Send`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reading {
    /// Nothing went wrong with the reading.
    Fine,
    /// The file passed the limit.
    TooLarge,
    /// The form could not be read.
    Unreadable,
}

impl Reading {
    fn load(state: &AtomicU8) -> Self {
        match state.load(Ordering::Relaxed) {
            1 => Self::TooLarge,
            2 => Self::Unreadable,
            _ => Self::Fine,
        }
    }

    fn value(self) -> u8 {
        match self {
            Self::Fine => 0,
            Self::TooLarge => 1,
            Self::Unreadable => 2,
        }
    }

    fn store(self, state: &AtomicU8) {
        state.store(self.value(), Ordering::Relaxed);
    }
}

/// The largest file `uploadFile` takes, and how a larger one is refused.
#[derive(Debug, Clone, Copy)]
struct SingleUploadLimit {
    bytes: u64,
    /// Whether a larger file could be sent in parts.
    in_parts: bool,
}

impl SingleUploadLimit {
    /// One part of an upload in parts (`docs/s6-design.md` 5.6, 10), or the largest file
    /// where that is less.
    fn of(app: &App) -> Self {
        let part = app.config().upload_part_bytes;
        let largest = u64::try_from(app.limits().largest_file).unwrap_or(0);
        if part < largest {
            Self {
                bytes: part,
                in_parts: true,
            }
        } else {
            Self {
                bytes: largest,
                in_parts: false,
            }
        }
    }

    /// The refusal of a larger file: 413, with the reason `upload_in_parts` where the
    /// file could be sent in parts.
    fn refusal(self) -> AppError {
        let refusal = AppError::payload_too_large(self.bytes);
        if self.in_parts {
            refusal.with_reason(Reason::UploadInParts)
        } else {
            refusal
        }
    }
}

/// Stores the file a form's part carries, as `filename`, if it is within the limit.
async fn store_field(
    app: &App,
    board_id: &str,
    filename: &str,
    field: Field<'_>,
    limit: SingleUploadLimit,
) -> Result<String, AppError> {
    let state = AtomicU8::new(Reading::Fine.value());
    let mut received: u64 = 0;
    let bytes = field.map(|chunk| {
        let chunk = chunk.map_err(|error| {
            Reading::Unreadable.store(&state);
            io::Error::other(error)
        })?;
        received = received.saturating_add(u64::try_from(chunk.len()).unwrap_or(u64::MAX));
        if received > limit.bytes {
            Reading::TooLarge.store(&state);
            return Err(io::Error::other("the file is larger than the limit"));
        }
        Ok(chunk)
    });
    let mut reader = StreamReader::new(Box::pin(bytes));
    // What fails is not kept: the storage writes a file whole or not at all.
    let saved = app.save_file(&mut reader, board_id, filename).await;
    match (saved, Reading::load(&state)) {
        (Ok(file_id), _) => Ok(file_id),
        (Err(_), Reading::TooLarge) => Err(limit.refusal()),
        (Err(_), Reading::Unreadable) => Err(AppError::bad_request("the file could not be read")),
        (Err(error), Reading::Fine) => Err(error),
    }
}

/// Reads what is left of a refused form and throws it away, up to
/// [`FORM_DISCARD_LIMIT`] and for at most [`FORM_DISCARD_TIMEOUT`].
async fn drain_form(multipart: &mut Multipart) {
    let draining = async {
        let mut discarded: u64 = 0;
        while let Ok(Some(mut field)) = multipart.next_field().await {
            while let Ok(Some(chunk)) = field.chunk().await {
                discarded =
                    discarded.saturating_add(u64::try_from(chunk.len()).unwrap_or(u64::MAX));
                if discarded > FORM_DISCARD_LIMIT {
                    return;
                }
            }
        }
    };
    // Whether the form ended, was too long or too slow makes no difference to the answer.
    let _ = tokio::time::timeout(FORM_DISCARD_TIMEOUT, draining).await;
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
        return Err(AppError::precondition_failed(
            "the file is newer than the If-Unmodified-Since of the request",
        )
        .with_header("If-Unmodified-Since"));
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
            let mut response = range_error("the range is not in the file").into_response();
            // What the range could have been: the length of the file.
            response
                .headers_mut()
                .insert(CONTENT_RANGE, header_value(&format!("bytes */{size}")));
            Ok(response)
        }
        Some(RangeResult::Invalid) => Err(range_error("the Range header is no range of bytes")),
    }
}

/// The 416 for a `Range` the file cannot be served by.
fn range_error(message: &str) -> AppError {
    AppError::range_not_satisfiable(message).with_header("Range")
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
