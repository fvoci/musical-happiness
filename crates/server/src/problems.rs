//! The kinds of problem the API answers with, and the pages that describe them.
//!
//! The `type` of a problem is `/problems/<name>` under the path the server is served
//! at, and that address answers with a page of text about the kind. The list is made
//! from [`ErrorKind::ALL`] and [`Reason::ALL`], which is all there is: an error has the
//! type of its reason, or else of its kind.

use std::fmt::Write as _;

use axum::Router;
use axum::extract::{OriginalUri, Path};
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE, X_CONTENT_TYPE_OPTIONS};
use axum::http::{HeaderValue, Uri};
use axum::middleware;
use axum::response::{IntoResponse, Response};
use axum::routing::get;

use crate::error::{AppError, ErrorKind, PROBLEMS_PATH, Reason, type_reference};
use crate::error_response::problems_only;

/// What a kind of problem is the kind of: every error either has a reason, and is the
/// problem of that reason whatever its status, or has none and is the problem of its
/// status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Basis {
    /// The errors of this status that have no reason.
    Status(u16),
    /// The errors with this `reason`; the description says which statuses they come
    /// with.
    Reason(&'static str),
}

/// One kind of problem.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProblemType {
    /// The last part of the `type`: `not-found`, `last-admin`.
    pub name: String,
    /// The `title` of every problem of the kind.
    pub title: &'static str,
    /// What the kind is the kind of.
    pub basis: Basis,
    /// What the kind means and what a client can do about it, in English.
    pub description: &'static str,
}

impl ProblemType {
    /// The `type` of the problems of this kind, on a server at the root of its host.
    pub fn type_reference(&self) -> String {
        type_reference(&self.name)
    }

    /// The page about the kind, on a server that is served under `base_path`.
    fn page(&self, base_path: &str) -> String {
        let mut page = format!(
            "{}\n\ntype: {base_path}{}\n",
            self.title,
            self.type_reference()
        );
        // Writing to a string cannot fail.
        let _ = match self.basis {
            Basis::Status(status) => writeln!(page, "status: {status}"),
            Basis::Reason(reason) => writeln!(page, "reason: {reason}"),
        };
        let _ = writeln!(page, "\n{}", self.description);
        page
    }
}

impl From<ErrorKind> for ProblemType {
    fn from(kind: ErrorKind) -> Self {
        Self {
            name: kind.name().to_owned(),
            title: kind.title(),
            basis: Basis::Status(kind.status()),
            description: describe_kind(kind),
        }
    }
}

impl From<Reason> for ProblemType {
    fn from(reason: Reason) -> Self {
        Self {
            name: reason.name(),
            title: reason.title(),
            basis: Basis::Reason(reason.as_str()),
            description: describe_reason(reason),
        }
    }
}

/// Every kind of problem: those of the statuses, in their order, then those of the
/// reasons.
pub fn all() -> Vec<ProblemType> {
    let kinds = ErrorKind::ALL.into_iter().map(ProblemType::from);
    let reasons = Reason::ALL.iter().copied().map(ProblemType::from);
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
            "The request has no session, or its session is over: sign in and send the request \
             again. What a shared board lets be read without a session answers so to a \
             request without the board's read token, and the websocket endpoint to a \
             handshake that has no `Origin` header and no bearer token."
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
        ErrorKind::ServiceUnavailable => {
            "The server cannot serve for now: what it depends on does not answer. The \
             readiness check (`GET /api/v1/ready`) answers so while the database or the \
             file storage cannot be used; `detail` names which."
        }
    }
}

/// The description of [`Reason::OidcUnavailable`].
const OIDC_LOGIN_PAGE_UNAVAILABLE: &str = "Status 409 at the start of a sign-in through an \
    organization's provider; on the login page after one, whose reasons come back in the \
    `oidc_error` parameter of its address, by these names. This one: the organization has \
    no provider that is on, or the provider could not be reached, or it was switched off \
    or changed while the person was signing in; or the sign-in cannot end for a reason of \
    the server's: the client secret does not open with its key, the account of the \
    identity was deleted or changed meanwhile, no user name was free, or the server \
    failed. The server's log says which.";

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
             and this registration has none. `errors` names `/token`."
        }
        Reason::SignupTokenInvalid => {
            "Status 403. The signup token of the registration is not the workspace's. `errors` \
             names `/token`."
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
        Reason::GuestRoleLimit => "Status 403. A guest of a workspace has no role above editor.",
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
            "Status 413. The file is larger than one request takes (`limit`): send it as an \
             upload in parts, which takes a file up to the largest the server stores."
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
        Reason::SecretKeyRequired
        | Reason::OidcIssuerUnverified
        | Reason::OidcUnavailable
        | Reason::OidcStateInvalid
        | Reason::OidcDenied
        | Reason::OidcTokenRefused
        | Reason::OidcTokenInvalid
        | Reason::OidcEmailMissing
        | Reason::OidcEmailUnverified
        | Reason::OidcEmailTaken
        | Reason::OidcJoinClosed => describe_sign_in(reason).unwrap_or_default(),
        Reason::RelationNotAList
        | Reason::RelationToItself
        | Reason::RelationRepeated
        | Reason::RelationTooMany
        | Reason::RelationNotACard
        | Reason::RelationCardNotFound => describe_relation(reason).unwrap_or_default(),
        Reason::UpdateRefused
        | Reason::UpdateOutOfStep
        | Reason::UpdateTooLarge
        | Reason::DocumentTooLarge
        | Reason::TooManyDocuments
        | Reason::DocumentUnavailable => describe_editing_together(reason),
        Reason::DocumentReadOnly
        | Reason::ConnectionTooSlow
        | Reason::TooManyDocumentsJoined
        | Reason::DocumentFull
        | Reason::TooManyFrames => describe_rooms(reason),
    }
}

/// The descriptions of the reasons a value of a card relation is refused for.
fn describe_relation(reason: Reason) -> Option<&'static str> {
    Some(match reason {
        Reason::RelationNotAList => {
            "Status 400. The value of a card relation property (of the `type` `relation`) is \
             `null` or a list of identifiers of cards of the board. `errors` names where the \
             request has the card's values (a block's `fields` or `updatedFields`, a card's \
             `properties` or `updatedProperties`); the words name the property."
        }
        Reason::RelationToItself => {
            "Status 400. A card relation names its own card. `errors` as for \
             `relation-not-a-list`."
        }
        Reason::RelationRepeated => {
            "Status 400. A card relation names a card twice. `errors` as for \
             `relation-not-a-list`."
        }
        Reason::RelationTooMany => {
            "Status 400. A card relation names more cards than one may (`limit`) and one it \
             did not name before. `errors` as for `relation-not-a-list`."
        }
        Reason::RelationNotACard => {
            "Status 400. A card relation names a block of the board that is no card. \
             `errors` as for `relation-not-a-list`."
        }
        Reason::RelationCardNotFound => {
            "Status 400. A card relation names no card of the board: none of that identifier, \
             one removed from the trash, or another board's, which the answer does not tell \
             apart. A card in the trash may be named, and what a value named before is not \
             asked for again. `errors` as for `relation-not-a-list`."
        }
        _ => return None,
    })
}

/// What the reasons of a body edited together mean.
fn describe_editing_together(reason: Reason) -> &'static str {
    match reason {
        Reason::UpdateRefused => {
            "Status 400. The document of a card's body that is edited together does not take \
             the update: it is no update, writes outside the body, or would leave a document \
             that is no body. The document is as it was."
        }
        Reason::UpdateOutOfStep => {
            "Status 409. The update builds on changes the document of the body does not \
             have: the editor that made it holds another document, or one that was made \
             again. Load the document again and make the change there."
        }
        Reason::UpdateTooLarge => {
            "Status 413. The update is larger than the server takes (setting \
             `collab_update_bytes`); `limit` is the limit in bytes. Send the change in \
             smaller updates."
        }
        Reason::DocumentTooLarge => {
            "Status 413. With the update the document of the body, or the body, would be \
             larger than it may be (setting `collab_state_bytes`, and the limit of a body); \
             `limit` is the limit in bytes."
        }
        Reason::TooManyDocuments => {
            "Status 503. The server holds as many documents of bodies being edited as it \
             may (settings `collab_workers`, `collab_documents_per_worker`, \
             `collab_worker_memory_bytes`). Try again when others are closed."
        }
        Reason::DocumentUnavailable => {
            "Status 503. The document of the body cannot be worked on for now: the process \
             that holds the documents ended or did not answer in time, which drops what was \
             under way, or did so again and again with this document, which is closed for \
             a while; or a write of the card's whole body came while what its editors typed \
             could not be written to the card first, and was not done. The document is as \
             it was stored before. Try again later."
        }
        other => unreachable!("{other:?} is no reason of editing together"),
    }
}

/// What the reasons mean that only the rooms of the websocket give (`DOC_REFUSED`,
/// `DOC_NACK`, `DOC_LEFT`).
fn describe_rooms(reason: Reason) -> &'static str {
    match reason {
        Reason::DocumentReadOnly => {
            "Over the websocket only (`DOC_NACK`). The connection is in the room of the \
             card's body to read: its user reads the card's board and may not change its \
             cards. The update went to nobody; the editor that made it holds a change the \
             document does not, and comes into the room anew to drop it."
        }
        Reason::ConnectionTooSlow => {
            "Over the websocket only (`DOC_LEFT`, `DOC_REFUSED`). As many messages wait \
             for the connection as may (settings `collab_queue_messages`, and twice \
             `collab_state_bytes` in bytes), and it was put out of the room, or not let \
             in, so that the others go on. The frames that waited for it of the room \
             are dropped. Come into the room again: the whole document is sent anew."
        }
        Reason::TooManyDocumentsJoined => {
            "Over the websocket only (`DOC_REFUSED`). The connection is in as many rooms \
             as one connection may be (setting `collab_documents_per_connection`). Leave \
             one, or open another connection."
        }
        Reason::DocumentFull => {
            "Over the websocket only (`DOC_REFUSED`). As many connections are in the room \
             of the card's body as may be (setting `collab_connections_per_document`). \
             Try again when one has left."
        }
        Reason::TooManyFrames => {
            "Over the websocket only (`DOC_REFUSED`, `DOC_LEFT`). The connection sent the \
             rooms more than it may: as many frames a second as the setting \
             `collab_frames_per_second` says, and as many at once after a pause, a \
             `JOIN_DOC` and a sync step 1 counting as five. A `JOIN_DOC` past it is \
             refused; a frame past it puts the connection out of the room the frame was \
             for, since the room no longer has what the connection sent. Send less often, \
             and come in again."
        }
        other => unreachable!("{other:?} is no reason of the rooms"),
    }
}

/// The descriptions of the reasons of signing in through a provider.
fn describe_sign_in(reason: Reason) -> Option<&'static str> {
    Some(match reason {
        Reason::SecretKeyRequired => {
            "Status 409. The server seals the client secret of a provider with its key, \
             `FVOCI_SECRET_KEY`, and has none: a provider cannot be stored until the server is \
             started with one."
        }
        Reason::OidcIssuerUnverified => {
            "Status 400. The discovery document of the issuer could not be read, or does not \
             name the issuer exactly, or its endpoints are not `https`. The server reaches \
             only `https` addresses, follows no redirect, and reaches only public addresses \
             unless its configuration lets it reach others. `errors` names `/issuer`; \
             `detail` says which of these it was, and nothing of the provider's answer."
        }
        Reason::OidcUnavailable => OIDC_LOGIN_PAGE_UNAVAILABLE,
        Reason::OidcStateInvalid => {
            "On the login page, after a sign-in through a provider. What came back is no \
             sign-in this browser started, or one that was used already or took longer than \
             ten minutes, or it came back to the address of another organization than the one \
             it was started for, or from another issuer (`iss`). Start again from the login \
             page."
        }
        Reason::OidcDenied => {
            "On the login page, after a sign-in through a provider: the provider sent the \
             person back without letting them sign in."
        }
        Reason::OidcTokenRefused => {
            "On the login page, after a sign-in through a provider: the provider did not \
             give an ID token for the code, or could not be reached for it."
        }
        Reason::OidcTokenInvalid => {
            "On the login page, after a sign-in through a provider: its ID token does not \
             hold (signature, algorithm, key, issuer, audience, time, nonce). The server's \
             log says which."
        }
        Reason::OidcEmailMissing => {
            "On the login page, after a sign-in through a provider: the person has no \
             account yet, and the provider gave no email address to make one with."
        }
        Reason::OidcEmailUnverified => {
            "On the login page, after a sign-in through a provider: the person has no \
             account yet, and the provider says it has not checked their email address."
        }
        Reason::OidcEmailTaken => {
            "On the login page, after a sign-in through a provider: the person has no \
             account through the provider, and an account with a password has the email \
             address the provider gave as its own. The two are not joined by their \
             address: sign in with the password of that account. (Connecting a provider to \
             an account that has one is not there yet.) An address a provider gave for \
             another account is no account's own, and refuses nobody."
        }
        Reason::OidcJoinClosed => {
            "On the login page, after a sign-in through a provider: the person is no member \
             of the organization, and the organization does not let newcomers in through \
             its provider."
        }
        _ => return None,
    })
}

/// How long a cache may keep a page: the pages change with a release of the server.
const PAGE_CACHE_CONTROL: &str = "public, max-age=3600";

/// A page of plain text that anyone may read and a cache may keep for a while.
fn text_page(text: String) -> Response {
    (
        [
            (
                CONTENT_TYPE,
                HeaderValue::from_static("text/plain; charset=utf-8"),
            ),
            (X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff")),
            (CACHE_CONTROL, HeaderValue::from_static(PAGE_CACHE_CONTROL)),
        ],
        text,
    )
        .into_response()
}

/// The path the server is served under, from the address a page of this module was
/// asked at: what comes before [`PROBLEMS_PATH`].
fn base_path_of(asked: &Uri) -> &str {
    let path = asked.path();
    path.rfind(PROBLEMS_PATH).map_or("", |at| &path[..at])
}

/// The routes of the pages: the list at [`PROBLEMS_PATH`] and a page for each kind
/// below it. A name that is no text is refused as a problem, like any other error.
pub(crate) fn routes<S: Clone + Send + Sync + 'static>() -> Router<S> {
    Router::new()
        .route(PROBLEMS_PATH, get(handle_problems))
        .route(&format!("{PROBLEMS_PATH}/{{name}}"), get(handle_problem))
        .layer(middleware::from_fn(problems_only))
}

/// The page about the kind of problem `name`: what the `type` of a problem points at.
async fn handle_problem(
    OriginalUri(asked): OriginalUri,
    Path(name): Path<String>,
) -> Result<Response, AppError> {
    let problem = find(&name).ok_or_else(|| AppError::not_found("problem type"))?;
    Ok(text_page(problem.page(base_path_of(&asked))))
}

/// The list of every kind of problem, a line for each: its `type` and its title.
async fn handle_problems(OriginalUri(asked): OriginalUri) -> Response {
    let base_path = base_path_of(&asked);
    let mut page =
        "The kinds of problem this API answers with (RFC 9457). Each is described at its type.\n\n"
            .to_owned();
    for problem in all() {
        // Writing to a string cannot fail.
        let _ = writeln!(
            page,
            "{base_path}{}  {}",
            problem.type_reference(),
            problem.title
        );
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
        }
        assert_eq!(find("nothing"), None);
    }

    #[test]
    fn the_page_of_a_kind_says_its_type_title_and_what_it_means() {
        let page = find("last-admin").expect("a kind").page("");
        assert!(
            page.starts_with("Last administrator\n\ntype: /problems/last-admin\n"),
            "{page}"
        );
        assert!(page.contains("reason: last_admin\n"), "{page}");
        assert!(
            page.ends_with("\n\nStatus 409. The change would leave the board, or the server, without an administrator. Make somebody else one first.\n"),
            "{page}"
        );
        let page = find("not-found").expect("a kind").page("/base");
        assert!(
            page.contains("type: /base/problems/not-found\nstatus: 404\n"),
            "{page}"
        );
        assert!(!page.contains("reason:"), "{page}");
    }
}
