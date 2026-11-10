//! The body of an error response.

use serde::{Deserialize, Serialize};

/// The JSON body the server sends with every error status.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ErrorResponse {
    /// A message describing the error.
    pub error: String,
    /// The HTTP status code, repeated.
    pub error_code: u16,
}
