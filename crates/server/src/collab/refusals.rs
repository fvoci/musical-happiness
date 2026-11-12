//! What the server answers when a worker refuses a request about a document, or cannot
//! answer it.

use crate::collab_worker::wire::{Limits, Refusal, Refused};
use crate::error::{AppError, Reason};

/// The error a refusal of the worker stands for. What the worker said goes to the log,
/// not to the client.
pub(super) fn refused(refused: &Refused, limits: &Limits) -> AppError {
    let error = match refused.refusal {
        Refusal::Unreadable | Refusal::Unappliable | Refusal::NoBody => {
            AppError::bad_request("the document does not take the update")
                .with_reason(Reason::UpdateRefused)
        }
        Refusal::MissingChanges => AppError::conflict(
            "the update builds on changes the document does not have; load the document again",
        )
        .with_reason(Reason::UpdateOutOfStep),
        Refusal::UpdateTooLarge => AppError::too_large(
            format_args!(
                "the update is larger than the {} bytes the server takes",
                limits.update_bytes
            ),
            limits.update_bytes,
        )
        .with_reason(Reason::UpdateTooLarge),
        Refusal::DocumentTooLarge => AppError::too_large(
            "with the update the document would be larger than it may be",
            limits.state_bytes,
        )
        .with_reason(Reason::DocumentTooLarge),
        Refusal::BodyTooLarge => AppError::too_large(
            "with the update the body would be larger than it may be",
            limits.body_bytes,
        )
        .with_reason(Reason::DocumentTooLarge),
        Refusal::TooManyDocuments => {
            AppError::service_unavailable("the server holds as many documents as it may")
                .with_reason(Reason::TooManyDocuments)
        }
        Refusal::UnknownDocument | Refusal::OutOfOrder => {
            return AppError::internal(format_args!("the worker refused a request: {refused}"));
        }
    };
    error.context(format_args!("the worker refused it: {refused}"))
}

/// The error of a document that cannot be worked on for now.
pub(super) fn unavailable() -> AppError {
    AppError::service_unavailable("the document cannot be worked on for now; try again later")
        .with_reason(Reason::DocumentUnavailable)
}
