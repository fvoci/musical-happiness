//! Identifier generation.

use uuid::Uuid;

/// The kind of entity an identifier names. It decides the identifier's first character.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IdKind {
    /// No specific kind; also used for categories and uploaded file names.
    None,
    /// A team.
    Team,
    /// A board.
    Board,
    /// A card block.
    Card,
    /// A view block.
    View,
    /// A login session.
    Session,
    /// A user.
    User,
    /// A session token.
    Token,
    /// A text, checkbox, comment or divider block.
    Block,
    /// An image or attachment block.
    Attachment,
}

impl IdKind {
    /// Returns the character that identifiers of this kind start with.
    pub const fn prefix(self) -> char {
        match self {
            Self::None => '7',
            Self::Team => 't',
            Self::Board => 'b',
            Self::Card => 'c',
            Self::View => 'v',
            Self::Session => 's',
            Self::User => 'u',
            Self::Token => 'k',
            Self::Block => 'a',
            Self::Attachment => 'i',
        }
    }
}

/// The base32 alphabet of the original identifiers (z-base-32).
const ALPHABET: &[u8; 32] = b"ybndrfg8ejkmcpqxot1uwisza345h769";

/// Length of an identifier: the kind prefix plus 26 base32 characters.
const ID_LEN: usize = 27;

/// Returns a new random identifier of the given kind.
///
/// The identifier is 27 characters long: the kind's prefix followed by a random version 4
/// UUID in unpadded z-base-32.
pub fn new_id(kind: IdKind) -> String {
    let mut id = String::with_capacity(ID_LEN);
    id.push(kind.prefix());
    encode_base32(Uuid::new_v4().as_bytes(), &mut id);
    id
}

/// Appends the unpadded base32 encoding of `bytes` to `out`.
fn encode_base32(bytes: &[u8], out: &mut String) {
    let mut buffer: usize = 0;
    let mut bits: u32 = 0;
    for &byte in bytes {
        buffer = (buffer << 8) | usize::from(byte);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(char::from(ALPHABET[(buffer >> bits) & 0x1f]));
        }
        // Keep only the bits not yet emitted so the buffer never grows.
        buffer &= (1 << bits) - 1;
    }
    if bits > 0 {
        out.push(char::from(ALPHABET[(buffer << (5 - bits)) & 0x1f]));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base32_matches_reference_vectors() {
        // Reference values from the z-base-32 alphabet applied to RFC 4648 bit grouping.
        let cases: [(&[u8], &str); 4] = [
            (b"", ""),
            (&[0x00], "yy"),
            (&[0xff], "9h"),
            (&[0xf0, 0xbf, 0xc7], "6n9hq"),
        ];
        for (input, want) in cases {
            let mut got = String::new();
            encode_base32(input, &mut got);
            assert_eq!(got, want, "input {input:02x?}");
        }
    }

    #[test]
    fn new_id_is_prefix_plus_26_alphabet_characters() {
        for kind in [IdKind::None, IdKind::Board, IdKind::Attachment] {
            let id = new_id(kind);
            assert_eq!(id.len(), ID_LEN, "id {id}");
            assert!(id.starts_with(kind.prefix()), "id {id}");
            assert!(
                id.bytes().skip(1).all(|b| ALPHABET.contains(&b)),
                "id {id} has a character outside the alphabet"
            );
        }
    }

    #[test]
    fn new_ids_differ() {
        assert_ne!(new_id(IdKind::Card), new_id(IdKind::Card));
    }
}
