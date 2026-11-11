//! The body of an error response: the problem details of RFC 9457.

use serde::{Deserialize, Serialize};

/// The media type of a [`Problem`].
pub const PROBLEM_MEDIA_TYPE: &str = "application/problem+json";

/// The JSON body the server sends with every error status of the API, as
/// `application/problem+json` (RFC 9457).
///
/// A client decides by `status` and, where there is one, `reason`; both are stable,
/// and so is `type`. `title` and `detail` are English text for logs and developers.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(default)]
pub struct Problem {
    /// What kind of problem this is: `/problems/<name>`, a reference relative to the
    /// server, where the server describes the kind. The same kind of problem has the
    /// same `type` every time. The name is that of the `reason` where there is one,
    /// else that of the status, written with hyphens: `/problems/last-admin`,
    /// `/problems/not-found`.
    #[serde(rename = "type")]
    pub problem_type: String,
    /// The name of the `type`, in English; the same for every problem of the type.
    pub title: String,
    /// The HTTP status of the answer.
    pub status: u16,
    /// What went wrong this time, in English. Not for showing to users, and without
    /// internals.
    pub detail: String,
    /// The request this answer is to: `urn:uuid:` and the `request_id`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance: Option<String>,
    /// Why, where a client must tell causes of the same status apart, such as
    /// `invalid_credentials` or `last_admin`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// What of the request was refused, one entry for each: every member that a check
    /// of the request's own shape refuses, or the one that a later check does.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<InvalidInput>,
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

/// One thing of a request that was refused, and where in the request it is: exactly
/// one of `pointer`, `parameter` and `header` says.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(default)]
pub struct InvalidInput {
    /// A JSON Pointer (RFC 6901) to the member of the request body: `/username`,
    /// `/updatedFields/theme`. The empty pointer is the body as a whole.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pointer: Option<String>,
    /// The name of the parameter of the query or of the path.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parameter: Option<String>,
    /// The name of the request header.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub header: Option<String>,
    /// What is wrong with it, in English. Not for showing to users.
    pub detail: String,
    /// Why it was refused, stable: the `reason` of the problem where it has one, a
    /// cause of its own such as `required` or `too_short` where a check tells them
    /// apart, else `invalid`.
    pub reason: String,
}

/// Writes the name of a member of a JSON object as a JSON Pointer to it (RFC 6901):
/// `a/b` is `/a~1b`.
#[must_use]
pub fn json_pointer(member: &str) -> String {
    format!("/{}", pointer_token(member))
}

/// Writes one reference token of a JSON Pointer: `~` is `~0` and `/` is `~1`.
#[must_use]
pub fn pointer_token(name: &str) -> String {
    name.replace('~', "~0").replace('/', "~1")
}

#[cfg(test)]
mod tests {
    use super::{json_pointer, pointer_token};

    #[test]
    fn a_member_name_is_escaped_in_a_pointer() {
        assert_eq!(json_pointer("username"), "/username");
        assert_eq!(json_pointer("a/b~c"), "/a~1b~0c");
        assert_eq!(json_pointer(""), "/", "the member with the empty name");
        assert_eq!(pointer_token("~1"), "~01", "the tilde first");
    }
}
