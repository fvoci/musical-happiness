//! What tells whether the search server holds a card as it is now.

use std::fmt::Write as _;

use sha2::{Digest as _, Sha256};

/// How many bytes of the hash a digest keeps: enough that two texts of one card never
/// share one by chance.
const DIGEST_BYTES: usize = 16;

/// The digest of a title and a text: what a document is indexed with, so that a
/// search can tell whether what the search server holds of a card is the card as it
/// is now.
pub(crate) fn digest(title: &str, text: &str) -> String {
    let mut hasher = Sha256::new();
    // The length first: no title and text read as another title and text.
    hasher.update(u64::try_from(title.len()).unwrap_or(u64::MAX).to_be_bytes());
    hasher.update(title.as_bytes());
    hasher.update(text.as_bytes());
    let mut digest = String::with_capacity(2 * DIGEST_BYTES);
    for byte in hasher.finalize().iter().take(DIGEST_BYTES) {
        // Writing to a string cannot fail.
        let _ = write!(digest, "{byte:02x}");
    }
    digest
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_digest_tells_a_title_and_a_text_from_any_other() {
        assert_eq!(digest("ab", "c"), digest("ab", "c"));
        assert_ne!(digest("ab", "c"), digest("a", "bc"));
        assert_ne!(digest("ab", "c"), digest("ab", "d"));
        assert_eq!(digest("", "").len(), 32);
    }
}
