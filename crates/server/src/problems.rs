//! The kinds of problem the API answers with, and the pages that describe them.
//!
//! The `type` of a problem is `/problems/<name>` under the path the server is served
//! at, and that address answers with a page of text about the kind. The list is made
//! from [`ErrorKind::ALL`] and [`Reason::ALL`], which is all there is: an error has the
//! type of its reason, or else of its kind.

use std::fmt::Write as _;

use axum::extract::{OriginalUri, Path};
use axum::http::HeaderValue;
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE, X_CONTENT_TYPE_OPTIONS};
use axum::response::{IntoResponse, Response};

use crate::error::{AppError, ErrorKind, PROBLEMS_PATH, Reason};

/// One kind of problem.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProblemType {
    /// The last part of the `type`: `not-found`, `last-admin`.
    pub name: String,
    /// The `title` of every problem of the kind.
    pub title: &'static str,
    /// The status every problem of the kind has; `None` for the kind of a `reason`,
    /// whose description says which statuses it comes with.
    pub status: Option<u16>,
    /// The `reason` every problem of the kind has; `None` for the kind of a status.
    pub reason: Option<&'static str>,
    /// What the kind means and what a client can do about it, in English.
    pub description: &'static str,
}

impl ProblemType {
    /// The `type` of the problems of this kind, on a server at the root of its host.
    pub fn type_reference(&self) -> String {
        format!("{PROBLEMS_PATH}/{}", self.name)
    }

    /// The page about the kind, for a server that sends `type_reference` as its `type`.
    fn page(&self, type_reference: &str) -> String {
        let mut page = format!("{}\n\ntype: {type_reference}\n", self.title);
        // Writing to a string cannot fail.
        if let Some(status) = self.status {
            let _ = writeln!(page, "status: {status}");
        }
        if let Some(reason) = self.reason {
            let _ = writeln!(page, "reason: {reason}");
        }
        let _ = writeln!(page, "\n{}", self.description);
        page
    }
}

impl From<ErrorKind> for ProblemType {
    fn from(kind: ErrorKind) -> Self {
        Self {
            name: kind.name().to_owned(),
            title: kind.title(),
            status: Some(kind.status().as_u16()),
            reason: None,
            description: describe_kind(kind),
        }
    }
}

impl From<Reason> for ProblemType {
    fn from(reason: Reason) -> Self {
        Self {
            name: reason.name(),
            title: reason.title(),
            status: None,
            reason: Some(reason.as_str()),
            description: describe_reason(reason),
        }
    }
}

/// Every kind of problem: those of the statuses, in their order, then those of the
/// reasons.
pub fn all() -> Vec<ProblemType> {
    let kinds = ErrorKind::ALL.into_iter().map(ProblemType::from);
    let reasons = Reason::ALL.into_iter().map(ProblemType::from);
    kinds.chain(reasons).collect()
}

/// The kind of problem with this name, if there is one.
pub fn find(name: &str) -> Option<ProblemType> {
    all().into_iter().find(|problem| problem.name == name)
}

fn describe_kind(kind: ErrorKind) -> &'static str {
    match kind {
        ErrorKind::BadRequest => {
            "The request is malformed or asks for what cannot be: a body that is not the JSON \
             the operation takes, or a member, a parameter or a header with a value it cannot \
             have. Where the refusal is about something of the request, `errors` lists it; a \
             check of the request's shape lists everything it refuses at once."
        }
        ErrorKind::Unauthorized => {
            "The request has no session, or its session is over. Sign in and send the request \
             again."
        }
        ErrorKind::Forbidden => {
            "The user can see what the request is about, but may not do this with it."
        }
        ErrorKind::NotFound => {
            "What the request is about does not exist, or the user may not know that it does: \
             the two are answered alike. A path that is no operation of the API is answered so \
             as well."
        }
        ErrorKind::MethodNotAllowed => {
            "The path exists, but not with this method. The `Allow` header lists the methods \
             it has."
        }
        ErrorKind::Conflict => {
            "The request is fine, but the state of things is against it: what it would make is \
             there already, or what it changes has changed since it was read. Read again and \
             decide anew."
        }
        ErrorKind::PreconditionFailed => {
            "A condition the request sets in its headers does not hold. The download of a \
             file answers so to an `If-Unmodified-Since` before the time of the answer."
        }
        ErrorKind::PayloadTooLarge => {
            "The body of the request, or what the request names, is larger than the server \
             takes. `limit` is the most it takes."
        }
        ErrorKind::RangeNotSatisfiable => {
            "The `Range` header of the request is no range of bytes, or the range is not in \
             the file. For a range that is not in the file, the `Content-Range` header of the \
             answer tells the file's length."
        }
        ErrorKind::RateLimited => {
            "Too many attempts. `retryAfter`, and the `Retry-After` header, say how many \
             seconds to wait before trying again."
        }
        ErrorKind::Internal => {
            "The server failed. The answer does not say how; `requestId` is the name of the \
             request in the server's log."
        }
    }
}

fn describe_reason(reason: Reason) -> &'static str {
    match reason {
        Reason::Csrf => {
            "Status 403. A request that changes something lacks the header `X-Requested-With: \
             XMLHttpRequest`, by which the server knows that no page of another site made a \
             browser send it. A request with a bearer token needs none."
        }
        Reason::ForeignOrigin => {
            "Status 403. The request, or the websocket handshake, has an `Origin` header that \
             is neither the server's own origin nor one the server was configured to accept."
        }
        Reason::InvalidCredentials => {
            "The user name, the email address or the password is wrong; the answer does not \
             say which. Status 401 from a login. Status 400 from a change of the password, \
             whose session is good: `errors` names `/oldPassword`."
        }
        Reason::SignupTokenRequired => {
            "Status 403. Only the first user of a server registers without the signup token, \
             and this registration has none."
        }
        Reason::SignupTokenInvalid => {
            "Status 403. The signup token of the registration is not the team's."
        }
        Reason::UsernameTaken => {
            "Status 409. Another user has the user name. `errors` names `/username`."
        }
        Reason::EmailTaken => {
            "Status 409. Another user has the email address. `errors` names `/email`."
        }
        Reason::LastAdmin => {
            "Status 409. The change would leave the board, or the server, without an \
             administrator. Make somebody else one first."
        }
        Reason::AdminRequired => "Status 403. Only an administrator of the server may do this.",
        Reason::SharingDisabled => {
            "Status 403. Sharing a board with whoever has its link is turned off in the \
             configuration of this server."
        }
        Reason::TooManyUploads => {
            "Status 429. The user has as many uploads in parts open as one user may. Complete \
             or abandon one first. No wait is told: what makes room is the user's own doing."
        }
        Reason::UploadsPending => {
            "Status 429. The user's open uploads in parts hold back as many bytes as one \
             user's may. Complete or abandon one first."
        }
        Reason::ServerUploadsPending => {
            "Status 429. The open uploads in parts of all users hold back as many bytes as the \
             server lets them. Try again later."
        }
        Reason::UploadInParts => {
            "Status 413. The file is larger than one request takes (`limit`), but not larger \
             than a file may be: send it as an upload in parts."
        }
        Reason::ExportRunning => {
            "Status 429. The user has an export running, and runs one at a time. Try again \
             when it is done."
        }
        Reason::ServerExportsBusy => {
            "Status 429. The server runs as many exports as it may at once. Try again later."
        }
        Reason::ImportRunning => {
            "Status 429. The user has an import running, and runs one at a time. Try again \
             when it is done."
        }
        Reason::ServerImportsBusy => {
            "Status 429. The server runs as many imports as it may at once. Try again later."
        }
    }
}

/// A page of plain text that anyone may read and a cache may keep for a while.
fn text_page(text: String) -> Response {
    (
        [
            (
                CONTENT_TYPE,
                HeaderValue::from_static("text/plain; charset=utf-8"),
            ),
            (X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff")),
            (CACHE_CONTROL, HeaderValue::from_static("no-cache")),
        ],
        text,
    )
        .into_response()
}

/// The page about the kind of problem `name`: what the `type` of a problem points at.
/// The address it was asked at, under the path the server is served at, is that `type`.
pub(crate) async fn handle_problem(
    OriginalUri(asked): OriginalUri,
    Path(name): Path<String>,
) -> Result<Response, AppError> {
    let problem = find(&name).ok_or_else(|| AppError::not_found("problem type"))?;
    Ok(text_page(problem.page(asked.path())))
}

/// The list of every kind of problem, a line for each: its `type` and its title.
pub(crate) async fn handle_problems(OriginalUri(asked): OriginalUri) -> Response {
    let mut page =
        "The kinds of problem this API answers with (RFC 9457). Each is described at its type.\n\n"
            .to_owned();
    for problem in all() {
        // Writing to a string cannot fail.
        let _ = writeln!(page, "{}/{}  {}", asked.path(), problem.name, problem.title);
    }
    text_page(page)
}

#[cfg(test)]
mod tests {
    use super::{all, find};
    use crate::error::{ErrorKind, Reason};

    #[test]
    fn every_kind_and_every_reason_is_a_problem_type_with_a_name_of_its_own() {
        let problems = all();
        assert_eq!(
            problems.len(),
            ErrorKind::ALL.len() + Reason::ALL.len(),
            "one for each"
        );
        for problem in &problems {
            assert_eq!(
                find(&problem.name).as_ref(),
                Some(problem),
                "found by its name, and the name is no other's"
            );
            assert!(
                problem
                    .name
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c == '-'),
                "a name is lower-case words with hyphens: {}",
                problem.name
            );
            assert!(!problem.title.is_empty() && !problem.description.is_empty());
            assert_ne!(
                problem.status.is_some(),
                problem.reason.is_some(),
                "of a status or of a reason: {}",
                problem.name
            );
        }
        assert_eq!(find("nothing"), None);
    }

    #[test]
    fn the_page_of_a_kind_says_its_type_title_and_what_it_means() {
        let page = find("last-admin")
            .expect("a kind")
            .page("/problems/last-admin");
        assert!(
            page.starts_with("Last administrator\n\ntype: /problems/last-admin\n"),
            "{page}"
        );
        assert!(page.contains("reason: last_admin\n"), "{page}");
        assert!(
            page.ends_with("\n\nStatus 409. The change would leave the board, or the server, without an administrator. Make somebody else one first.\n"),
            "{page}"
        );
        let page = find("not-found")
            .expect("a kind")
            .page("/base/problems/not-found");
        assert!(
            page.contains("type: /base/problems/not-found\nstatus: 404\n"),
            "{page}"
        );
        assert!(!page.contains("reason:"), "{page}");
    }
}
