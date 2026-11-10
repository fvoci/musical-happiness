//! The error type of the application and API layers, and its HTTP form.

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use fvoci_model::{
    AuthParamError, BlockError, CardError, CategoryError, ErrorResponse, SubscriptionError,
};
use fvoci_store::StoreError;
use thiserror::Error;

/// An error of a request, classified by the HTTP status it is reported with.
///
/// The message of every variant except [`AppError::Internal`] is sent to the client as
/// is, so the messages of the original server are kept word for word.
#[derive(Debug, Error)]
pub enum AppError {
    /// The request is malformed or asks for something invalid: 400.
    #[error("{0}")]
    BadRequest(String),
    /// The request carries no valid credentials: 401.
    #[error("{0}")]
    Unauthorized(String),
    /// The user may not do this: 403.
    #[error("{0}")]
    Forbidden(String),
    /// The record does not exist: 404.
    #[error("{0}")]
    NotFound(String),
    /// The request body is larger than allowed: 413.
    #[error("{0}")]
    RequestEntityTooLarge(String),
    /// The operation is not available on this server: 501.
    #[error("{0}")]
    NotImplemented(String),
    /// Anything else: 500. The cause is logged and not sent to the client.
    #[error("{0:#}")]
    Internal(#[from] anyhow::Error),
}

impl AppError {
    /// A 400 error with the given message.
    pub fn bad_request(message: impl std::fmt::Display) -> Self {
        Self::BadRequest(message.to_string())
    }

    /// A 401 error with the given message.
    pub fn unauthorized(message: impl std::fmt::Display) -> Self {
        Self::Unauthorized(message.to_string())
    }

    /// A 403 error with the given message.
    pub fn forbidden(message: impl std::fmt::Display) -> Self {
        Self::Forbidden(message.to_string())
    }

    /// A 404 error for the named record, worded like the store's own: `{entity} not found`.
    pub fn not_found(entity: impl std::fmt::Display) -> Self {
        Self::NotFound(format!("{{{entity}}} not found"))
    }

    /// A 501 error with the given message.
    pub fn not_implemented(message: impl std::fmt::Display) -> Self {
        Self::NotImplemented(message.to_string())
    }

    /// A 500 error with the given cause, which is logged and not sent to the client.
    pub fn internal(cause: impl std::fmt::Display) -> Self {
        Self::Internal(anyhow::anyhow!("{cause}"))
    }

    /// Puts `context` before the message, as `context: message`, keeping the status.
    ///
    /// The original wraps errors this way on their way up, and the wrapped text is what
    /// clients see.
    #[must_use]
    pub fn context(self, context: &str) -> Self {
        let wrap = |message: String| format!("{context}: {message}");
        match self {
            Self::BadRequest(message) => Self::BadRequest(wrap(message)),
            Self::Unauthorized(message) => Self::Unauthorized(wrap(message)),
            Self::Forbidden(message) => Self::Forbidden(wrap(message)),
            Self::NotFound(message) => Self::NotFound(wrap(message)),
            Self::RequestEntityTooLarge(message) => Self::RequestEntityTooLarge(wrap(message)),
            Self::NotImplemented(message) => Self::NotImplemented(wrap(message)),
            Self::Internal(cause) => Self::Internal(cause.context(context.to_owned())),
        }
    }

    /// Returns `true` if this is a 404 error.
    pub fn is_not_found(&self) -> bool {
        matches!(self, Self::NotFound(_))
    }

    /// The HTTP status this error is reported with.
    pub fn status(&self) -> StatusCode {
        match self {
            Self::BadRequest(_) => StatusCode::BAD_REQUEST,
            Self::Unauthorized(_) => StatusCode::UNAUTHORIZED,
            Self::Forbidden(_) => StatusCode::FORBIDDEN,
            Self::NotFound(_) => StatusCode::NOT_FOUND,
            Self::RequestEntityTooLarge(_) => StatusCode::PAYLOAD_TOO_LARGE,
            Self::NotImplemented(_) => StatusCode::NOT_IMPLEMENTED,
            Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

impl From<StoreError> for AppError {
    fn from(error: StoreError) -> Self {
        match error {
            // The original counts its database layer's "no rows" among the "not found"
            // errors (`server/model/errors.go` `IsErrNotFound`), message and all.
            StoreError::NotFound(_) | StoreError::NotAllFound { .. } | StoreError::NoRows => {
                Self::NotFound(error.to_string())
            }
            // Only the two size limits are client errors; an empty board id is not.
            StoreError::InvalidBlock {
                source: BlockError::TitleSizeLimitExceeded | BlockError::FieldsSizeLimitExceeded,
                ..
            }
            | StoreError::InvalidCategory(_) => Self::BadRequest(error.to_string()),
            other => Self::Internal(other.into()),
        }
    }
}

impl From<AuthParamError> for AppError {
    fn from(error: AuthParamError) -> Self {
        Self::BadRequest(error.to_string())
    }
}

impl From<CategoryError> for AppError {
    fn from(error: CategoryError) -> Self {
        Self::BadRequest(error.to_string())
    }
}

impl From<BlockError> for AppError {
    fn from(error: BlockError) -> Self {
        match error {
            BlockError::TitleSizeLimitExceeded | BlockError::FieldsSizeLimitExceeded => {
                Self::BadRequest(error.to_string())
            }
            BlockError::EmptyBoardId => Self::Internal(error.into()),
        }
    }
}

impl From<CardError> for AppError {
    fn from(error: CardError) -> Self {
        match error {
            CardError::BoardIdMismatch => Self::BadRequest(error.to_string()),
            other => Self::Internal(other.into()),
        }
    }
}

impl From<SubscriptionError> for AppError {
    fn from(error: SubscriptionError) -> Self {
        Self::Internal(error.into())
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = self.status();
        let message = match &self {
            Self::Internal(cause) => {
                tracing::error!(error = %format!("{cause:#}"), "API error");
                "internal server error".to_owned()
            }
            other => {
                tracing::debug!(status = status.as_u16(), error = %other, "API error");
                other.to_string()
            }
        };
        let body = ErrorResponse {
            error: message,
            error_code: status.as_u16(),
        };
        (status, Json(body)).into_response()
    }
}

#[cfg(test)]
mod tests {
    use axum::http::header::CONTENT_TYPE;
    use fvoci_model::Category;

    use super::*;

    /// The body of `TestErrorResponse` (`server/api/api_test.go`): the error is answered
    /// with the status, as JSON, with a message that contains `message`.
    async fn check_error_response(error: AppError, status: u16, message: &str) {
        let response = error.into_response();
        assert_eq!(response.status().as_u16(), status, "the status");
        assert_eq!(
            response
                .headers()
                .get(CONTENT_TYPE)
                .and_then(|value| value.to_str().ok()),
            Some("application/json"),
            "the content type"
        );
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("the body is read");
        let body: ErrorResponse = serde_json::from_slice(&body).expect("the body is JSON");
        assert!(body.error.contains(message), "the message: {body:?}");
        assert_eq!(body.error_code, status, "the status in the body");
    }

    /// `TestErrorResponse/"ErrBadRequest should be a 400 code"`
    #[tokio::test]
    async fn error_response_err_bad_request_should_be_a_400_code() {
        check_error_response(AppError::bad_request("bad field"), 400, "bad field").await;
    }

    /// `TestErrorResponse/"ErrAuthParam should be a 400 code"`
    #[tokio::test]
    async fn error_response_err_auth_param_should_be_a_400_code() {
        let error = AuthParamError::new("password is required");
        check_error_response(error.into(), 400, "password is required").await;
    }

    /// `TestErrorResponse/"ErrInvalidCategory should be a 400 code"`. The error comes
    /// from validating a category, here one without an identifier, directly and on its
    /// way through the store.
    #[tokio::test]
    async fn error_response_err_invalid_category_should_be_a_400_code() {
        let invalid = || {
            Category::default()
                .validate()
                .expect_err("an empty category is invalid")
        };
        let message = invalid().to_string();
        assert!(!message.is_empty(), "the error has a message");
        check_error_response(invalid().into(), 400, &message).await;
        check_error_response(StoreError::from(invalid()).into(), 400, &message).await;
    }

    /// `TestErrorResponse/"ErrBoardMemberIsLastAdmin should be a 400 code"`. The port has
    /// no error type for it; the call site builds the bad request.
    #[tokio::test]
    async fn error_response_err_board_member_is_last_admin_should_be_a_400_code() {
        let error = AppError::bad_request("cannot leave a board with no admins");
        check_error_response(error, 400, "no admins").await;
    }

    /// `TestErrorResponse/"ErrBoardIDMismatch should be a 400 code"`
    #[tokio::test]
    async fn error_response_err_board_id_mismatch_should_be_a_400_code() {
        let error = CardError::BoardIdMismatch;
        check_error_response(error.into(), 400, "Board IDs do not match").await;
    }

    /// `TestErrorResponse/"ErrBlockTitleSizeLimitExceeded should be a 400 code"`
    #[tokio::test]
    async fn error_response_err_block_title_size_limit_exceeded_should_be_a_400_code() {
        let error = BlockError::TitleSizeLimitExceeded;
        check_error_response(error.into(), 400, "block title size limit exceeded").await;
    }

    /// `TestErrorResponse/"ErrBlockFieldsSizeLimitExceeded should be a 400 code"`
    #[tokio::test]
    async fn error_response_err_block_fields_size_limit_exceeded_should_be_a_400_code() {
        let error = BlockError::FieldsSizeLimitExceeded;
        check_error_response(error.into(), 400, "block fields size limit exceeded").await;
    }

    /// `TestErrorResponse/"ErrUnauthorized should be a 401 code"`
    #[tokio::test]
    async fn error_response_err_unauthorized_should_be_a_401_code() {
        let error = AppError::unauthorized("not enough permissions");
        check_error_response(error, 401, "not enough permissions").await;
    }

    /// `TestErrorResponse/"ErrForbidden should be a 403 code"` and
    /// `"ErrPermission should be a 403 code"`: the port has one error for both.
    #[tokio::test]
    async fn error_response_err_forbidden_and_err_permission_should_be_a_403_code() {
        let error = AppError::forbidden("not enough permissions");
        check_error_response(error, 403, "not enough permissions").await;
    }

    /// `TestErrorResponse/"ErrCategoryPermissionDenied should be a 403 code"`. The port
    /// has no error type for it; the call site builds the refusal.
    #[tokio::test]
    async fn error_response_err_category_permission_denied_should_be_a_403_code() {
        let error = AppError::forbidden("category doesn't belong to user");
        check_error_response(error, 403, "doesn't belong to user").await;
    }

    /// `TestErrorResponse/"ErrNotFound should be a 404 code"`, built here and by the store.
    #[tokio::test]
    async fn error_response_err_not_found_should_be_a_404_code() {
        check_error_response(AppError::not_found("board"), 404, "{board} not found").await;
        let error = StoreError::NotFound("board".to_owned());
        check_error_response(error.into(), 404, "{board} not found").await;
    }

    /// `TestErrorResponse/"ErrNotAllFound should be a 404 code"`
    #[tokio::test]
    async fn error_response_err_not_all_found_should_be_a_404_code() {
        let error = StoreError::NotAllFound {
            entity: "block",
            ids: vec!["1".to_owned(), "2".to_owned()],
        };
        let message = "not all instances of {block} in {1, 2} found";
        check_error_response(error.into(), 404, message).await;
    }

    /// `TestErrorResponse/"sql.ErrNoRows should be a 404 code"`
    #[tokio::test]
    async fn error_response_sql_err_no_rows_should_be_a_404_code() {
        check_error_response(StoreError::NoRows.into(), 404, "rows").await;
        check_error_response(StoreError::NoRows.into(), 404, "sql: no rows in result set").await;
    }

    /// `TestErrorResponse/"ErrNotFound should be a 404 code"`, the case of
    /// `ErrCategoryDeleted`. The port has no error type for it; the call site builds the
    /// "not found" with the original's message.
    #[tokio::test]
    async fn error_response_err_category_deleted_should_be_a_404_code() {
        let error = AppError::NotFound("category is deleted".to_owned());
        check_error_response(error, 404, "category is deleted").await;
    }

    /// `TestErrorResponse/"ErrRequestEntityTooLarge should be a 413 code"`
    #[tokio::test]
    async fn error_response_err_request_entity_too_large_should_be_a_413_code() {
        let error = AppError::RequestEntityTooLarge("request entity too large".to_owned());
        check_error_response(error, 413, "entity too large").await;
    }

    /// `TestErrorResponse/"ErrNotImplemented should be a 501 code"`
    #[tokio::test]
    async fn error_response_err_not_implemented_should_be_a_501_code() {
        let error = AppError::not_implemented("not permitted in standalone mode");
        check_error_response(error, 501, "standalone mode").await;
    }

    /// `TestErrorResponse/"Any other error should be a 500 code"`. The cause stays in the
    /// log; the client gets the fixed message.
    #[tokio::test]
    async fn error_response_any_other_error_should_be_a_500_code() {
        check_error_response(
            AppError::internal("http handler panic"),
            500,
            "internal server error",
        )
        .await;

        let response = AppError::internal("the cause").into_response();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("the body is read");
        assert_eq!(
            String::from_utf8_lossy(&body),
            r#"{"error":"internal server error","errorCode":500}"#,
            "the whole body"
        );

        // Errors of the store that are no client's fault end up here too.
        for error in [
            StoreError::UserNotFound("nobody".to_owned()),
            StoreError::Config("x".to_owned()),
        ] {
            check_error_response(error.into(), 500, "internal server error").await;
        }
    }

    #[test]
    fn statuses_follow_the_classification() {
        let cases = [
            (AppError::bad_request("a"), 400),
            (AppError::unauthorized("a"), 401),
            (AppError::forbidden("a"), 403),
            (AppError::not_found("user ID=1"), 404),
            (AppError::RequestEntityTooLarge("a".to_owned()), 413),
            (AppError::not_implemented("a"), 501),
            (AppError::internal("a"), 500),
        ];
        for (error, status) in cases {
            assert_eq!(error.status().as_u16(), status, "{error}");
        }
        assert_eq!(
            AppError::not_found("user ID=1").to_string(),
            "{user ID=1} not found"
        );
    }

    #[test]
    fn context_wraps_the_message_and_keeps_the_status() {
        let error = AppError::from(StoreError::NotFound("user ID=1".to_owned()))
            .context("unable to find user");
        assert!(error.is_not_found(), "still a 404: {error:?}");
        assert_eq!(
            error.to_string(),
            "unable to find user: {user ID=1} not found"
        );
    }

    #[test]
    fn store_errors_map_like_the_original_classification() {
        let too_large = StoreError::InvalidBlock {
            id: "b".to_owned(),
            source: BlockError::TitleSizeLimitExceeded,
        };
        let error = AppError::from(too_large);
        assert_eq!(error.status().as_u16(), 400);
        assert_eq!(
            error.to_string(),
            "error validating block b: block title size limit exceeded"
        );

        let no_board = StoreError::InvalidBlock {
            id: "b".to_owned(),
            source: BlockError::EmptyBoardId,
        };
        assert_eq!(AppError::from(no_board).status().as_u16(), 500);

        let not_all = StoreError::NotAllFound {
            entity: "block",
            ids: vec!["a".to_owned(), "b".to_owned()],
        };
        let error = AppError::from(not_all);
        assert_eq!(error.status().as_u16(), 404);
        assert_eq!(
            error.to_string(),
            "not all instances of {block} in {a, b} found"
        );

        assert_eq!(
            AppError::from(StoreError::Config("x".to_owned()))
                .status()
                .as_u16(),
            500
        );
    }
}
