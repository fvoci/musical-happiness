//! The error type of the application and API layers, and its HTTP form.
//!
//! Every error answer of the API has one body, the [`Problem`] of RFC 9457, sent as
//! `application/problem+json`: a `type` that names the kind of problem and is the same
//! for every problem of the kind, its `title`, the `status`, a `detail` for developers,
//! and where a client needs them a `reason`, what of the request was refused (`errors`)
//! or the exceeded `limit`.

use axum::body::Body;
use axum::extract::Request;
use axum::http::header::{CONTENT_ENCODING, CONTENT_LENGTH, CONTENT_TYPE, RETRY_AFTER};
use axum::http::{HeaderValue, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use fvoci_model::{
    AuthParamError, BlockError, BoardError, CardError, CategoryError, InvalidInput,
    PROBLEM_MEDIA_TYPE, Problem, SubscriptionError, json_pointer,
};
use fvoci_store::StoreError;

/// Where the server describes its kinds of problem: `/problems/<name>` is the `type`
/// of a problem, and the path of the page about it.
pub const PROBLEMS_PATH: &str = "/problems";

/// What kind of error a request ended with. The HTTP status follows from it, and the
/// `type` of the problem where the error has no [`Reason`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    /// The request is malformed or asks for something invalid: 400.
    BadRequest,
    /// The request carries no valid session, or the login failed: 401.
    Unauthorized,
    /// The user can see the thing but may not do this with it: 403.
    Forbidden,
    /// The thing does not exist, or the user may not know that it does: 404.
    NotFound,
    /// The path exists but not with this method: 405.
    MethodNotAllowed,
    /// The request is fine but the state is against it: 409.
    Conflict,
    /// A condition the request set on what it asks for does not hold: 412.
    PreconditionFailed,
    /// The request body is larger than allowed: 413.
    PayloadTooLarge,
    /// The range the request asks for is not in what it asks for: 416.
    RangeNotSatisfiable,
    /// Too many attempts; try again later: 429.
    RateLimited,
    /// The server's own fault: 500. The cause is logged and not sent to the client.
    Internal,
}

impl ErrorKind {
    /// The HTTP status of this kind of error.
    pub fn status(self) -> StatusCode {
        match self {
            Self::BadRequest => StatusCode::BAD_REQUEST,
            Self::Unauthorized => StatusCode::UNAUTHORIZED,
            Self::Forbidden => StatusCode::FORBIDDEN,
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::MethodNotAllowed => StatusCode::METHOD_NOT_ALLOWED,
            Self::Conflict => StatusCode::CONFLICT,
            Self::PreconditionFailed => StatusCode::PRECONDITION_FAILED,
            Self::PayloadTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            Self::RangeNotSatisfiable => StatusCode::RANGE_NOT_SATISFIABLE,
            Self::RateLimited => StatusCode::TOO_MANY_REQUESTS,
            Self::Internal => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    /// Every kind, in the order of the statuses.
    pub const ALL: [Self; 11] = [
        Self::BadRequest,
        Self::Unauthorized,
        Self::Forbidden,
        Self::NotFound,
        Self::MethodNotAllowed,
        Self::Conflict,
        Self::PreconditionFailed,
        Self::PayloadTooLarge,
        Self::RangeNotSatisfiable,
        Self::RateLimited,
        Self::Internal,
    ];

    /// The kind of error a status is of; `None` for a status the API does not answer
    /// with.
    pub fn of_status(status: StatusCode) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.status() == status)
    }

    /// The name of the problem an error of this kind is when it has no [`Reason`]: the
    /// last part of its `type`.
    pub fn name(self) -> &'static str {
        match self {
            Self::BadRequest => "bad-request",
            Self::Unauthorized => "unauthorized",
            Self::Forbidden => "forbidden",
            Self::NotFound => "not-found",
            Self::MethodNotAllowed => "method-not-allowed",
            Self::Conflict => "conflict",
            Self::PreconditionFailed => "precondition-failed",
            Self::PayloadTooLarge => "payload-too-large",
            Self::RangeNotSatisfiable => "range-not-satisfiable",
            Self::RateLimited => "rate-limited",
            Self::Internal => "internal",
        }
    }

    /// The `title` of that problem.
    pub fn title(self) -> &'static str {
        match self {
            Self::BadRequest => "Bad request",
            Self::Unauthorized => "No valid session",
            Self::Forbidden => "Forbidden",
            Self::NotFound => "Not found",
            Self::MethodNotAllowed => "Method not allowed",
            Self::Conflict => "Conflict",
            Self::PreconditionFailed => "Precondition failed",
            Self::PayloadTooLarge => "Payload too large",
            Self::RangeNotSatisfiable => "Range not satisfiable",
            Self::RateLimited => "Too many requests",
            Self::Internal => "Internal server error",
        }
    }
}

/// Why a request was refused, where clients must tell causes of one kind apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    /// The request lacks the header of the cross-site request check.
    Csrf,
    /// The request, or the websocket handshake, comes from a page of another origin.
    ForeignOrigin,
    /// The user name, email address or password of a login is wrong.
    InvalidCredentials,
    /// Registration needs the signup token, and the request has none.
    SignupTokenRequired,
    /// The signup token of a registration is not the server's.
    SignupTokenInvalid,
    /// The user name is taken.
    UsernameTaken,
    /// The email address is taken.
    EmailTaken,
    /// The change would leave the board, or the instance, without an administrator.
    LastAdmin,
    /// Only an administrator of the instance may do this.
    AdminRequired,
    /// Public sharing is turned off in the server's configuration.
    SharingDisabled,
    /// The user has as many uploads in parts open as one may.
    TooManyUploads,
    /// The user's open uploads in parts hold back as many bytes as one user's may.
    UploadsPending,
    /// The open uploads in parts of the whole server hold back as many bytes as they may.
    ServerUploadsPending,
    /// The file is larger than one request takes; it is to be sent in parts.
    UploadInParts,
    /// The user has an export running.
    ExportRunning,
    /// The server runs as many exports as it may.
    ServerExportsBusy,
    /// The user has an import running.
    ImportRunning,
    /// The server runs as many imports as it may.
    ServerImportsBusy,
}

impl Reason {
    /// Every reason. A reason that is not listed here has no page under
    /// [`PROBLEMS_PATH`]; an error with it panics in a debug build, as the tests are.
    pub const ALL: [Self; 18] = [
        Self::Csrf,
        Self::ForeignOrigin,
        Self::InvalidCredentials,
        Self::SignupTokenRequired,
        Self::SignupTokenInvalid,
        Self::UsernameTaken,
        Self::EmailTaken,
        Self::LastAdmin,
        Self::AdminRequired,
        Self::SharingDisabled,
        Self::TooManyUploads,
        Self::UploadsPending,
        Self::ServerUploadsPending,
        Self::UploadInParts,
        Self::ExportRunning,
        Self::ServerExportsBusy,
        Self::ImportRunning,
        Self::ServerImportsBusy,
    ];

    /// The name of the problem an error with this reason is, the last part of its
    /// `type`: the reason, written with hyphens.
    pub fn name(self) -> String {
        self.as_str().replace('_', "-")
    }

    /// The `title` of that problem.
    pub fn title(self) -> &'static str {
        match self {
            Self::Csrf => "Cross-site request check failed",
            Self::ForeignOrigin => "Request from another origin",
            Self::InvalidCredentials => "Invalid credentials",
            Self::SignupTokenRequired => "Signup token required",
            Self::SignupTokenInvalid => "Signup token not valid",
            Self::UsernameTaken => "User name taken",
            Self::EmailTaken => "Email address taken",
            Self::LastAdmin => "Last administrator",
            Self::AdminRequired => "Administrator required",
            Self::SharingDisabled => "Public sharing turned off",
            Self::TooManyUploads => "Too many open uploads",
            Self::UploadsPending => "Open uploads hold too much",
            Self::ServerUploadsPending => "The server's open uploads hold too much",
            Self::UploadInParts => "File to be uploaded in parts",
            Self::ExportRunning => "An export is running",
            Self::ServerExportsBusy => "The server is busy exporting",
            Self::ImportRunning => "An import is running",
            Self::ServerImportsBusy => "The server is busy importing",
        }
    }

    /// The value of the body's `reason` member.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Csrf => "csrf",
            Self::ForeignOrigin => "foreign_origin",
            Self::InvalidCredentials => "invalid_credentials",
            Self::SignupTokenRequired => "signup_token_required",
            Self::SignupTokenInvalid => "signup_token_invalid",
            Self::UsernameTaken => "username_taken",
            Self::EmailTaken => "email_taken",
            Self::LastAdmin => "last_admin",
            Self::AdminRequired => "admin_required",
            Self::SharingDisabled => "sharing_disabled",
            Self::TooManyUploads => "too_many_uploads",
            Self::UploadsPending => "uploads_pending",
            Self::ServerUploadsPending => "server_uploads_pending",
            Self::UploadInParts => "upload_in_parts",
            Self::ExportRunning => "export_running",
            Self::ServerExportsBusy => "server_exports_busy",
            Self::ImportRunning => "import_running",
            Self::ServerImportsBusy => "server_imports_busy",
        }
    }
}

/// The text every server error is answered with; what went wrong is in the log.
const INTERNAL_MESSAGE: &str = "internal server error";

/// The `reason` of what was refused when neither the error nor the check that refused
/// it tells a finer one.
const INVALID: &str = "invalid";

/// Where in a request something that was refused is.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Place {
    /// In the body, at this JSON Pointer.
    Body(String),
    /// The parameter of this name, of the query or of the path.
    Parameter(String),
    /// The header of this name.
    Header(String),
}

/// What the store refused the members of.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Subject {
    /// The block with this identifier.
    Block(String),
    /// A board; the store does not say which.
    Board,
}

/// One thing of a request that was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Refused {
    place: Place,
    /// What is wrong with it; the message of the error if `None`.
    detail: Option<String>,
    /// Why; the reason of the error, or [`INVALID`], if `None`.
    reason: Option<&'static str>,
}

/// An error of a request.
///
/// The message of every kind except [`ErrorKind::Internal`] is sent to the client.
#[derive(Debug)]
pub struct AppError {
    kind: ErrorKind,
    message: String,
    reason: Option<Reason>,
    /// What of the request was refused, where the error is about something of it.
    refused: Vec<Refused>,
    /// What the refused members are of, where the store refused a block or a board: a
    /// request with a list of them says with it which one, or that it cannot say
    /// ([`AppError::among_blocks`]). Never sent.
    subject: Option<Subject>,
    limit: Option<u64>,
    /// How many seconds to wait before trying again.
    retry_after: Option<u64>,
    /// The cause of a server error, for the log.
    cause: Option<anyhow::Error>,
}

impl std::fmt::Display for AppError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.cause {
            Some(cause) => write!(formatter, "{cause:#}"),
            None => formatter.write_str(&self.message),
        }
    }
}

impl std::error::Error for AppError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.cause.as_ref().map(|cause| {
            let cause: &(dyn std::error::Error + Send + Sync + 'static) = cause.as_ref();
            let cause: &(dyn std::error::Error + 'static) = cause;
            cause
        })
    }
}

impl AppError {
    fn new(kind: ErrorKind, message: impl std::fmt::Display) -> Self {
        Self {
            kind,
            message: message.to_string(),
            reason: None,
            refused: Vec::new(),
            subject: None,
            limit: None,
            retry_after: None,
            cause: None,
        }
    }

    /// A 429 error: too many attempts, try again after `wait`. The wait is told in
    /// whole seconds, rounded up, in the `Retry-After` header and in the body.
    pub fn rate_limited(wait: std::time::Duration) -> Self {
        let seconds = wait.as_secs() + u64::from(wait.subsec_nanos() > 0);
        let mut error = Self::new(ErrorKind::RateLimited, "too many attempts; try again later");
        error.retry_after = Some(seconds.max(1));
        error
    }

    /// A 429 error for a limit on what is held at once, rather than on attempts: no
    /// wait is told, since what frees room is the client's own doing or another's.
    pub fn limited(message: impl std::fmt::Display, reason: Reason) -> Self {
        Self::new(ErrorKind::RateLimited, message).with_reason(reason)
    }

    /// A 400 error with the given message.
    pub fn bad_request(message: impl std::fmt::Display) -> Self {
        Self::new(ErrorKind::BadRequest, message)
    }

    /// A 401 error with the given message.
    pub fn unauthorized(message: impl std::fmt::Display) -> Self {
        Self::new(ErrorKind::Unauthorized, message)
    }

    /// A 403 error with the given message.
    pub fn forbidden(message: impl std::fmt::Display) -> Self {
        Self::new(ErrorKind::Forbidden, message)
    }

    /// A 404 error for a kind of thing, such as `board`: `board not found`.
    ///
    /// The message names the kind and nothing else, so that it is the same for a thing
    /// that does not exist and for one the requester may not know about.
    pub fn not_found(what: impl std::fmt::Display) -> Self {
        Self::new(ErrorKind::NotFound, format_args!("{what} not found"))
    }

    /// A 405 error.
    pub fn method_not_allowed() -> Self {
        Self::new(ErrorKind::MethodNotAllowed, "method not allowed")
    }

    /// A 409 error with the given message.
    pub fn conflict(message: impl std::fmt::Display) -> Self {
        Self::new(ErrorKind::Conflict, message)
    }

    /// A 412 error with the given message.
    pub fn precondition_failed(message: impl std::fmt::Display) -> Self {
        Self::new(ErrorKind::PreconditionFailed, message)
    }

    /// A 416 error with the given message.
    pub fn range_not_satisfiable(message: impl std::fmt::Display) -> Self {
        Self::new(ErrorKind::RangeNotSatisfiable, message)
    }

    /// A 413 error that names the limit, in bytes, that the request body exceeds.
    pub fn payload_too_large(limit: u64) -> Self {
        let mut error = Self::new(
            ErrorKind::PayloadTooLarge,
            format_args!("the request body is larger than {limit} bytes"),
        );
        error.limit = Some(limit);
        error
    }

    /// A 413 error for what the request names, not its body, being over `limit`: an
    /// archive's entries, for one.
    pub fn too_large(message: impl std::fmt::Display, limit: u64) -> Self {
        let mut error = Self::new(ErrorKind::PayloadTooLarge, message);
        error.limit = Some(limit);
        error
    }

    /// A 500 error with the given cause, which is logged and not sent to the client.
    pub fn internal(cause: impl std::fmt::Display) -> Self {
        Self::from(anyhow::anyhow!("{cause}"))
    }

    /// Adds the stable reason a client can act on.
    #[must_use]
    pub fn with_reason(mut self, reason: Reason) -> Self {
        self.reason = Some(reason);
        self
    }

    /// Names the member of the request body that was refused, by its name: a member of
    /// the object the body is.
    #[must_use]
    pub fn with_field(self, field: &str) -> Self {
        self.with_pointer(json_pointer(field))
    }

    /// Names what of the request body was refused by a JSON Pointer to it, for what is
    /// deeper than a member of the body: `/blocks/0/title`.
    #[must_use]
    pub fn with_pointer(self, pointer: impl Into<String>) -> Self {
        self.refusing(Place::Body(pointer.into()))
    }

    /// Names the parameter of the query or of the path that was refused.
    #[must_use]
    pub fn with_parameter(self, name: &str) -> Self {
        self.refusing(Place::Parameter(name.to_owned()))
    }

    /// Names the request header that was refused.
    #[must_use]
    pub fn with_header(self, name: &str) -> Self {
        self.refusing(Place::Header(name.to_owned()))
    }

    /// Says that what was refused in the body is under `prefix`, a JSON Pointer: for a
    /// request that carries a list of what the error is about, `/blocks/2`.
    #[must_use]
    pub fn under(mut self, prefix: &str) -> Self {
        for refused in &mut self.refused {
            if let Place::Body(pointer) = &mut refused.place {
                pointer.insert_str(0, prefix);
            }
        }
        self
    }

    /// Says where in a request with a list of blocks the members are that the store
    /// refused of one of them: under `list` (a JSON Pointer, empty if the body is the
    /// list) at the place its identifier has among `ids`. A refusal of a block that is
    /// not among them, or of one of the request's boards, names no member: where it is
    /// cannot be said. Any other error is left as it is.
    #[must_use]
    pub fn among_blocks<'a>(mut self, ids: impl IntoIterator<Item = &'a str>, list: &str) -> Self {
        let index = match self.subject.take() {
            None => return self,
            Some(Subject::Block(block)) => ids.into_iter().position(|id| id == block),
            Some(Subject::Board) => None,
        };
        if let Some(index) = index {
            return self.under(&format!("{list}/{index}"));
        }
        self.refused
            .retain(|refused| !matches!(refused.place, Place::Body(_)));
        self
    }

    /// Says that what was refused at the JSON Pointer `from` is at `to` in the request:
    /// for a request that names a member otherwise than what it changes does, as a
    /// patch's `updatedProperties` are a board's `properties`.
    #[must_use]
    pub fn moved(mut self, from: &str, to: &str) -> Self {
        for refused in &mut self.refused {
            if refused.place == Place::Body(from.to_owned()) {
                refused.place = Place::Body(to.to_owned());
            }
        }
        self
    }

    fn refusing(mut self, place: Place) -> Self {
        self.refused.push(Refused {
            place,
            detail: None,
            reason: None,
        });
        self
    }

    /// Names the limit the request went over: bytes for a body, a count for a
    /// collection.
    #[must_use]
    pub fn with_limit(mut self, limit: u64) -> Self {
        self.limit = Some(limit);
        self
    }

    /// Says where the error came up, for the log. A server error carries it in its
    /// cause. What a client is told does not change: the words of the app layer about
    /// its own steps are not for clients.
    #[must_use]
    pub fn context(mut self, context: impl std::fmt::Display) -> Self {
        if let Some(cause) = self.cause.take() {
            self.cause = Some(cause.context(context.to_string()));
        } else {
            tracing::debug!(%context, error = %self.message, "API error");
        }
        self
    }

    /// The kind of this error.
    pub fn kind(&self) -> ErrorKind {
        self.kind
    }

    /// Returns `true` if this is a 404 error.
    pub fn is_not_found(&self) -> bool {
        self.kind == ErrorKind::NotFound
    }

    /// Returns `true` if this error refuses the requester (401, 403 or 404) rather than
    /// failing: what an access check answers to a user who may not do what they ask.
    pub fn is_refusal(&self) -> bool {
        matches!(
            self.kind,
            ErrorKind::NotFound | ErrorKind::Forbidden | ErrorKind::Unauthorized
        )
    }

    /// The HTTP status this error is reported with.
    pub fn status(&self) -> StatusCode {
        self.kind.status()
    }

    /// The body this error is reported with, without the identifier of the request.
    pub fn body(&self) -> Problem {
        let (name, title) = match self.reason {
            Some(reason) => (reason.name(), reason.title()),
            None => (self.kind.name().to_owned(), self.kind.title()),
        };
        debug_assert!(
            self.reason
                .is_none_or(|reason| Reason::ALL.contains(&reason)),
            "{:?} is not in Reason::ALL",
            self.reason
        );
        let reason = self.reason.map(Reason::as_str);
        let errors = self
            .refused
            .iter()
            .map(|refused| {
                let mut input = InvalidInput {
                    detail: refused.detail.as_ref().unwrap_or(&self.message).clone(),
                    reason: refused.reason.or(reason).unwrap_or(INVALID).to_owned(),
                    ..InvalidInput::default()
                };
                match &refused.place {
                    Place::Body(pointer) => input.pointer = Some(pointer.clone()),
                    Place::Parameter(name) => input.parameter = Some(name.clone()),
                    Place::Header(name) => input.header = Some(name.clone()),
                }
                input
            })
            .collect();
        Problem {
            problem_type: format!("{PROBLEMS_PATH}/{name}"),
            title: title.to_owned(),
            status: self.status().as_u16(),
            detail: self.message.clone(),
            instance: None,
            reason: reason.map(str::to_owned),
            errors,
            limit: self.limit,
            retry_after: self.retry_after,
            request_id: None,
        }
    }
}

/// The response with `problem` as its body, as `application/problem+json`.
pub(crate) fn problem_response(problem: &Problem) -> Response {
    // A problem is a handful of strings and numbers; serializing it cannot fail. If it
    // ever did, the status alone still says what kind of error it is.
    let body = serde_json::to_vec(problem).unwrap_or_default();
    let status = StatusCode::from_u16(problem.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let mut response = (status, Body::from(body)).into_response();
    response
        .headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static(PROBLEM_MEDIA_TYPE));
    response
}

impl From<anyhow::Error> for AppError {
    fn from(cause: anyhow::Error) -> Self {
        Self {
            cause: Some(cause),
            ..Self::new(ErrorKind::Internal, INTERNAL_MESSAGE)
        }
    }
}

impl From<StoreError> for AppError {
    fn from(error: StoreError) -> Self {
        match error {
            // What was not found is in the store's message, which names identifiers; the
            // answer does not repeat it. Routes say what kind of thing they miss.
            StoreError::NotFound(_) | StoreError::NotAllFound { .. } | StoreError::NoRows => {
                tracing::debug!(%error, "not found in the store");
                Self::new(ErrorKind::NotFound, "not found")
            }
            // More client state than one account keeps.
            StoreError::TooManyPreferences { limit } => Self::bad_request(error)
                .with_field("updatedFields")
                .with_limit(u64::try_from(limit).unwrap_or(u64::MAX)),
            // A registration without the signup token that was not the first after all.
            StoreError::NotFirstUser => Self::forbidden("registration needs the signup token")
                .with_reason(Reason::SignupTokenRequired)
                .with_field("token"),
            // The one lookup that names its kind: a password is set for a user.
            StoreError::UserNotFound(_) => {
                tracing::debug!(%error, "not found in the store");
                Self::not_found("user")
            }
            // What a client sent and the store refused.
            StoreError::InvalidBlock {
                source:
                    BlockError::TitleSizeLimitExceeded
                    | BlockError::FieldsSizeLimitExceeded
                    | BlockError::InvalidCardProperties(_),
                ..
            }
            | StoreError::InvalidBoard(BoardError::InvalidCardProperty(_))
            | StoreError::InvalidBoardsAndBlocks(_)
            | StoreError::BlockNotInDeletedBoards { .. }
            | StoreError::InvalidCategory(_)
            | StoreError::InvalidSubscription(_) => Self::bad_request(error),
            StoreError::InvalidBoard(BoardError::NulCharacter { member }) => {
                let mut error = Self::bad_request(error).with_field(member);
                error.subject = Some(Subject::Board);
                error
            }
            // A parent that is no block of the board, whether it is none at all or
            // another board's: the answer does not tell the two apart.
            StoreError::InvalidBlock {
                id,
                source:
                    source @ (BlockError::ParentNotOnBoard
                    | BlockError::ParentBelowBlock
                    | BlockError::BodyNotAllowed
                    | BlockError::NulInTitle
                    | BlockError::InvalidCardField { .. }
                    | BlockError::TypeNotCurrent(_)),
            } => {
                let mut error = Self::from(source);
                error.subject = Some(Subject::Block(id));
                error
            }
            // What a request names by something that is no identifier does not exist.
            // The text is not echoed: it is the client's own, and it may be long.
            StoreError::InvalidId(_) => {
                tracing::debug!(%error, "a write named something that is no identifier");
                Self::bad_request("an identifier in the request is not a valid identifier")
            }
            StoreError::Conflict(_) => Self::conflict(error),
            // The message names no times: the client has the time it read the block
            // at, and reads the block again for the one it has now.
            StoreError::Stale { .. } => {
                Self::conflict("the block has changed since it was read").context(error)
            }
            other => Self::from(anyhow::Error::from(other)),
        }
    }
}

impl From<AuthParamError> for AppError {
    fn from(error: AuthParamError) -> Self {
        let field = error.field();
        Self::bad_request(error).with_field(field)
    }
}

impl From<CategoryError> for AppError {
    fn from(error: CategoryError) -> Self {
        Self::bad_request(error)
    }
}

impl From<BlockError> for AppError {
    fn from(error: BlockError) -> Self {
        match error {
            BlockError::TitleSizeLimitExceeded
            | BlockError::FieldsSizeLimitExceeded
            | BlockError::InvalidCardProperties(_) => Self::bad_request(error),
            BlockError::BodyNotAllowed => Self::bad_request(error).with_field("body"),
            BlockError::NulInTitle => Self::bad_request(error).with_field("title"),
            BlockError::InvalidCardField { .. } => Self::bad_request(error).with_field("fields"),
            BlockError::TypeNotCurrent(_) => Self::bad_request(error).with_field("type"),
            BlockError::ParentNotOnBoard | BlockError::ParentBelowBlock => {
                Self::bad_request(error).with_field("parentId")
            }
            BlockError::EmptyBoardId => Self::from(anyhow::Error::from(error)),
        }
    }
}

impl From<CardError> for AppError {
    fn from(error: CardError) -> Self {
        match error {
            // What a client sent.
            CardError::BoardIdMismatch | CardError::Invalid(_) => Self::bad_request(error),
            CardError::ReadAtMissing => Self::bad_request(error).with_field("readAt"),
            // What is stored cannot be read as a card.
            CardError::NotCardBlock | CardError::InvalidFieldType { .. } => {
                Self::from(anyhow::Error::from(error))
            }
        }
    }
}

impl From<SubscriptionError> for AppError {
    fn from(error: SubscriptionError) -> Self {
        Self::bad_request(error)
    }
}

/// Answers in the API's own format what was refused before a handler ran, by the
/// framework: a parameter of the path that is no text, for one. Such an answer comes
/// with the framework's words as plain text; it becomes the problem of its status,
/// with words of this server, and keeps its other headers. A status the API has no
/// kind for becomes 400 if it is the client's and 500 if not.
pub(crate) async fn problems_only(request: Request, next: Next) -> Response {
    let response = next.run(request).await;
    let status = response.status();
    let is_error = status.is_client_error() || status.is_server_error();
    if !is_error || response.extensions().get::<ReportedError>().is_some() {
        return response;
    }
    let kind = ErrorKind::of_status(status).unwrap_or(if status.is_client_error() {
        ErrorKind::BadRequest
    } else {
        ErrorKind::Internal
    });
    let message = match kind {
        ErrorKind::BadRequest => "the request is malformed",
        ErrorKind::Internal => INTERNAL_MESSAGE,
        _ => status
            .canonical_reason()
            .unwrap_or("the request was refused"),
    };
    tracing::debug!(
        status = status.as_u16(),
        "an answer that was no problem was made one"
    );
    let (parts, _) = response.into_parts();
    let mut problem = AppError::new(kind, message).into_response();
    for (name, value) in &parts.headers {
        if ![CONTENT_TYPE, CONTENT_LENGTH, CONTENT_ENCODING].contains(name) {
            problem.headers_mut().insert(name, value.clone());
        }
    }
    problem
}

/// The body of an error response as it left the handler, kept beside the response so
/// that the request log can add the identifier of the request to it.
#[derive(Debug, Clone)]
pub(crate) struct ReportedError(pub(crate) Problem);

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = self.status();
        if let Some(cause) = &self.cause {
            tracing::error!(error = %format!("{cause:#}"), "API error");
        } else {
            tracing::debug!(status = status.as_u16(), error = %self.message, "API error");
        }
        let body = self.body();
        let mut response = problem_response(&body);
        if let Some(seconds) = body.retry_after {
            response
                .headers_mut()
                .insert(RETRY_AFTER, HeaderValue::from(seconds));
        }
        response.extensions_mut().insert(ReportedError(body));
        response
    }
}

#[cfg(test)]
mod tests {
    use axum::http::header::CONTENT_TYPE;
    use fvoci_model::Category;

    use super::*;

    /// Turns the error into its response and returns the status and the body.
    async fn reported(error: AppError) -> (u16, Problem) {
        let response = error.into_response();
        let status = response.status().as_u16();
        assert_eq!(
            response
                .headers()
                .get(CONTENT_TYPE)
                .and_then(|value| value.to_str().ok()),
            Some("application/problem+json"),
            "the content type"
        );
        assert!(
            response.extensions().get::<ReportedError>().is_some(),
            "the body is kept beside the response for the request log"
        );
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("the body is read");
        (
            status,
            serde_json::from_slice(&body).expect("the body is JSON"),
        )
    }

    #[tokio::test]
    async fn every_kind_has_its_status_type_and_title() {
        let cases = [
            (
                AppError::bad_request("bad field"),
                400,
                "bad-request",
                "bad field",
            ),
            (
                AppError::unauthorized("no session"),
                401,
                "unauthorized",
                "no session",
            ),
            (
                AppError::forbidden("not yours"),
                403,
                "forbidden",
                "not yours",
            ),
            (
                AppError::not_found("board"),
                404,
                "not-found",
                "board not found",
            ),
            (
                AppError::method_not_allowed(),
                405,
                "method-not-allowed",
                "method not allowed",
            ),
            (AppError::conflict("taken"), 409, "conflict", "taken"),
            (
                AppError::precondition_failed("changed"),
                412,
                "precondition-failed",
                "changed",
            ),
            (
                AppError::payload_too_large(64),
                413,
                "payload-too-large",
                "the request body is larger than 64 bytes",
            ),
            (
                AppError::range_not_satisfiable("no such bytes"),
                416,
                "range-not-satisfiable",
                "no such bytes",
            ),
            (
                AppError::rate_limited(std::time::Duration::from_millis(1500)),
                429,
                "rate-limited",
                "too many attempts; try again later",
            ),
            (
                AppError::internal("the disk is on fire at /var/lib"),
                500,
                "internal",
                "internal server error",
            ),
        ];
        assert_eq!(cases.len(), ErrorKind::ALL.len(), "a case for every kind");
        for (error, status, name, detail) in cases {
            let kind = error.kind();
            let (got_status, body) = reported(error).await;
            assert_eq!(got_status, status, "the status of {name}");
            assert_eq!(body.status, status, "the body repeats the status");
            assert_eq!(body.problem_type, format!("/problems/{name}"), "the type");
            assert_eq!(body.title, kind.title(), "the title of {name}");
            assert_eq!(body.detail, detail, "the detail of {name}");
            assert_eq!(body.request_id, None, "the request log adds the identifier");
            assert_eq!(body.instance, None, "and the instance");
        }
    }

    #[tokio::test]
    async fn an_error_with_a_reason_is_the_problem_of_that_reason() {
        for reason in Reason::ALL {
            let forbidden = AppError::forbidden("x").with_reason(reason);
            let conflict = AppError::conflict("y").with_reason(reason);
            let (status, body) = reported(forbidden).await;
            let (other_status, other) = reported(conflict).await;
            assert_eq!((status, other_status), (403, 409));
            let name = reason.as_str().replace('_', "-");
            assert_eq!(body.problem_type, format!("/problems/{name}"));
            assert_eq!(
                (&other.problem_type, &other.title),
                (&body.problem_type, &body.title),
                "the same type and title whatever the status and the detail"
            );
            assert_eq!(body.title, reason.title());
            assert_eq!(body.reason.as_deref(), Some(reason.as_str()));
        }
    }

    #[test]
    fn no_two_problems_have_the_same_name() {
        let mut names: Vec<String> = ErrorKind::ALL
            .iter()
            .map(|kind| kind.name().to_owned())
            .chain(Reason::ALL.iter().map(|reason| reason.name()))
            .collect();
        let count = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), count, "{names:?}");
    }

    #[tokio::test]
    async fn reason_errors_and_limit_are_in_the_body_only_when_set() {
        let (_, plain) = reported(AppError::bad_request("x")).await;
        assert_eq!(
            serde_json::to_value(&plain).expect("the body serializes"),
            serde_json::json!({
                "type": "/problems/bad-request", "title": "Bad request", "status": 400,
                "detail": "x"
            }),
            "nothing else"
        );

        let error = AppError::conflict("no admins")
            .with_field("schemeAdmin")
            .with_reason(Reason::LastAdmin);
        let (_, body) = reported(error).await;
        assert_eq!(body.reason.as_deref(), Some("last_admin"), "the reason");
        assert_eq!(
            serde_json::to_value(&body.errors).expect("the entries serialize"),
            serde_json::json!([
                { "pointer": "/schemeAdmin", "detail": "no admins", "reason": "last_admin" }
            ]),
            "the member, with the detail and the reason of the problem"
        );

        let (_, body) = reported(AppError::payload_too_large(4096)).await;
        assert_eq!(body.limit, Some(4096), "the limit");
    }

    #[tokio::test]
    async fn a_block_the_store_refused_is_found_in_the_list_of_the_request() {
        let refused = || {
            AppError::from(StoreError::InvalidBlock {
                id: "b2".to_owned(),
                source: BlockError::NulInTitle,
            })
        };
        let pointers = |error: AppError| -> Vec<Option<String>> {
            error
                .body()
                .errors
                .into_iter()
                .map(|entry| entry.pointer)
                .collect()
        };
        assert_eq!(
            pointers(refused()),
            [Some("/title".to_owned())],
            "a request that is the block"
        );
        assert_eq!(
            pointers(refused().among_blocks(["b1", "b2"], "/blockPatches")),
            [Some("/blockPatches/1/title".to_owned())]
        );
        assert_eq!(
            pointers(refused().among_blocks(["b1", "b2"], "")),
            [Some("/1/title".to_owned())],
            "the body is the list"
        );
        assert_eq!(
            pointers(refused().among_blocks(["b1"], "")),
            [],
            "a block the request does not list: nowhere to point"
        );
        let board = || {
            AppError::from(StoreError::InvalidBoard(BoardError::NulCharacter {
                member: "properties",
            }))
        };
        assert_eq!(
            pointers(board().moved("/properties", "/updatedProperties")),
            [Some("/updatedProperties".to_owned())],
            "a patch names the member its own way"
        );
        assert_eq!(
            pointers(board().among_blocks(["b1"], "/blocks")),
            [],
            "one of several boards: nowhere to point"
        );
        let other = AppError::bad_request("no").with_field("blockIds");
        assert_eq!(
            pointers(other.among_blocks(["b1"], "/blockPatches")),
            [Some("/blockIds".to_owned())],
            "an error that is about no block stays"
        );
    }

    #[tokio::test]
    async fn what_was_refused_is_told_by_where_it_is_in_the_request() {
        let error = AppError::bad_request("no")
            .with_field("a/b")
            .with_pointer("/blocks/0/title")
            .with_parameter("page")
            .with_header("Content-Digest");
        let (_, body) = reported(error).await;
        assert_eq!(
            serde_json::to_value(&body.errors).expect("the entries serialize"),
            serde_json::json!([
                { "pointer": "/a~1b", "detail": "no", "reason": "invalid" },
                { "pointer": "/blocks/0/title", "detail": "no", "reason": "invalid" },
                { "parameter": "page", "detail": "no", "reason": "invalid" },
                { "header": "Content-Digest", "detail": "no", "reason": "invalid" },
            ])
        );
    }

    #[tokio::test]
    async fn a_server_error_does_not_say_what_went_wrong() {
        let error = AppError::internal("connection to 10.0.0.5 refused").context("saving");
        assert_eq!(
            error.to_string(),
            "saving: connection to 10.0.0.5 refused",
            "the log gets the cause"
        );
        let (status, body) = reported(error).await;
        assert_eq!(status, 500);
        assert_eq!(body.detail, "internal server error", "the client does not");
        assert_eq!(body.problem_type, "/problems/internal");
    }

    /// Where an error came up is for the log. What a client is told stays as it is.
    #[tokio::test]
    async fn context_does_not_change_what_a_client_is_told() {
        let error = AppError::bad_request("no admins").context("removing a member");
        assert_eq!(error.kind(), ErrorKind::BadRequest);
        let (status, body) = reported(error).await;
        assert_eq!(status, 400);
        assert_eq!(
            body.detail, "no admins",
            "the app layer's words are not sent"
        );

        let error = AppError::internal("disk full").context("writing a file");
        assert!(
            error.to_string().contains("writing a file"),
            "a server error carries it for the log: {error}"
        );
    }

    #[tokio::test]
    async fn what_the_store_does_not_find_is_not_found_without_the_identifier() {
        for error in [
            StoreError::NotFound("board ID=bsecret".to_owned()),
            StoreError::NoRows,
            StoreError::NotAllFound {
                entity: "block",
                ids: vec!["csecret".to_owned()],
            },
        ] {
            let (status, body) = reported(error.into()).await;
            assert_eq!(status, 404);
            assert_eq!(body.problem_type, "/problems/not-found");
            assert_eq!(body.detail, "not found", "no identifier, no database text");
        }
    }

    #[tokio::test]
    async fn what_a_client_sent_and_the_store_refused_is_a_bad_request() {
        let category = Category::default()
            .validate()
            .expect_err("an empty category");
        let block = |source| StoreError::InvalidBlock {
            id: "c1".to_owned(),
            source,
        };
        let errors = [
            StoreError::from(category),
            block(BlockError::TitleSizeLimitExceeded),
            block(BlockError::FieldsSizeLimitExceeded),
            StoreError::InvalidBoardsAndBlocks(fvoci_model::BoardsAndBlocksError::NoBoards),
        ];
        for error in errors {
            let text = error.to_string();
            let (status, body) = reported(error.into()).await;
            assert_eq!(status, 400, "{text}");
            assert_eq!(body.detail, text, "the store's message says what is wrong");
        }
    }

    #[tokio::test]
    async fn a_user_the_store_does_not_find_is_not_found_without_the_name() {
        let (status, body) = reported(StoreError::UserNotFound("u1".to_owned()).into()).await;
        assert_eq!(status, 404);
        assert_eq!(body.detail, "user not found");
    }

    #[tokio::test]
    async fn any_other_store_error_is_a_server_error() {
        let errors = [
            block_without_board(),
            StoreError::Config("no such file".to_owned()),
        ];
        for error in errors {
            let (status, body) = reported(error.into()).await;
            assert_eq!(status, 500);
            assert_eq!(body.detail, "internal server error");
        }
    }

    fn block_without_board() -> StoreError {
        StoreError::InvalidBlock {
            id: "c1".to_owned(),
            source: BlockError::EmptyBoardId,
        }
    }

    #[test]
    fn card_errors_are_the_clients_or_the_servers() {
        assert_eq!(
            AppError::from(CardError::BoardIdMismatch).kind(),
            ErrorKind::BadRequest,
            "a card for another board"
        );
        assert_eq!(
            AppError::from(CardError::Invalid("Icon can have only one grapheme")).kind(),
            ErrorKind::BadRequest,
            "an invalid card"
        );
        assert_eq!(
            AppError::from(CardError::NotCardBlock).kind(),
            ErrorKind::Internal,
            "a stored block that is no card"
        );
    }
}
