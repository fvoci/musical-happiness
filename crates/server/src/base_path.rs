//! The path the server is served under: `FVOCI_HTTP_SERVER_BASEPATH`.

use anyhow::bail;

use crate::BASE_PATH_ENV;

/// The path the web app and the API are served under (`FVOCI_HTTP_SERVER_BASEPATH`),
/// as the server uses it: without a final `/`, empty for none.
///
/// # Errors
///
/// Returns an error for what the router would read as a pattern or could never match,
/// with words, rather than a panic of the router or a server that answers 404 to
/// everything.
pub fn checked_base_path(base_path: &str) -> anyhow::Result<&str> {
    let base_path = base_path.trim_end_matches('/');
    let is_malformed = !base_path.starts_with('/')
        || base_path.contains(['{', '}', '*', '?', '#', '\\'])
        || base_path.contains("//")
        || base_path.contains("/:")
        || base_path.chars().any(char::is_whitespace);
    if !base_path.is_empty() && is_malformed {
        bail!(
            "{BASE_PATH_ENV} must be empty or an absolute path such as /boards, not {base_path:?}"
        );
    }
    Ok(base_path)
}
