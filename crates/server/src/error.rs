//! The error type of the application and API layers, and its HTTP form.
//!
//! Every error answer of the API has one body, [`ErrorBody`]: a stable `error` code that
//! follows from the status, a `message` for developers, and where a client needs them a
//! `reason`, the refused `field` or the exceeded `limit`.

use axum::Json;
use axum::http::header::RETRY_AFTER;
use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use fvoci_model::{
    AuthParamError, BlockError, BoardError, CardError, CategoryError, ErrorBody, SubscriptionError,
};
use fvoci_store::StoreError;

/// What kind of error a request ended with. The HTTP status and the `error` code of the
/// body follow from it.
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
    /// The request body is larger than allowed: 413.
    PayloadTooLarge,
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
            Self::PayloadTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            Self::RateLimited => StatusCode::TOO_MANY_REQUESTS,
            Self::Internal => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    /// The value of the body's `error` member.
    pub fn code(self) -> &'static str {
        match self {
            Self::BadRequest => "bad_request",
            Self::Unauthorized => "unauthorized",
            Self::Forbidden => "forbidden",
            Self::NotFound => "not_found",
            Self::MethodNotAllowed => "method_not_allowed",
            Self::Conflict => "conflict",
            Self::PayloadTooLarge => "payload_too_large",
            Self::RateLimited => "rate_limited",
            Self::Internal => "internal",
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
}

impl Reason {
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
        }
    }
}

/// The text every server error is answered with; what went wrong is in the log.
const INTERNAL_MESSAGE: &str = "internal server error";

/// An error of a request.
///
/// The message of every kind except [`ErrorKind::Internal`] is sent to the client.
#[derive(Debug)]
pub struct AppError {
    kind: ErrorKind,
    message: String,
    reason: Option<Reason>,
    field: Option<String>,
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
            field: None,
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

    /// A 413 error that names the limit, in bytes, that the request body exceeds.
    pub fn payload_too_large(limit: u64) -> Self {
        let mut error = Self::new(
            ErrorKind::PayloadTooLarge,
            format_args!("the request body is larger than {limit} bytes"),
        );
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

    /// Names the member of the request that was refused.
    #[must_use]
    pub fn with_field(mut self, field: &str) -> Self {
        self.field = Some(field.to_owned());
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

    /// The HTTP status this error is reported with.
    pub fn status(&self) -> StatusCode {
        self.kind.status()
    }

    /// The body this error is reported with, without the identifier of the request.
    pub fn body(&self) -> ErrorBody {
        ErrorBody {
            error: self.kind.code().to_owned(),
            message: self.message.clone(),
            reason: self.reason.map(|reason| reason.as_str().to_owned()),
            field: self.field.clone(),
            limit: self.limit,
            retry_after: self.retry_after,
            request_id: None,
        }
    }
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
            // A parent that is no block of the board, whether it is none at all or
            // another board's: the answer does not tell the two apart.
            StoreError::InvalidBlock {
                source:
                    source @ (BlockError::ParentNotOnBoard
                    | BlockError::ParentBelowBlock
                    | BlockError::BodyNotAllowed),
                ..
            } => Self::from(source),
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

/// The body of an error response as it left the handler, kept beside the response so
/// that the request log can add the identifier of the request to it.
#[derive(Debug, Clone)]
pub(crate) struct ReportedError(pub(crate) ErrorBody);

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = self.status();
        if let Some(cause) = &self.cause {
            tracing::error!(error = %format!("{cause:#}"), "API error");
        } else {
            tracing::debug!(status = status.as_u16(), error = %self.message, "API error");
        }
        let body = self.body();
        let mut response = (status, Json(&body)).into_response();
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
    async fn reported(error: AppError) -> (u16, ErrorBody) {
        let response = error.into_response();
        let status = response.status().as_u16();
        assert_eq!(
            response
                .headers()
                .get(CONTENT_TYPE)
                .and_then(|value| value.to_str().ok()),
            Some("application/json"),
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
    async fn every_kind_has_its_status_and_code() {
        let cases = [
            (
                AppError::bad_request("bad field"),
                400,
                "bad_request",
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
                "not_found",
                "board not found",
            ),
            (
                AppError::method_not_allowed(),
                405,
                "method_not_allowed",
                "method not allowed",
            ),
            (AppError::conflict("taken"), 409, "conflict", "taken"),
            (
                AppError::payload_too_large(64),
                413,
                "payload_too_large",
                "the request body is larger than 64 bytes",
            ),
            (
                AppError::internal("the disk is on fire at /var/lib"),
                500,
                "internal",
                "internal server error",
            ),
        ];
        for (error, status, code, message) in cases {
            let (got_status, body) = reported(error).await;
            assert_eq!(got_status, status, "the status of {code}");
            assert_eq!(body.error, code, "the code");
            assert_eq!(body.message, message, "the message of {code}");
            assert_eq!(body.request_id, None, "the request log adds the identifier");
        }
    }

    #[tokio::test]
    async fn reason_field_and_limit_are_in_the_body_only_when_set() {
        let (_, plain) = reported(AppError::bad_request("x")).await;
        assert_eq!(
            serde_json::to_value(&plain).expect("the body serializes"),
            serde_json::json!({"error": "bad_request", "message": "x"}),
            "nothing else"
        );

        let error = AppError::conflict("no admins")
            .with_reason(Reason::LastAdmin)
            .with_field("schemeAdmin");
        let (_, body) = reported(error).await;
        assert_eq!(body.reason.as_deref(), Some("last_admin"), "the reason");
        assert_eq!(body.field.as_deref(), Some("schemeAdmin"), "the field");

        let (_, body) = reported(AppError::payload_too_large(4096)).await;
        assert_eq!(body.limit, Some(4096), "the limit");
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
        assert_eq!(body.message, "internal server error", "the client does not");
    }

    /// Where an error came up is for the log. What a client is told stays as it is.
    #[tokio::test]
    async fn context_does_not_change_what_a_client_is_told() {
        let error = AppError::bad_request("no admins").context("removing a member");
        assert_eq!(error.kind(), ErrorKind::BadRequest);
        let (status, body) = reported(error).await;
        assert_eq!(status, 400);
        assert_eq!(
            body.message, "no admins",
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
            assert_eq!(body.error, "not_found");
            assert_eq!(body.message, "not found", "no identifier, no database text");
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
            assert_eq!(body.message, text, "the store's message says what is wrong");
        }
    }

    #[tokio::test]
    async fn a_user_the_store_does_not_find_is_not_found_without_the_name() {
        let (status, body) = reported(StoreError::UserNotFound("u1".to_owned()).into()).await;
        assert_eq!(status, 404);
        assert_eq!(body.message, "user not found");
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
            assert_eq!(body.message, "internal server error");
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
