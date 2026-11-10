//! Secrets: values that are a credential to whoever holds them.
//!
//! Session tokens, signup tokens, the read tokens of shared boards and the names files
//! are stored under are secrets. They do not come from the identifier generator, whose
//! values start with the time and continue a counter within a millisecond; they are 256
//! bits from the operating system's random number generator.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;

/// The number of random bytes in a secret.
const SECRET_BYTES: usize = 32;

/// The length of a secret's text: 32 bytes in unpadded base64url.
pub const SECRET_LENGTH: usize = 43;

/// Returns a new secret: 43 characters of the URL-safe base64 alphabet.
///
/// # Panics
///
/// Panics if the operating system cannot supply random bytes. There is nothing to fall
/// back on that would still be a secret.
pub fn new_secret() -> String {
    let mut bytes = [0_u8; SECRET_BYTES];
    getrandom::fill(&mut bytes)
        .expect("the operating system's random number generator supplies bytes");
    URL_SAFE_NO_PAD.encode(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_secret_is_43_url_safe_characters() {
        let secret = new_secret();
        assert_eq!(secret.len(), SECRET_LENGTH, "secret {secret}");
        assert!(
            secret
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_'),
            "secret {secret}"
        );
    }

    #[test]
    fn secrets_differ_and_are_no_identifiers() {
        let secrets: std::collections::HashSet<String> = (0..100).map(|_| new_secret()).collect();
        assert_eq!(secrets.len(), 100, "no two secrets are the same");
        assert!(
            secrets.iter().all(|secret| !fvoci_model::is_id(secret)),
            "a secret does not have the form of an identifier"
        );
    }
}
