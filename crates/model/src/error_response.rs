//! The body of an error response.

use serde::{Deserialize, Serialize};

/// The JSON body the server sends with every error status of the API.
///
/// A client decides by `error` and, where there is one, `reason`; both are stable.
/// `message` is English text for logs and developers and may change.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(default)]
pub struct ErrorBody {
    /// The kind of error, one per status code: `bad_request`, `unauthorized`,
    /// `forbidden`, `not_found`, `method_not_allowed`, `conflict`, `payload_too_large`,
    /// `rate_limited`, `internal`.
    pub error: String,
    /// What went wrong, in English. Not for showing to users, and without internals.
    pub message: String,
    /// Why, where a client must tell causes of the same kind apart, such as
    /// `invalid_credentials` or `last_admin`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// The member of the request that was refused.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    /// The limit that was exceeded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u64>,
    /// How many seconds to wait before trying again, as in the `Retry-After` header.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry_after: Option<u64>,
    /// The identifier of the request, the same as in the `x-request-id` response header
    /// and in the server's request log.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
}
